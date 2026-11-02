//! Typed, bounded in-process delivery. Payloads never enter the JSON codec.
mod receive;
#[cfg(test)]
mod tests;

use crate::{Delivery, Event, Loss, LossReason, PublishError, Snapshot, StreamError};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Condvar, Mutex, MutexGuard},
};

/// Conservative payload bytes retained by one queued value. Shared allocations
/// must be counted in full even when cloning the value only clones an `Arc`.
pub trait RetainedBytes {
    fn retained_bytes(&self) -> usize;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeLimits {
    pub max_subscribers: usize,
    /// Retained payload bytes in the current or a queued baseline.
    pub max_snapshot_bytes: usize,
    /// Retained payload bytes in a single event.
    pub max_frame_bytes: usize,
    /// Queued events per reader; its separately bounded baseline is excluded.
    pub max_pending_frames: usize,
    /// Retained event payload bytes per reader; envelope metadata is excluded.
    pub max_pending_bytes: usize,
}

#[derive(Clone)]
pub struct NativeStream<S, E> {
    shared: Arc<Shared<S, E>>,
}

pub struct NativeSubscription<S, E> {
    shared: Arc<Shared<S, E>>,
    id: u64,
}

#[derive(Clone)]
pub struct NativeCancellation<S, E> {
    shared: Arc<Shared<S, E>>,
    id: u64,
}

struct Shared<S, E> {
    limits: NativeLimits,
    state: Mutex<State<S, E>>,
    wake: Condvar,
}

struct State<S, E> {
    snapshot: Arc<Snapshot<S>>,
    sequence: u64,
    subscribers: BTreeMap<u64, Subscriber<S, E>>,
    next_subscriber: u64,
    closed: bool,
}

struct Subscriber<S, E> {
    snapshot: Option<Arc<Snapshot<S>>>,
    events: VecDeque<(Arc<Event<E>>, usize)>,
    pending_bytes: usize,
    loss: Option<Loss>,
    permanent: bool,
    awaiting_baseline: bool,
}

impl<S: Clone + RetainedBytes, E: Clone + RetainedBytes> NativeStream<S, E> {
    pub fn new(
        generation: u64,
        snapshot: S,
        captured_us: u64,
        clock_id: impl Into<String>,
        limits: NativeLimits,
    ) -> Result<Self, PublishError> {
        if limits.max_subscribers == 0
            || limits.max_snapshot_bytes == 0
            || limits.max_frame_bytes == 0
            || limits.max_pending_frames == 0
            || limits.max_pending_bytes == 0
        {
            return Err(PublishError::Serialization(
                "Native stream limits must be positive.".into(),
            ));
        }
        if generation == 0 {
            return Err(PublishError::InvalidGeneration);
        }
        let clock_id = clock_id.into();
        crate::journal::validate_clock(&clock_id)?;
        if snapshot.retained_bytes() > limits.max_snapshot_bytes {
            return Err(PublishError::SnapshotTooLarge);
        }
        Ok(Self {
            shared: Arc::new(Shared {
                limits,
                state: Mutex::new(State {
                    snapshot: Arc::new(Snapshot {
                        generation,
                        revision: 1,
                        captured_us,
                        clock_id,
                        value: snapshot,
                    }),
                    sequence: 0,
                    subscribers: BTreeMap::new(),
                    next_subscriber: 0,
                    closed: false,
                }),
                wake: Condvar::new(),
            }),
        })
    }

    /// Capturing the baseline and registering the reader are one atomic action.
    pub fn subscribe(&self) -> Result<NativeSubscription<S, E>, StreamError> {
        let mut state = self.shared.lock();
        if state.closed {
            return Err(StreamError::Closed);
        }
        if state.subscribers.len() >= self.shared.limits.max_subscribers {
            return Err(StreamError::SubscriberLimit);
        }
        let id = state
            .next_subscriber
            .checked_add(1)
            .ok_or_else(|| StreamError::Serialization("Subscriber counter exhausted.".into()))?;
        state.next_subscriber = id;
        let snapshot = state.snapshot.clone();
        state.subscribers.insert(
            id,
            Subscriber {
                snapshot: Some(snapshot),
                events: VecDeque::new(),
                pending_bytes: 0,
                loss: None,
                permanent: false,
                awaiting_baseline: true,
            },
        );
        Ok(NativeSubscription {
            shared: self.shared.clone(),
            id,
        })
    }

    /// Install the new baseline and append its event atomically. Retained byte
    /// reporting runs before acquiring the journal lock; payload cloning happens
    /// only after a reader releases it.
    pub fn publish(
        &self,
        generation: u64,
        event: E,
        new_snapshot: S,
        captured_us: u64,
        clock_id: impl Into<String>,
    ) -> Result<u64, PublishError> {
        let clock_id = clock_id.into();
        crate::journal::validate_clock(&clock_id)?;
        let bytes = event.retained_bytes();
        if bytes > self.shared.limits.max_frame_bytes {
            return Err(PublishError::FrameTooLarge);
        }
        if new_snapshot.retained_bytes() > self.shared.limits.max_snapshot_bytes {
            return Err(PublishError::SnapshotTooLarge);
        }
        let mut state = self.shared.lock();
        if state.closed {
            return Err(PublishError::Closed);
        }
        if generation != state.snapshot.generation {
            return Err(PublishError::StaleGeneration);
        }
        let revision = state
            .snapshot
            .revision
            .checked_add(1)
            .ok_or(PublishError::CounterExhausted)?;
        let sequence = state
            .sequence
            .checked_add(1)
            .ok_or(PublishError::CounterExhausted)?;
        let event = Arc::new(Event {
            generation,
            sequence,
            revision,
            captured_us,
            clock_id: clock_id.clone(),
            value: event,
        });
        state.snapshot = Arc::new(Snapshot {
            generation,
            revision,
            captured_us,
            clock_id,
            value: new_snapshot,
        });
        state.sequence = sequence;
        for subscriber in state.subscribers.values_mut() {
            if subscriber.permanent {
                continue;
            }
            let pending = subscriber.pending_bytes.checked_add(bytes);
            if subscriber.events.len() >= self.shared.limits.max_pending_frames
                || pending.is_none_or(|count| count > self.shared.limits.max_pending_bytes)
            {
                subscriber.fail(Loss {
                    generation,
                    revision,
                    reason: LossReason::Overflow,
                });
            } else {
                subscriber.pending_bytes = pending.unwrap();
                subscriber.events.push_back((event.clone(), bytes));
            }
        }
        drop(state);
        self.shared.wake.notify_all();
        Ok(revision)
    }

    /// A healthy reader receives loss then the replacement baseline. Readers
    /// permanently lost to overflow or closure must create a new subscription.
    pub fn replace_generation(
        &self,
        generation: u64,
        snapshot: S,
        captured_us: u64,
        clock_id: impl Into<String>,
    ) -> Result<u64, PublishError> {
        let clock_id = clock_id.into();
        crate::journal::validate_clock(&clock_id)?;
        if snapshot.retained_bytes() > self.shared.limits.max_snapshot_bytes {
            return Err(PublishError::SnapshotTooLarge);
        }
        let mut state = self.shared.lock();
        if state.closed {
            return Err(PublishError::Closed);
        }
        if generation <= state.snapshot.generation {
            return Err(PublishError::StaleGeneration);
        }
        let revision = state
            .snapshot
            .revision
            .checked_add(1)
            .ok_or(PublishError::CounterExhausted)?;
        state.snapshot = Arc::new(Snapshot {
            generation,
            revision,
            captured_us,
            clock_id,
            value: snapshot,
        });
        state.sequence = 0;
        let baseline = state.snapshot.clone();
        for subscriber in state.subscribers.values_mut() {
            if subscriber.permanent {
                continue;
            }
            subscriber.events.clear();
            subscriber.pending_bytes = 0;
            subscriber.snapshot = Some(baseline.clone());
            subscriber.loss = Some(Loss {
                generation,
                revision,
                reason: LossReason::GenerationReplaced,
            });
            subscriber.awaiting_baseline = true;
        }
        drop(state);
        self.shared.wake.notify_all();
        Ok(revision)
    }

    pub fn generation(&self) -> u64 {
        self.shared.lock().snapshot.generation
    }
    pub fn revision(&self) -> u64 {
        self.shared.lock().snapshot.revision
    }

    /// Discard pending deliveries and immediately notify all readers of closure.
    pub fn close(&self) {
        let mut state = self.shared.lock();
        if state.closed {
            return;
        }
        state.closed = true;
        let generation = state.snapshot.generation;
        let revision = state.snapshot.revision;
        for subscriber in state.subscribers.values_mut() {
            subscriber.fail(Loss {
                generation,
                revision,
                reason: LossReason::Closed,
            });
        }
        drop(state);
        self.shared.wake.notify_all();
    }
}

impl<S, E> Shared<S, E> {
    fn lock(&self) -> MutexGuard<'_, State<S, E>> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

impl<S, E> Subscriber<S, E> {
    fn fail(&mut self, loss: Loss) {
        self.snapshot = None;
        self.events.clear();
        self.pending_bytes = 0;
        self.loss = Some(loss);
        self.permanent = true;
        self.awaiting_baseline = false;
    }

    fn has_delivery(&self) -> bool {
        self.loss.is_some() || self.snapshot.is_some() || !self.events.is_empty()
    }
}
