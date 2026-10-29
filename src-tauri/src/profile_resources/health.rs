//! Opportunistic peripheral readiness never waits behind registry adoption
//! while its caller holds the native profile coordinator.
use super::*;
use std::sync::TryLockError;

impl Runtime {
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
