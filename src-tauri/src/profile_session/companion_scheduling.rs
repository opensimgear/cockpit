//! Source clocks and actual output/retry deadlines replace host acquisition ticks.
use super::*;

impl CompanionSources {
    pub(crate) fn set_wake(&mut self, wake: Option<Arc<osg_streams::NativeWake>>) {
        self.feedback_worker.set_wake(wake.clone());
        self.wake = wake;
    }

    pub(crate) fn notification_driven(&self) -> bool {
        self.instances.is_empty() || self.wake.is_some() && self.acquired && !self.stopped
            && self.instances.iter().all(|instance|
                instance.feed.as_ref().is_some_and(NativeFeed::notification_driven))
    }

    pub(crate) fn needs_delivery_retry(&self) -> bool {
        self.acquired && !self.stopped && self.instances.iter().any(|instance|
            !instance.ready || instance.inputs.iter().any(|input| input.token.is_none())
                || instance.outputs.iter().any(|output| output.token.is_none()))
    }

    pub(crate) fn scheduled_poll_complete(&mut self, now: u64) {
        // Notifications can arrive while Hub or inventory commits still own
        // their mutex. Retry only an observed contention/clear obligation;
        // held controls awaiting neutral rely on their next input notification.
        let pending = self.instances.iter().any(|instance|
            instance.validation_pending || instance.feedback_clear_pending);
        self.retry_at = pending.then(|| now.saturating_add(5_000));
    }

    pub(crate) fn next_deadline_us(&self) -> Option<u64> {
        if !self.acquired || self.stopped { return None; }
        self.instances.iter().filter(|instance| instance.ready || instance.metadata_paused).filter_map(|instance|
            instance.feed.as_ref().and_then(NativeFeed::next_deadline_us))
            .chain(self.retry_at).min()
    }

    pub(crate) fn feedback_deadline(&self) -> Option<Instant> {
        if !self.acquired || self.stopped || self.feedback_worker.busy() { return None; }
        self.instances.iter().filter(|instance|
            instance.ready && !instance.feedback_clear_pending && !instance.outputs.is_empty())
            .map(|instance| instance.feedback_at.map_or_else(Instant::now,
                |last| last + feedback::MIN_PUBLISH_INTERVAL)).min()
    }
}
