use super::*;
use output::ScopeNotice;

#[derive(Default)]
pub(super) struct FocusWatch {
    #[cfg(windows)]
    native: Option<osg_desktop_output::windows::focus::ScopeWatch>,
    consumed_loss: u64,
    healthy: bool,
    last: Option<ScopeNotice>,
}
#[cfg(windows)]
impl From<osg_desktop_output::windows::focus::ScopeSnapshot> for ScopeNotice {
    fn from(value: osg_desktop_output::windows::focus::ScopeSnapshot) -> Self {
        Self { watch_id: value.watch_id, sequence: value.sequence, loss_epoch: value.loss_epoch,
            eligible: value.eligible, error: value.error }
    }
}
impl FocusWatch {
    fn snapshot(&self) -> Result<ScopeNotice, String> {
        #[cfg(windows)]
        if let Some(watch) = &self.native { return watch.snapshot().map(Into::into); }
        Err("Desktop scope observer is unavailable.".into())
    }
}
impl Desktop {
    /// Healthy native subscriptions supply scope changes; the host may use this
    /// to distinguish a notification source from a service polling fallback.
    pub fn notification_driven(&self) -> bool {
        self.focus_watch.healthy && self.focus_watch.snapshot().is_ok()
    }
    pub(super) fn scope_notifications(&self) -> Value {
        let current = self.focus_watch.snapshot();
        let notice = current.as_ref().ok().or(self.focus_watch.last.as_ref());
        json!({"watchId": notice.map(|notice| notice.watch_id),
            "sequence": notice.map(|notice| notice.sequence),
            "lossEpoch": notice.map(|notice| notice.loss_epoch),
            "eligible": notice.map(|notice| notice.eligible),
            "nativeHealthy": self.focus_watch.healthy && current.is_ok()
                && notice.is_some_and(|notice| notice.loss_epoch == self.focus_watch.consumed_loss)})
    }

    pub(super) fn register_scope_watch(&mut self) -> Result<ScopeNotice, String> {
        #[cfg(test)]
        if !self.output.asynchronous() {
            return Ok(ScopeNotice { watch_id: 0, sequence: 0, loss_epoch: 0, eligible: false, error: None });
        }
        if self.focus_watch.healthy { return self.focus_watch.snapshot(); }
        self.retire_scope_watch();
        #[cfg(windows)]
        {
            let scope = self.scope.clone().ok_or("A verified desktop scope is required.")?;
            let revoke = self.output.scope_revoker(self.run.clone());
            let watch = osg_desktop_output::windows::focus::ScopeWatch::open(scope,
                std::sync::Arc::new(move |notice| revoke.observe(notice.into())))?;
            self.output.install_scope_watch(watch.id());
            self.focus_watch.native = Some(watch);
            let notice = match self.focus_watch.snapshot() {
                Ok(notice) => notice,
                Err(error) => { self.retire_scope_watch(); return Err(error); }
            };
            self.output.scope_revoker(self.run.clone()).observe(notice.clone());
            self.output.acknowledge_scope(&notice);
            self.focus_watch.consumed_loss = notice.loss_epoch;
            self.focus_watch.last = Some(notice.clone());
            self.focus_watch.healthy = true;
            Ok(notice)
        }
        #[cfg(not(windows))]
        Err("Desktop scope observation requires Windows.".into())
    }
    pub(super) fn retire_scope_watch(&mut self) {
        // Invalidate the callback lease before native unsubscribe/drop can wait.
        self.output.retire_scope_watch();
        self.focus_watch = FocusWatch::default();
    }
    pub(super) fn poll_scope(&mut self, admissions: &[Admission], reducer: &Phase) -> Result<Vec<String>, String> {
        if !self.started { return Ok(vec![]); }
        let notice = match self.focus_watch.snapshot() {
            Ok(notice) => notice,
            Err(error) => {
                self.focus_watch.healthy = false;
                self.output.pause(self.run.clone())?;
                self.poll_admissions_focused(&[], reducer, false)?;
                self.last_error = Some(error.clone());
                return Err(error);
            }
        };
        self.poll_scope_notice(admissions, reducer, notice)
    }
    fn poll_scope_notice(&mut self, admissions: &[Admission], reducer: &Phase, notice: ScopeNotice) -> Result<Vec<String>, String> {
        self.focus_watch.last = Some(notice.clone());
        // snapshot() can race a callback; sequence fencing inside the Output
        // owner prevents an older read from overwriting newer revocation.
        self.output.scope_revoker(self.run.clone()).observe(notice.clone());
        let lost = notice.loss_epoch != self.focus_watch.consumed_loss;
        if lost {
            self.poll_admissions_focused(&[], reducer, false)?;
            self.focus_watch.consumed_loss = notice.loss_epoch;
        }
        self.output.acknowledge_scope(&notice);
        let eligibility = notice.error.map_or(Ok(notice.eligible), Err);
        self.poll_admissions_checked(if lost { &[] } else { admissions }, reducer, eligibility)
    }
}
impl Drop for Desktop {
    fn drop(&mut self) { self.retire_scope_watch(); }
}

#[cfg(test)]
#[path = "focus_tests.rs"]
mod tests;
