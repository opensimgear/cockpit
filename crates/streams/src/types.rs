#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_subscribers: usize,
    /// Encoded JSON snapshot payload bytes; envelope metadata is excluded.
    pub max_snapshot_bytes: usize,
    /// Encoded JSON event payload bytes; envelope metadata is excluded.
    pub max_frame_bytes: usize,
    /// Encoded event payload bytes accepted in one atomic batch.
    pub max_batch_bytes: usize,
    pub max_pending_frames: usize,
    /// Encoded JSON event payload bytes queued per subscriber; metadata is excluded.
    pub max_pending_bytes: usize,
    pub max_batch_frames: usize,
}

pub const DEFAULT_LIMITS: Limits = Limits {
    max_subscribers: 64,
    max_snapshot_bytes: 1024 * 1024,
    max_frame_bytes: 64 * 1024,
    max_batch_bytes: 4 * 1024 * 1024,
    max_pending_frames: 1024,
    max_pending_bytes: 1024 * 1024,
    max_batch_frames: 4096,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot<S> {
    pub generation: u64,
    pub revision: u64,
    pub captured_us: u64,
    pub clock_id: String,
    pub value: S,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loss {
    pub generation: u64,
    pub revision: u64,
    pub reason: LossReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LossReason {
    Overflow,
    GenerationReplaced,
    BaselineReset,
    Closed,
    DecodeFailure,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event<E> {
    pub generation: u64,
    pub sequence: u64,
    pub revision: u64,
    pub captured_us: u64,
    pub clock_id: String,
    pub value: E,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delivery<S, E> {
    Snapshot(Snapshot<S>),
    Event(Event<E>),
    Loss(Loss),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublishError {
    InvalidClockId,
    SnapshotTooLarge,
    FrameTooLarge,
    BatchTooLarge,
    EmptyBatch,
    StaleGeneration,
    InvalidGeneration,
    CounterExhausted,
    Closed,
    Serialization(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StreamError {
    SubscriberLimit,
    Closed,
    InvalidTimeout,
    Serialization(String),
}
