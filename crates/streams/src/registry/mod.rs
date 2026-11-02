mod publisher;
mod reader;

use crate::{
    wire::{Identity, ENVELOPE_RESERVE},
    Cancellation, PublishError, Stream, StreamError, DEFAULT_LIMITS,
};
pub use publisher::Publisher;
pub use reader::{Reader, ReaderCancellation};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard, Weak},
};

pub const KNOWN_AUDIENCES: &[&str] = &["ui", "browser", "panel", "mcp"];
pub const MAX_TOPICS: usize = 128;
pub const MAX_READERS: usize = 64;
pub const MAX_CLIENT_READERS: usize = 16;

#[derive(Clone, Debug)]
pub struct TopicSpec {
    pub topic: String,
    pub source: String,
    pub audiences: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryError {
    InvalidIdentity,
    InvalidAudience,
    DuplicateTopic,
    TopicLimit,
    UnknownTopic,
    Unauthorized,
    SubscriberLimit,
    ClientLimit,
    CounterExhausted,
    Closed,
    Publish(PublishError),
    Stream(StreamError),
}

impl From<PublishError> for RegistryError {
    fn from(error: PublishError) -> Self {
        Self::Publish(error)
    }
}
impl From<StreamError> for RegistryError {
    fn from(error: StreamError) -> Self {
        Self::Stream(error)
    }
}
impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for RegistryError {}

/// One process-owned registry, cloned by transports and topic owners.
/// All topics and clients share its admission budget.
#[derive(Clone)]
pub struct Broker {
    shared: Arc<Shared>,
}
struct Shared {
    session: String,
    state: Mutex<State>,
}
struct State {
    topics: BTreeMap<String, Arc<Topic>>,
    readers: BTreeMap<u64, (String, Weak<reader::Lease>)>,
    next_reader: u64,
    closed: bool,
}
struct Topic {
    identity: Identity,
    audiences: Vec<String>,
    stream: Stream<Value, Value>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

impl Broker {
    pub fn new(session: String) -> Result<Self, RegistryError> {
        validate_identity(&session)?;
        Ok(Self {
            shared: Arc::new(Shared {
                session,
                state: Mutex::new(State {
                    topics: BTreeMap::new(),
                    readers: BTreeMap::new(),
                    next_reader: 0,
                    closed: false,
                }),
            }),
        })
    }

    /// Service registration is the only path that creates an authoritative topic.
    pub fn register(
        &self,
        spec: TopicSpec,
        initial: Value,
        captured_us: u64,
        clock_id: String,
    ) -> Result<Publisher, RegistryError> {
        validate_identity(&spec.topic)?;
        validate_identity(&spec.source)?;
        if spec.audiences.is_empty()
            || spec.audiences.len() > KNOWN_AUDIENCES.len()
            || spec
                .audiences
                .iter()
                .any(|role| !KNOWN_AUDIENCES.contains(&role.as_str()))
        {
            return Err(RegistryError::InvalidAudience);
        }
        let mut audiences = spec.audiences;
        audiences.sort();
        audiences.dedup();
        let identity = Identity {
            topic: spec.topic.clone(),
            source: spec.source,
            session: self.shared.session.clone(),
        };
        identity.validate(&initial, captured_us, &clock_id, false)?;
        let mut limits = DEFAULT_LIMITS;
        limits.max_snapshot_bytes -= ENVELOPE_RESERVE;
        limits.max_frame_bytes -= ENVELOPE_RESERVE;
        // Count worst-case envelope overhead inside the ordered queue byte cap.
        limits.max_pending_frames = 256;
        limits.max_pending_bytes /= 2;
        let stream = Stream::with_limits(1, initial, captured_us, clock_id, limits)?;
        let mut state = self.shared.lock();
        if state.closed {
            return Err(RegistryError::Closed);
        }
        if state.topics.contains_key(&spec.topic) {
            return Err(RegistryError::DuplicateTopic);
        }
        if state.topics.len() >= MAX_TOPICS {
            return Err(RegistryError::TopicLimit);
        }
        let topic = Arc::new(Topic {
            identity,
            audiences,
            stream,
        });
        state.topics.insert(spec.topic, topic.clone());
        Ok(Publisher { topic })
    }

    pub fn subscribe(
        &self,
        client: &str,
        audience: &str,
        topic: &str,
    ) -> Result<Reader, RegistryError> {
        validate_identity(client)?;
        validate_identity(topic)?;
        if !KNOWN_AUDIENCES.contains(&audience) {
            return Err(RegistryError::InvalidAudience);
        }
        let mut state = self.shared.lock();
        if state.closed {
            return Err(RegistryError::Closed);
        }
        let topic = state
            .topics
            .get(topic)
            .cloned()
            .ok_or(RegistryError::UnknownTopic)?;
        if !topic.audiences.iter().any(|allowed| allowed == audience) {
            return Err(RegistryError::Unauthorized);
        }
        if state.readers.len() >= MAX_READERS {
            return Err(RegistryError::SubscriberLimit);
        }
        if state
            .readers
            .values()
            .filter(|(owner, _)| owner == client)
            .count()
            >= MAX_CLIENT_READERS
        {
            return Err(RegistryError::ClientLimit);
        }
        let id = state
            .next_reader
            .checked_add(1)
            .ok_or(RegistryError::CounterExhausted)?;
        let subscription = topic.stream.subscribe()?;
        let lease = Arc::new(reader::Lease::new(
            Arc::downgrade(&self.shared),
            id,
            subscription.cancellation_handle(),
        ));
        state.next_reader = id;
        state
            .readers
            .insert(id, (client.into(), Arc::downgrade(&lease)));
        Ok(Reader::new(topic, subscription, lease))
    }

    /// Withdraw this exact registration, including its existing readers.
    /// A stale owner cannot remove a replacement registered under the same name.
    pub fn retire(&self, publisher: &Publisher) -> bool {
        let removed = {
            let mut state = self.shared.lock();
            let topic = &publisher.topic;
            if !state.topics.get(&topic.identity.topic)
                .is_some_and(|current| Arc::ptr_eq(current, topic))
            {
                return false;
            }
            state.topics.remove(&topic.identity.topic).expect("current topic")
        };
        removed.stream.close();
        true
    }

    /// Retire only this exact current registration, atomically against subscribe.
    /// Lock order matches subscribe: broker first, then stream.
    pub fn retire_if_idle(&self, publisher: &Publisher) -> bool {
        let mut state = self.shared.lock();
        let topic = &publisher.topic;
        if !state
            .topics
            .get(&topic.identity.topic)
            .is_some_and(|current| Arc::ptr_eq(current, topic))
        {
            return false;
        }
        let mut stream = topic.stream.shared.lock();
        if !stream.subscribers.is_empty() {
            return false;
        }
        stream.closed = true;
        state.topics.remove(&topic.identity.topic);
        drop(stream);
        topic.stream.shared.wake.notify_all();
        true
    }

    pub fn close_client(&self, client: &str) {
        let leases: Vec<_> = self
            .shared
            .lock()
            .readers
            .values()
            .filter(|(owner, _)| owner == client)
            .filter_map(|(_, lease)| lease.upgrade())
            .collect();
        for lease in leases {
            lease.close();
        }
    }

    pub fn close(&self) {
        let (topics, leases) = {
            let mut state = self.shared.lock();
            state.closed = true;
            (
                state.topics.values().cloned().collect::<Vec<_>>(),
                state
                    .readers
                    .values()
                    .filter_map(|(_, lease)| lease.upgrade())
                    .collect::<Vec<_>>(),
            )
        };
        for topic in topics {
            topic.stream.close();
        }
        for lease in leases {
            lease.close();
        }
    }
}

fn validate_identity(value: &str) -> Result<(), RegistryError> {
    if value.is_empty() || value.len() > 128 || !value.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(RegistryError::InvalidIdentity);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
