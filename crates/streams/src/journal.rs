mod publish;
mod latest;

use crate::{
    codec::{encode_limited, map_encode_error},
    types::{
        Delivery, Event, Limits, Loss, LossReason, PublishError, Snapshot, StreamError,
        DEFAULT_LIMITS,
    },
};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    marker::PhantomData,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex, MutexGuard,
    },
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Stream<S, E> {
    pub(crate) shared: Arc<Shared>,
    pub(crate) marker: PhantomData<fn() -> (S, E)>,
}

pub(crate) struct Shared {
    pub(crate) limits: Limits,
    pub(crate) state: Mutex<State>,
    pub(crate) wake: Condvar,
    has_readers: AtomicBool,
}

pub(crate) struct State {
    pub(crate) generation: u64,
    pub(crate) revision: u64,
    pub(crate) sequence: u64,
    pub(crate) snapshot: EncodedSnapshot,
    pub(crate) subscribers: BTreeMap<u64, SubscriberState>,
    pub(crate) next_subscriber: u64,
    pub(crate) closed: bool,
}

pub(crate) struct EncodedSnapshot {
    pub(crate) captured_us: u64,
    pub(crate) clock_id: String,
    pub(crate) bytes: Arc<[u8]>,
}

pub(crate) struct SubscriberState {
    pub(crate) snapshot: Option<QueuedSnapshot>,
    pub(crate) events: VecDeque<QueuedEvent>,
    pub(crate) pending_bytes: usize,
    pub(crate) loss: Option<Loss>,
    pub(crate) blocked: bool,
    pub(crate) loss_before_snapshot: bool,
}

#[derive(Clone)]
pub(crate) struct QueuedSnapshot {
    pub(crate) generation: u64,
    pub(crate) revision: u64,
    pub(crate) captured_us: u64,
    pub(crate) clock_id: String,
    pub(crate) bytes: Arc<[u8]>,
}

pub(crate) struct QueuedEvent {
    pub(crate) generation: u64,
    pub(crate) sequence: u64,
    pub(crate) revision: u64,
    pub(crate) captured_us: u64,
    pub(crate) clock_id: String,
    pub(crate) bytes: Arc<[u8]>,
}

pub struct Subscription<S, E> {
    pub(crate) shared: Arc<Shared>,
    pub(crate) id: u64,
    pub(crate) marker: PhantomData<fn() -> (S, E)>,
}

#[derive(Clone)]
pub struct Cancellation {
    pub(crate) shared: Arc<Shared>,
    pub(crate) id: u64,
}

impl<S, E> Stream<S, E>
where
    S: Serialize + DeserializeOwned,
    E: Serialize + DeserializeOwned,
{
    pub fn new(
        generation: u64,
        snapshot: S,
        captured_us: u64,
        clock_id: impl Into<String>,
    ) -> Result<Self, PublishError> {
        Self::with_limits(generation, snapshot, captured_us, clock_id, DEFAULT_LIMITS)
    }

    pub fn with_limits(
        generation: u64,
        snapshot: S,
        captured_us: u64,
        clock_id: impl Into<String>,
        limits: Limits,
    ) -> Result<Self, PublishError> {
        let clock_id = clock_id.into();
        validate_limits(limits)?;
        validate_clock(&clock_id)?;
        if generation == 0 {
            return Err(PublishError::InvalidGeneration);
        }
        let bytes = encode_limited(&snapshot, limits.max_snapshot_bytes)
            .map_err(|error| map_encode_error(error, PublishError::SnapshotTooLarge))?;
        Ok(Self {
            shared: Arc::new(Shared {
                limits,
                state: Mutex::new(State {
                    generation,
                    revision: 1,
                    sequence: 0,
                    snapshot: EncodedSnapshot {
                        captured_us,
                        clock_id,
                        bytes: bytes.into(),
                    },
                    subscribers: BTreeMap::new(),
                    next_subscriber: 0,
                    closed: false,
                }),
                wake: Condvar::new(),
                has_readers: AtomicBool::new(false),
            }),
            marker: PhantomData,
        })
    }

    /// Atomically capture the current baseline and register for later events.
    pub fn subscribe(&self) -> Result<Subscription<S, E>, StreamError> {
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
        let snapshot = queued_snapshot(&state);
        state.subscribers.insert(
            id,
            SubscriberState {
                snapshot: Some(snapshot),
                events: VecDeque::new(),
                pending_bytes: 0,
                loss: None,
                blocked: false,
                loss_before_snapshot: false,
            },
        );
        self.shared.has_readers.store(true, Ordering::Release);
        Ok(Subscription {
            shared: self.shared.clone(),
            id,
            marker: PhantomData,
        })
    }
}

impl<S, E> Subscription<S, E>
where
    S: DeserializeOwned,
    E: DeserializeOwned,
{
    /// Nonblocking read. Deserialization occurs after releasing the owner lock.
    pub fn try_recv(&mut self) -> Result<Option<Delivery<S, E>>, StreamError> {
        let queued = {
            let mut state = self.shared.lock();
            let Some(subscriber) = state.subscribers.get_mut(&self.id) else {
                return Err(StreamError::Closed);
            };
            if subscriber.loss.is_some()
                && (subscriber.snapshot.is_none() || subscriber.loss_before_snapshot)
            {
                let loss = subscriber.loss.take().unwrap();
                subscriber.loss_before_snapshot = false;
                Some(QueuedDelivery::Loss(loss))
            } else if let Some(snapshot) = subscriber.snapshot.take() {
                if subscriber.loss.is_none() {
                    subscriber.blocked = false;
                }
                Some(QueuedDelivery::Snapshot(snapshot))
            } else if let Some(loss) = subscriber.loss.take() {
                subscriber.loss_before_snapshot = false;
                Some(QueuedDelivery::Loss(loss))
            } else if let Some(event) = subscriber.events.pop_front() {
                subscriber.pending_bytes =
                    subscriber.pending_bytes.saturating_sub(event.bytes.len());
                Some(QueuedDelivery::Event(event))
            } else {
                None
            }
        };
        match queued {
            None => Ok(None),
            Some(QueuedDelivery::Loss(loss)) => Ok(Some(Delivery::Loss(loss))),
            Some(QueuedDelivery::Snapshot(snapshot)) => {
                let value = match serde_json::from_slice(&snapshot.bytes) {
                    Ok(value) => value,
                    Err(error) => {
                        self.fail_payload(snapshot.generation, snapshot.revision);
                        return Err(StreamError::Serialization(error.to_string()));
                    }
                };
                Ok(Some(Delivery::Snapshot(Snapshot {
                    generation: snapshot.generation,
                    revision: snapshot.revision,
                    captured_us: snapshot.captured_us,
                    clock_id: snapshot.clock_id,
                    value,
                })))
            }
            Some(QueuedDelivery::Event(event)) => {
                let value = match serde_json::from_slice(&event.bytes) {
                    Ok(value) => value,
                    Err(error) => {
                        self.fail_payload(event.generation, event.revision);
                        return Err(StreamError::Serialization(error.to_string()));
                    }
                };
                Ok(Some(Delivery::Event(Event {
                    generation: event.generation,
                    sequence: event.sequence,
                    revision: event.revision,
                    captured_us: event.captured_us,
                    clock_id: event.clock_id,
                    value,
                })))
            }
        }
    }

    /// Wait until data arrives, the stream closes, or the subscription is cancelled.
    pub fn recv(&mut self) -> Result<Delivery<S, E>, StreamError> {
        loop {
            if let Some(delivery) = self.try_recv()? {
                return Ok(delivery);
            }
            let state = self.shared.lock();
            if !state.subscribers.contains_key(&self.id) {
                return Err(StreamError::Closed);
            }
            if let Some(subscriber) = state.subscribers.get(&self.id) {
                if has_delivery(subscriber) {
                    drop(state);
                    continue;
                }
            }
            if state.closed {
                return Err(StreamError::Closed);
            }
            drop(
                self.shared
                    .wake
                    .wait(state)
                    .unwrap_or_else(|poison| poison.into_inner()),
            );
        }
    }

    /// Wait for a delivery without polling. `Ok(None)` means the timeout elapsed.
    pub fn recv_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<Delivery<S, E>>, StreamError> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(StreamError::InvalidTimeout)?;
        loop {
            if let Some(delivery) = self.try_recv()? {
                return Ok(Some(delivery));
            }
            let state = self.shared.lock();
            if !state.subscribers.contains_key(&self.id) {
                return Err(StreamError::Closed);
            }
            if let Some(subscriber) = state.subscribers.get(&self.id) {
                if has_delivery(subscriber) {
                    drop(state);
                    continue;
                }
            }
            if state.closed {
                return Err(StreamError::Closed);
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Ok(None);
            };
            let (state, timeout) = self
                .shared
                .wake
                .wait_timeout(state, remaining)
                .unwrap_or_else(|poison| poison.into_inner());
            drop(state);
            if timeout.timed_out() && Instant::now() >= deadline {
                return Ok(None);
            }
        }
    }

    pub fn cancellation_handle(&self) -> Cancellation {
        Cancellation {
            shared: self.shared.clone(),
            id: self.id,
        }
    }
}

impl<S, E> Drop for Subscription<S, E> {
    fn drop(&mut self) {
        self.shared.remove_reader(self.id);
    }
}

fn has_delivery(subscriber: &SubscriberState) -> bool {
    subscriber.loss.is_some() || subscriber.snapshot.is_some() || !subscriber.events.is_empty()
}

enum QueuedDelivery {
    Snapshot(QueuedSnapshot),
    Event(QueuedEvent),
    Loss(Loss),
}

pub(crate) fn queued_snapshot(state: &State) -> QueuedSnapshot {
    QueuedSnapshot {
        generation: state.generation,
        revision: state.revision,
        captured_us: state.snapshot.captured_us,
        clock_id: state.snapshot.clock_id.clone(),
        bytes: state.snapshot.bytes.clone(),
    }
}

pub(crate) fn overflow(subscriber: &mut SubscriberState, generation: u64, revision: u64) {
    overflow_with_reason(
        subscriber,
        Loss {
            generation,
            revision,
            reason: LossReason::Overflow,
        },
    );
}

pub(crate) fn overflow_with_reason(subscriber: &mut SubscriberState, loss: Loss) {
    let preserve_initial_snapshot = subscriber.snapshot.is_some() && subscriber.loss.is_none();
    if !preserve_initial_snapshot {
        subscriber.snapshot = None;
    }
    subscriber.loss_before_snapshot = !preserve_initial_snapshot;
    subscriber.events.clear();
    subscriber.pending_bytes = 0;
    subscriber.loss = Some(loss);
    subscriber.blocked = true;
}

fn validate_limits(limits: Limits) -> Result<(), PublishError> {
    if limits.max_subscribers == 0
        || limits.max_snapshot_bytes == 0
        || limits.max_frame_bytes == 0
        || limits.max_batch_bytes == 0
        || limits.max_pending_frames == 0
        || limits.max_pending_bytes == 0
        || limits.max_batch_frames == 0
    {
        return Err(PublishError::Serialization(
            "Stream limits must be positive.".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_clock(clock: &str) -> Result<(), PublishError> {
    if clock.is_empty() || clock.len() > 128 || !clock.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(PublishError::InvalidClockId);
    }
    Ok(())
}

impl Shared {
    pub(crate) fn has_readers(&self) -> bool {
        self.has_readers.load(Ordering::Acquire)
    }

    pub(crate) fn remove_reader(&self, id: u64) {
        let mut state = self.lock();
        state.subscribers.remove(&id);
        self.has_readers
            .store(!state.subscribers.is_empty(), Ordering::Release);
        drop(state);
        self.wake.notify_all();
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}
