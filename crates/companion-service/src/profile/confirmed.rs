//! Assignment authority is immutable between durable record/content edits.
//! A confirmed receipt survives unrelated wire polling without treating mutex
//! contention as revocation. NativeFeed still checks actual writer liveness.
use super::*;
use std::sync::TryLockError;

const MAX_CONFIRMED: usize = 128;

fn key(receipt: &AssignmentReceipt) -> Result<String, String> {
    Ok(hash(&serde_json::to_vec(receipt).map_err(|error| error.to_string())?))
}

impl Hub {
    fn remember_assignment(&self, receipt: &AssignmentReceipt) -> Result<(), String> {
        // The caller still owns Hub.inner. Never acquire Hub.inner while
        // holding this cache, and never wait for a contended cache on health.
        let revision = self.record_revision.load(Ordering::Acquire);
        let mut confirmed = match self.confirmed_assignments.try_lock() {
            Ok(confirmed) => confirmed,
            Err(TryLockError::WouldBlock) => return Ok(()),
            Err(TryLockError::Poisoned(poison)) => poison.into_inner(),
        };
        confirmed.retain(|_, checked| *checked == revision);
        if confirmed.len() >= MAX_CONFIRMED { confirmed.clear(); }
        confirmed.insert(key(receipt)?, revision);
        Ok(())
    }

    fn assignment_confirmed(&self, receipt: &AssignmentReceipt) -> Result<bool, String> {
        let revision = self.record_revision.load(Ordering::Acquire);
        let confirmed = match self.confirmed_assignments.try_lock() {
            Ok(confirmed) => confirmed,
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Poisoned(poison)) => poison.into_inner(),
        };
        Ok(confirmed.get(&key(receipt)?) == Some(&revision)
            && self.record_revision.load(Ordering::Acquire) == revision)
    }

    pub fn validate_profile_assignment(&self, receipt: &AssignmentReceipt) -> Result<(), String> {
        let inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        owned::validate_owned(&inner, receipt)?;
        self.remember_assignment(receipt)
    }

    /// Reuse only a strict check of this exact receipt at the current edit
    /// epoch. A changed record/content revision requires strict revalidation;
    /// unrelated wire/session traffic never fabricates assignment loss.
    pub fn try_validate_profile_assignment(&self, receipt: &AssignmentReceipt) -> Result<Option<()>, String> {
        let inner = match self.inner.try_lock() {
            Ok(inner) => inner,
            Err(TryLockError::WouldBlock) => return Ok(self.assignment_confirmed(receipt)?.then_some(())),
            Err(TryLockError::Poisoned(poison)) => poison.into_inner(),
        };
        owned::validate_owned(&inner, receipt)?;
        self.remember_assignment(receipt)?;
        Ok(Some(()))
    }
}

#[cfg(test)]
#[path = "confirmed_tests.rs"]
mod tests;
