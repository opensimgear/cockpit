//! Native helper subscriptions. Reads use retained notification evidence only;
//! the adapter's immediate eligibility check remains a separate admission gate.
use super::focus::{ScopeSnapshot, ScopeWatch};
use crate::protocol::{Scope, ScopeEvidence};
use std::{collections::BTreeMap, sync::Arc};

const SCOPE_LIMIT: usize = 64;
type Wake = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
pub(super) struct Scopes {
    watches: BTreeMap<u64, ScopeWatch>,
    wake: Option<Wake>,
}

fn validate_registration(registration: u64, duplicate: bool, count: usize) -> Result<(), String> {
    if registration == 0 {
        return Err("Desktop scope registration must be nonzero.".into());
    }
    if duplicate {
        return Err("Desktop scope registration is already bound.".into());
    }
    if count >= SCOPE_LIMIT {
        return Err("Desktop helper scope registration limit exceeded.".into());
    }
    Ok(())
}

fn evidence(snapshot: ScopeSnapshot) -> ScopeEvidence {
    ScopeEvidence {
        loss_epoch: snapshot.loss_epoch,
        eligible: snapshot.eligible && snapshot.error.is_none(),
    }
}

impl Scopes {
    /// The helper installs its owned native event waker before registering runs.
    pub(super) fn set_wake(&mut self, wake: Wake) {
        self.wake = Some(wake);
    }

    pub(super) fn bind(
        &mut self,
        registration: u64,
        scope: &Scope,
    ) -> Result<ScopeEvidence, String> {
        validate_registration(
            registration,
            self.watches.contains_key(&registration),
            self.watches.len(),
        )?;
        let wake = self
            .wake
            .clone()
            .ok_or("Desktop helper scope wake is not installed.")?;
        // Callback identity is not authority: it only wakes the helper, which
        // reads its current registration's watch. A late wake is harmless.
        let watch = ScopeWatch::open(scope.clone(), Arc::new(move |_| wake()))?;
        // Keep registration atomic on observer failure. The local watch drops
        // and retires its subscription if this initial snapshot fails.
        let initial = evidence(watch.snapshot()?);
        self.watches.insert(registration, watch);
        Ok(initial)
    }

    pub(super) fn evidence(&self, registration: u64) -> Result<ScopeEvidence, String> {
        let watch = self
            .watches
            .get(&registration)
            .ok_or("Desktop helper scope registration is not bound.")?;
        // snapshot() reports terminal observer failure as Err. A transient
        // inspection error is a healthy, ineligible notification boundary.
        watch.snapshot().map(evidence)
    }

    pub(super) fn retire(&mut self, registration: u64) {
        // Remove identity before unsubscribe can wait for native shutdown.
        let retired = self.watches.remove(&registration);
        drop(retired);
    }
}

#[cfg(test)]
#[path = "worker_scope_tests.rs"]
mod tests;
