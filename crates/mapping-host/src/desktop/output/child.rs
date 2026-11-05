//! Validated helper receipts revoke memory before the coordinator journals.
use super::{Output, Shared, State};
use osg_desktop_output::{coordinator::ScopeUpdate, protocol::{ScopeAdmission, ScopeReceipt}};

pub(super) fn permitted(state: &State, proof: Option<ScopeAdmission>) -> bool {
    proof.is_some() && proof == state.executing_admission
        && state.child_scope.is_some_and(|receipt| receipt.live && Some(receipt.admission) == proof)
        && state.acknowledged_child == proof
}
pub(super) fn renewable(state: &State) -> bool {
    state.child_scope.is_some_and(|receipt| receipt.live
        && state.acknowledged_child == Some(receipt.admission))
}
pub(super) fn observe(shared: &Shared, update: ScopeUpdate) {
    let mut state = shared.lock();
    if state.closed || state.shutdown || state.child_run.as_ref() != Some(&update.run) { return; }
    let changed = state.child_scope.is_some_and(|prior|
        prior.admission != update.receipt.admission || prior.live != update.receipt.live);
    state.child_scope = Some(update.receipt);
    if changed {
        state.epoch = state.epoch.wrapping_add(1);
        state.authorization = None;
        state.acknowledged_child = None;
        state.pause = Some(update.run);
    }
    if !update.receipt.live {
        state.fault = Some("Desktop helper scope registration expired.".into());
        state.authorization = None;
    }
    drop(state);
    shared.notify();
}
impl Output {
    pub(in crate::desktop) fn reject_child(&self, error: String) {
        let mut state = self.shared.lock();
        state.fault = Some(error);
        state.epoch = state.epoch.wrapping_add(1);
        state.authorization = None;
        drop(state);
        self.drive();
    }
    pub(in crate::desktop) fn child_scope(&self) -> Option<ScopeReceipt> {
        self.shared.lock().child_scope
    }
    pub(in crate::desktop) fn acknowledge_child(&self, proof: ScopeAdmission) {
        let mut state = self.shared.lock();
        if state.child_scope.is_some_and(|receipt| receipt.live && receipt.admission == proof) {
            state.acknowledged_child = Some(proof);
        }
    }
}
