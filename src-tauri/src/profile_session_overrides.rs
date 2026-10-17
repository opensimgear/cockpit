//! Peripheral publication overrides require an owned runtime capability.
//! Existing CAN Enable/Disable changes durable registry state and USB has no
//! publication lease. Never reinterpret either as a temporary profile override.
use osg_profiles::compiler_models::ExecutionPlan;
pub(crate) fn validate(plan: &ExecutionPlan) -> Result<(), String> {
    if plan.runtime_overrides.is_empty() {
        Ok(())
    } else {
        Err("Temporary controller enable overrides require an advertised nonpersistent publication lease with exact owner, route/boot fencing and compare-if-unchanged restoration. Existing USB and CAN firmware does not provide that capability; no persistent device setting was changed.".into())
    }
}
#[cfg(test)]
#[path = "profile_session_overrides_tests.rs"]
mod tests;
