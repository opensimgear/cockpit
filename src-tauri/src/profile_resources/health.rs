//! Opportunistic peripheral readiness never waits behind registry adoption
//! while its caller holds the native profile coordinator.
use super::*;
use std::sync::TryLockError;

impl Runtime {
    /// Inventory contention preserves the last committed exact-token result.
    /// A never-confirmed token remains pending; committed revocation is an error.
    pub(crate) fn validate_live_delivery(&self, token: &ResourceToken) -> Result<Route, String> {
        self.try_validate_live_delivery(token)?.ok_or_else(||
            "Native resource metadata is updating; waiting for current delivery validation.".into())
    }
    pub(crate) fn try_validate_live_delivery(&self, token: &ResourceToken) -> Result<Option<Route>, String> {
        if self.stop.load(Ordering::Acquire) {
            return Err("Resource inventory has stopped.".into());
        }
        let state = match self.state.try_lock() {
            Ok(state) => state,
            Err(TryLockError::WouldBlock) => return self.state.confirmed(token),
            Err(TryLockError::Poisoned(poison)) => poison.into_inner(),
        };
        let result = Self::validate_delivery_locked(&state, token);
        state.confirm(token, &result);
        result.map(Some)
    }

    pub(crate) fn try_snapshot_for_preparation(
        &self,
        assignments: Vec<osg_profiles::store::LocalAssignment>,
    ) -> Result<
        Option<(
            osg_profiles::compiler_models::CompileSnapshot,
            Vec<PortPreparation>,
        )>,
        String,
    > {
        if self.stop.load(Ordering::Acquire) {
            return Err("Resource inventory has stopped.".into());
        }
        if assignments.len() > 64
            || assignments
                .iter()
                .map(|assignment| assignment.ports.len())
                .sum::<usize>()
                > 1024
        {
            return Err("Preparation capture exceeds bounded assignments.".into());
        }
        let state = match self.state.try_lock() {
            Ok(state) => state,
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Poisoned(poison)) => poison.into_inner(),
        };
        Self::snapshot_for_preparation_locked(&state, assignments, &[]).map(Some)
    }
}

#[cfg(test)]
#[path = "health_tests.rs"]
mod tests;
