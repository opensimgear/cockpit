use super::*;
use crate::wire::EncodedPayload;
use serde_json::Value;

impl<S, E> Stream<S, E>
where
    S: Serialize + DeserializeOwned,
    E: Serialize + DeserializeOwned,
{
    /// Commit one ordered event and the resulting complete snapshot atomically.
    pub fn publish(
        &self,
        generation: u64,
        snapshot: S,
        event: E,
        captured_us: u64,
        clock_id: impl Into<String>,
    ) -> Result<(u64, u64), PublishError> {
        self.publish_batch(
            generation,
            snapshot,
            vec![(event, captured_us, clock_id.into())],
        )
    }

    /// Atomically commit a final snapshot and an ordered event batch.
    pub fn publish_batch(
        &self,
        generation: u64,
        snapshot: S,
        events: Vec<(E, u64, String)>,
    ) -> Result<(u64, u64), PublishError> {
        if events.is_empty() {
            return Err(PublishError::EmptyBatch);
        }
        if events.len() > self.shared.limits.max_batch_frames {
            return Err(PublishError::BatchTooLarge);
        }
        let snapshot_bytes = encode_limited(&snapshot, self.shared.limits.max_snapshot_bytes)
            .map_err(|error| map_encode_error(error, PublishError::SnapshotTooLarge))?;
        let mut encoded_events = Vec::with_capacity(events.len());
        let mut batch_bytes = 0usize;
        for (value, captured_us, clock_id) in events {
            validate_clock(&clock_id)?;
            let bytes = encode_limited(&value, self.shared.limits.max_frame_bytes)
                .map_err(|error| map_encode_error(error, PublishError::FrameTooLarge))?;
            batch_bytes = batch_bytes
                .checked_add(bytes.len())
                .ok_or(PublishError::BatchTooLarge)?;
            if batch_bytes > self.shared.limits.max_batch_bytes {
                return Err(PublishError::BatchTooLarge);
            }
            encoded_events.push((Arc::<[u8]>::from(bytes), captured_us, clock_id));
        }

        self.commit_encoded(generation, snapshot_bytes.into(), encoded_events)
    }
}

impl Stream<Value, Value> {
    /// Reuse bytes already serialized and bounded by the wire validator.
    pub(crate) fn publish_encoded(
        &self,
        generation: u64,
        snapshot: EncodedPayload,
        event: EncodedPayload,
        captured_us: u64,
        clock_id: &str,
    ) -> Result<(u64, u64), PublishError> {
        self.commit_encoded(
            generation,
            snapshot.into_bytes().into(),
            vec![(event.into_bytes().into(), captured_us, clock_id.into())],
        )
    }
}

impl<S, E> Stream<S, E> {
    fn commit_encoded(
        &self,
        generation: u64,
        snapshot_bytes: Arc<[u8]>,
        encoded_events: Vec<(Arc<[u8]>, u64, String)>,
    ) -> Result<(u64, u64), PublishError> {
        // Both callers supply JSON encoded by this crate. Recheck limits here
        // so the wire fast path obeys every journal admission and queue budget.
        let limits = self.shared.limits;
        if encoded_events.is_empty() {
            return Err(PublishError::EmptyBatch);
        }
        if encoded_events.len() > limits.max_batch_frames {
            return Err(PublishError::BatchTooLarge);
        }
        if snapshot_bytes.len() > limits.max_snapshot_bytes {
            return Err(PublishError::SnapshotTooLarge);
        }
        let mut batch_bytes = 0usize;
        for (bytes, _, clock_id) in &encoded_events {
            validate_clock(clock_id)?;
            if bytes.len() > limits.max_frame_bytes {
                return Err(PublishError::FrameTooLarge);
            }
            batch_bytes = batch_bytes
                .checked_add(bytes.len())
                .ok_or(PublishError::BatchTooLarge)?;
            if batch_bytes > limits.max_batch_bytes {
                return Err(PublishError::BatchTooLarge);
            }
        }
        let mut state = self.shared.lock();
        if state.closed {
            return Err(PublishError::Closed);
        }
        if generation != state.generation {
            return Err(PublishError::StaleGeneration);
        }
        let final_revision = state
            .revision
            .checked_add(encoded_events.len() as u64)
            .ok_or(PublishError::CounterExhausted)?;
        let final_sequence = state
            .sequence
            .checked_add(encoded_events.len() as u64)
            .ok_or(PublishError::CounterExhausted)?;
        let generation = state.generation;
        let first_revision = state.revision + 1;
        let first_sequence = state.sequence + 1;
        state.snapshot = EncodedSnapshot {
            captured_us: encoded_events.last().unwrap().1,
            clock_id: encoded_events.last().unwrap().2.clone(),
            bytes: snapshot_bytes,
        };
        state.revision = final_revision;
        state.sequence = final_sequence;

        for subscriber in state.subscribers.values_mut() {
            if subscriber.blocked {
                continue;
            }
            for (index, (bytes, captured_us, clock_id)) in encoded_events.iter().enumerate() {
                let next_bytes = subscriber.pending_bytes.saturating_add(bytes.len());
                if subscriber.events.len() >= self.shared.limits.max_pending_frames
                    || next_bytes > self.shared.limits.max_pending_bytes
                {
                    overflow(subscriber, generation, final_revision);
                    break;
                }
                subscriber.pending_bytes = next_bytes;
                subscriber.events.push_back(QueuedEvent {
                    generation,
                    sequence: first_sequence + index as u64,
                    revision: first_revision + index as u64,
                    captured_us: *captured_us,
                    clock_id: clock_id.clone(),
                    bytes: bytes.clone(),
                });
            }
        }
        drop(state);
        self.shared.wake.notify_all();
        Ok((final_revision, final_sequence))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::Identity;
    use serde_json::json;

    fn encoded(value: &Value, change: bool) -> EncodedPayload {
        Identity {
            topic: "topic".into(),
            source: "source".into(),
            session: "session".into(),
        }
        .encode(value, 7, "clock", change)
        .unwrap()
    }

    #[test]
    fn validated_wire_payloads_still_obey_journal_budgets_atomically() {
        for (limits, clock, expected) in [
            (
                Limits {
                    max_snapshot_bytes: 1,
                    ..DEFAULT_LIMITS
                },
                "clock",
                PublishError::SnapshotTooLarge,
            ),
            (
                Limits {
                    max_frame_bytes: 1,
                    ..DEFAULT_LIMITS
                },
                "clock",
                PublishError::FrameTooLarge,
            ),
            (
                Limits {
                    max_batch_bytes: 1,
                    ..DEFAULT_LIMITS
                },
                "clock",
                PublishError::BatchTooLarge,
            ),
            (DEFAULT_LIMITS, "bad clock", PublishError::InvalidClockId),
        ] {
            let stream =
                Stream::<Value, Value>::with_limits(1, json!(0), 0, "clock", limits).unwrap();
            let mut reader = stream.subscribe().unwrap();
            reader.try_recv().unwrap();
            assert_eq!(
                stream.publish_encoded(
                    1,
                    encoded(&json!(42), false),
                    encoded(&json!(42), true),
                    7,
                    clock
                ),
                Err(expected)
            );
            let state = stream.shared.lock();
            assert_eq!((state.revision, state.sequence), (1, 0));
            assert_eq!(&*state.snapshot.bytes, b"0");
            drop(state);
            assert!(reader.try_recv().unwrap().is_none());
        }
    }

    #[test]
    fn encoded_commits_preserve_per_subscriber_byte_loss_and_drop_capacity() {
        let limits = Limits {
            max_pending_bytes: 5,
            max_subscribers: 2,
            ..DEFAULT_LIMITS
        };
        let stream = Stream::<Value, Value>::with_limits(1, json!(0), 0, "clock", limits).unwrap();
        let mut slow = stream.subscribe().unwrap();
        let mut fast = stream.subscribe().unwrap();
        slow.try_recv().unwrap();
        fast.try_recv().unwrap();
        for n in 1..=2 {
            stream
                .publish_encoded(
                    1,
                    encoded(&json!(n), false),
                    encoded(&json!("ab"), true),
                    7,
                    "clock",
                )
                .unwrap();
            assert!(
                matches!(fast.try_recv().unwrap(), Some(Delivery::Event(event)) if event.value == json!("ab") && event.sequence == n)
            );
        }
        assert!(
            matches!(slow.try_recv().unwrap(), Some(Delivery::Loss(loss)) if loss.reason == LossReason::Overflow)
        );
        assert!(slow.try_recv().unwrap().is_none());
        drop(slow);
        let mut replacement = stream.subscribe().unwrap();
        assert!(
            matches!(replacement.try_recv().unwrap(), Some(Delivery::Snapshot(snapshot)) if snapshot.value == json!(2) && snapshot.revision == 3)
        );
    }
}
