use super::*;

impl Mailbox {
    pub(super) fn notify(&self) {
        if let Some(wake) = &self.wake { wake.notify(); }
    }
    pub(super) fn control(&self) {
        for wake in self.controls.iter().filter_map(Weak::upgrade) { wake.notify(); }
    }
    pub(super) fn overflow(&mut self, logical: &str) -> Result<(), String> {
        self.loss(logical, "Panel frame queue overflowed; missing events were discarded and fresh baselines are required.")
    }
    pub(super) fn loss(&mut self, logical: &str, message: &str) -> Result<(), String> {
        self.frames.clear();
        self.baselines.clear();
        self.liveness.clear();
        self.loss_epoch = self.loss_epoch.checked_add(1).ok_or("Panel loss epoch exhausted.")?;
        self.diagnostics.insert(logical.into(), message.into());
        self.control();
        self.notify();
        Ok(())
    }
}
