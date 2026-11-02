//! Bounded, in-memory ordered observation streams.
//!
//! This crate provides a transport-neutral journal only. It does not acquire
//! sources, schedule native work, define domain wire envelopes, or grant command
//! authority. A stream fences the caller-supplied generation; the owning service
//! remains responsible for adapter, device/process, session, publication and
//! source identity, and for replacing the generation at the correct boundary.

mod codec;
mod fault;
mod journal;
mod lifecycle;
mod native;
mod registry;
mod types;
mod wire;

pub use registry::{
    Broker, Publisher, Reader, ReaderCancellation, RegistryError, TopicSpec, KNOWN_AUDIENCES,
    MAX_CLIENT_READERS, MAX_READERS, MAX_TOPICS,
};
pub use wire::{WireFrame, MAX_CHANGE_BYTES, MAX_SNAPSHOT_BYTES};

pub use journal::{Cancellation, Stream, Subscription};
pub use native::{NativeCancellation, NativeLimits, NativeStream, NativeSubscription, RetainedBytes};
pub use types::{
    Delivery, Event, Limits, Loss, LossReason, PublishError, Snapshot, StreamError, DEFAULT_LIMITS,
};

#[cfg(test)]
mod tests;
