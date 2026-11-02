use crate::{
    journal::{overflow_with_reason, Subscription},
    Loss, LossReason,
};

impl<S, E> Subscription<S, E> {
    pub(crate) fn fail_payload(&self, generation: u64, revision: u64) {
        let mut state = self.shared.lock();
        if state.closed || state.generation != generation {
            return;
        }
        if let Some(subscriber) = state.subscribers.get_mut(&self.id) {
            if subscriber.blocked
                || subscriber.loss.is_some()
                || subscriber
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.revision > revision)
            {
                return;
            }
            subscriber.snapshot = None;
            overflow_with_reason(
                subscriber,
                Loss {
                    generation,
                    revision,
                    reason: LossReason::DecodeFailure,
                },
            );
        }
        drop(state);
        self.shared.wake.notify_all();
    }
}
