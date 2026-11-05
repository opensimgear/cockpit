//! In-memory revocation shared with the native scope observer. Never drive IPC
//! here: a callback can run while the ordered owner is blocked in a request.
use super::{Output, Shared, State};
use osg_mapping::engine::RunToken;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub(in crate::desktop) struct ScopeNotice {
    pub watch_id: u64,
    pub sequence: u64,
    pub loss_epoch: u64,
    pub eligible: bool,
    pub error: Option<String>,
}
pub(super) struct ScopeAuthorization {
    id: u64,
    sequence: Option<u64>,
    loss_epoch: u64,
    acknowledged_loss: u64,
    eligible: bool,
    active: bool,
}
pub(super) fn permitted(state: &State) -> bool {
    state.scope_watch.as_ref().is_none_or(|watch|
        watch.eligible && watch.loss_epoch == watch.acknowledged_loss)
}
pub(super) fn renewable(state: &State) -> bool {
    state.scope_watch.as_ref().is_none_or(|watch| watch.loss_epoch == watch.acknowledged_loss)
}
#[derive(Clone)]
pub(in crate::desktop) struct ScopeRevoker {
    shared: Arc<Shared>,
    run: RunToken,
}
impl ScopeRevoker {
    pub(in crate::desktop) fn observe(&self, notice: ScopeNotice) {
        let mut state = self.shared.lock();
        let Some(watch) = state.scope_watch.as_mut() else { return };
        if watch.id != notice.watch_id || watch.sequence.is_some_and(|sequence| sequence >= notice.sequence) {
            return;
        }
        let active = watch.active;
        let revoke = notice.loss_epoch != watch.loss_epoch
            || (!notice.eligible || notice.error.is_some()) && watch.eligible;
        watch.sequence = Some(notice.sequence);
        watch.loss_epoch = notice.loss_epoch;
        watch.eligible = notice.eligible && notice.error.is_none();
        if revoke {
            state.epoch = state.epoch.wrapping_add(1);
            state.authorization = None;
            if active { state.pause = Some(self.run.clone()); }
        }
        drop(state);
        self.shared.notify();
    }
}
impl Output {
    pub(in crate::desktop) fn scope_revoker(&self, run: RunToken) -> ScopeRevoker {
        ScopeRevoker { shared: self.shared.clone(), run }
    }
    pub(in crate::desktop) fn install_scope_watch(&self, id: u64) {
        let mut state = self.shared.lock();
        state.epoch = state.epoch.wrapping_add(1);
        state.authorization = None;
        state.scope_watch = Some(ScopeAuthorization {
            id, sequence: None, loss_epoch: 0, acknowledged_loss: 0, eligible: false, active: false,
        });
    }
    pub(super) fn activate_scope_watch(&self, run: RunToken) {
        let mut state = self.shared.lock();
        if let Some(watch) = state.scope_watch.as_mut() { watch.active = true; }
        if !permitted(&state) { state.pause = Some(run); }
        drop(state);
        self.drive();
    }
    pub(in crate::desktop) fn acknowledge_scope(&self, notice: &ScopeNotice) {
        let mut state = self.shared.lock();
        if let Some(watch) = state.scope_watch.as_mut() {
            if watch.id == notice.watch_id && watch.sequence == Some(notice.sequence) {
                watch.acknowledged_loss = notice.loss_epoch;
            }
        }
    }
    pub(in crate::desktop) fn retire_scope_watch(&self) {
        let mut state = self.shared.lock();
        if state.scope_watch.take().is_some() {
            state.epoch = state.epoch.wrapping_add(1);
            state.authorization = None;
        }
    }
}

