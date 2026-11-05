use super::{preparing_panel, Hub, Inner, LEASE_MS, PANEL_PREPARATION_LEASE_MS};
use mio::Waker;
use std::{
    ops::{Deref, DerefMut},
    sync::{Arc, MutexGuard},
};

const MAX_DEADLINE_OBSERVERS: usize = 16;

fn earliest(inner: &Inner) -> Option<u64> {
    let mut next = inner
        .code_offer
        .as_ref()
        .map(|offer| offer.expires)
        .into_iter()
        .chain(inner.offer.as_ref().map(|offer| offer.expires))
        .chain(inner.pending.values().map(|pending| pending.expires))
        .chain(inner.sessions.values().map(|session| {
            let lease = if preparing_panel(session) {
                PANEL_PREPARATION_LEASE_MS
            } else {
                LEASE_MS
            };
            session.last_seen.saturating_add(lease)
        }));
    next.next().map(|first| next.fold(first, u64::min))
}

pub(super) struct Mutation<'a> {
    hub: &'a Hub,
    inner: MutexGuard<'a, Inner>,
    before: Option<u64>,
}

impl Deref for Mutation<'_> {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for Mutation<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl Drop for Mutation<'_> {
    fn drop(&mut self) {
        let after = earliest(&self.inner);
        let became_earlier = match (self.before, after) {
            (None, Some(_)) => true,
            (Some(before), Some(after)) => after < before,
            _ => false,
        };
        if !became_earlier {
            return;
        }
        let mut wakes = self
            .hub
            .deadline_wakes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        wakes.retain(|wake| {
            if let Some(wake) = wake.upgrade() {
                let _ = wake.wake();
                true
            } else {
                false
            }
        });
    }
}

impl Hub {
    pub(super) fn mutation(&self) -> Mutation<'_> {
        let inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = earliest(&inner);
        Mutation { hub: self, inner, before }
    }

    /// Earliest absolute expiry in the same monotonic millisecond epoch as `now_ms`.
    pub fn next_deadline_ms(&self) -> Option<u64> {
        let inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        earliest(&inner)
    }

    /// Register the listener reactor's coalescing wake for newly earlier deadlines.
    pub fn observe_deadlines(&self, wake: Arc<Waker>) -> Result<(), String> {
        let mut wakes = self
            .deadline_wakes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        wakes.retain(|current| current.strong_count() > 0);
        let weak = Arc::downgrade(&wake);
        if wakes.iter().any(|current| current.ptr_eq(&weak)) {
            return Ok(());
        }
        if wakes.len() >= MAX_DEADLINE_OBSERVERS {
            return Err("Companion listener deadline observer limit reached.".into());
        }
        wakes.push(weak);
        wake.wake().map_err(|error| format!("Cannot wake companion listener: {error}"))
    }
}

#[cfg(test)]
#[path = "hub_deadlines_tests.rs"]
mod tests;
