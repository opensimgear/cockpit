use crate::{
    codec::{encode_limited, map_encode_error},
    journal::{overflow_with_reason, queued_snapshot, EncodedSnapshot, Stream},
    Loss, LossReason, PublishError,
};
use serde::{de::DeserializeOwned, Serialize};

impl<S, E> Stream<S, E>
where
    S: Serialize + DeserializeOwned,
    E: Serialize + DeserializeOwned,
{
    /// Install a newer source generation and its atomic baseline. A generation
    /// change invalidates every old subscription cursor before new data flows.
    pub fn replace_generation(
        &self,
        generation: u64,
        snapshot: S,
        captured_us: u64,
        clock_id: impl Into<String>,
    ) -> Result<u64, PublishError> {
        let clock_id = clock_id.into();
        super::journal::validate_clock(&clock_id)?;
        let bytes = encode_limited(&snapshot, self.shared.limits.max_snapshot_bytes)
            .map_err(|error| map_encode_error(error, PublishError::SnapshotTooLarge))?;
        let mut state = self.shared.lock();
        if state.closed {
            return Err(PublishError::Closed);
        }
        if generation <= state.generation {
            return Err(PublishError::StaleGeneration);
        }
        let revision = state
            .revision
            .checked_add(1)
            .ok_or(PublishError::CounterExhausted)?;
        state.generation = generation;
        state.revision = revision;
        state.sequence = 0;
        state.snapshot = EncodedSnapshot {
            captured_us,
            clock_id,
            bytes: bytes.into(),
        };
        let current_snapshot = queued_snapshot(&state);
        for subscriber in state.subscribers.values_mut() {
            subscriber.snapshot = None;
            subscriber.loss = None;
            overflow_with_reason(
                subscriber,
                Loss {
                    generation,
                    revision,
                    reason: LossReason::GenerationReplaced,
                },
            );
            subscriber.snapshot = Some(current_snapshot.clone());
            subscriber.blocked = false;
        }
        drop(state);
        self.shared.wake.notify_all();
        Ok(revision)
    }

    /// Replace the atomic baseline after a discontinuity without changing the
    /// source generation. Existing subscribers receive loss, then this baseline.
    pub fn resynchronize(
        &self,
        expected_generation: u64,
        snapshot: S,
        captured_us: u64,
        clock_id: impl Into<String>,
    ) -> Result<u64, PublishError> {
        let clock_id = clock_id.into();
        super::journal::validate_clock(&clock_id)?;
        let bytes = encode_limited(&snapshot, self.shared.limits.max_snapshot_bytes)
            .map_err(|error| map_encode_error(error, PublishError::SnapshotTooLarge))?;
        let mut state = self.shared.lock();
        if state.closed {
            return Err(PublishError::Closed);
        }
        if expected_generation != state.generation {
            return Err(PublishError::StaleGeneration);
        }
        let revision = state
            .revision
            .checked_add(1)
            .ok_or(PublishError::CounterExhausted)?;
        state.revision = revision;
        state.snapshot = EncodedSnapshot {
            captured_us,
            clock_id,
            bytes: bytes.into(),
        };
        let generation = state.generation;
        let current_snapshot = queued_snapshot(&state);
        for subscriber in state.subscribers.values_mut() {
            subscriber.snapshot = None;
            subscriber.loss = None;
            overflow_with_reason(
                subscriber,
                Loss {
                    generation,
                    revision,
                    reason: LossReason::BaselineReset,
                },
            );
            subscriber.snapshot = Some(current_snapshot.clone());
            subscriber.blocked = false;
        }
        drop(state);
        self.shared.wake.notify_all();
        Ok(revision)
    }

    pub fn revision(&self) -> u64 {
        self.shared.lock().revision
    }

    pub fn generation(&self) -> u64 {
        self.shared.lock().generation
    }

    pub fn close(&self) {
        let mut state = self.shared.lock();
        state.closed = true;
        let generation = state.generation;
        let revision = state.revision;
        for subscriber in state.subscribers.values_mut() {
            subscriber.snapshot = None;
            subscriber.loss = None;
            overflow_with_reason(
                subscriber,
                Loss {
                    generation,
                    revision,
                    reason: LossReason::Closed,
                },
            );
        }
        drop(state);
        self.shared.wake.notify_all();
    }
}

impl crate::journal::Cancellation {
    /// Remove a subscriber and wake any thread blocked in `recv`.
    pub fn cancel(&self) {
        self.shared.remove_reader(self.id);
    }
}
