//! Opt-in replacement of complete absolute state; never an ordered event path.
use super::*;
use crate::wire::EncodedPayload;
use serde_json::Value;

impl Stream<Value, Value> {
    /// The registry already encoded and bounded the complete snapshot once.
    /// No event or event-sequence number is produced by state replacement.
    pub(crate) fn update_latest_encoded(
        &self,
        generation: u64,
        snapshot: EncodedPayload,
        captured_us: u64,
        clock_id: &str,
    ) -> Result<u64, PublishError> {
        validate_clock(clock_id)?;
        let bytes = snapshot.into_bytes();
        if bytes.len() > self.shared.limits.max_snapshot_bytes {
            return Err(PublishError::SnapshotTooLarge);
        }
        let mut state = self.shared.lock();
        if state.closed {return Err(PublishError::Closed);}
        if generation != state.generation {return Err(PublishError::StaleGeneration);}
        let revision = state.revision.checked_add(1).ok_or(PublishError::CounterExhausted)?;
        // All validation precedes the first authoritative mutation.
        state.revision = revision;
        state.snapshot = EncodedSnapshot {captured_us,clock_id:clock_id.into(),bytes:bytes.into()};
        let current = queued_snapshot(&state);
        for subscriber in state.subscribers.values_mut() {
            subscriber.events.clear();
            subscriber.pending_bytes = 0;
            // Preserve a real loss boundary, including one that previously
            // followed an unread initial snapshot. A consumed loss stays
            // consumed; blocked readers resume only on this fresh baseline.
            subscriber.loss_before_snapshot = subscriber.loss.is_some();
            subscriber.snapshot = Some(current.clone());
        }
        drop(state);
        self.shared.wake.notify_all();
        Ok(revision)
    }
}

#[cfg(test)]
#[path = "latest_tests.rs"]
mod tests;
