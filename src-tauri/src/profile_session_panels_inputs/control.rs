use super::*;

impl Mailbox {
    pub(super) fn notify(&self) {
        if let Some(wake) = &self.wake { wake.notify(); }
        if let Some(wake) = self.baseline_wake.upgrade() { wake.notify(); }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_waiter_observes_mailbox_commits_without_replacing_runtime_wake() {
        let runtime = Arc::new(NativeWake::default());
        let baseline = Arc::new(NativeWake::default());
        let mailbox = Mailbox {
            wake: Some(runtime.clone()),
            baseline_wake: Arc::downgrade(&baseline),
            ..Mailbox::default()
        };
        mailbox.notify();
        assert_eq!(runtime.observed(), 1);
        assert_eq!(baseline.observed(), 1);
        drop(baseline);
        assert!(mailbox.baseline_wake.upgrade().is_none());
        mailbox.notify();
        assert_eq!(runtime.observed(), 2);
    }
}
