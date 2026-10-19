//! Opt-in, bounded QPC sidecars for development acceptance. These records never
//! authorize input, alter a dispatch result, or replace the ownership journal.
//!
//! IDs are process-local; qualify them by the header's `(processPid,
//! processInstance)` pair and audit run nonce. Counters and QPC values serialize
//! as decimal strings. Zero denotes an absent optional numeric identity.
use serde::Serialize;
use std::{process::Command, time::Duration};

pub type Id = u64;
pub const ENV_DIRECTORY: &str = "OSG_DESKTOP_AUDIT_DIR";
pub const ENV_RUN: &str = "OSG_DESKTOP_AUDIT_RUN";
pub const ENV_PARENT_PID: &str = "OSG_DESKTOP_AUDIT_PARENT_PID";
pub const ENV_PARENT_INSTANCE: &str = "OSG_DESKTOP_AUDIT_PARENT_INSTANCE";
pub const ENV_CHANNEL: &str = "OSG_DESKTOP_AUDIT_CHANNEL";
pub const MAX_FIELDS: usize = 16;
pub const QUEUE_LIMIT: usize = 4096;
pub const RECORD_LIMIT: u64 = 65_536;
pub const BYTE_LIMIT: u64 = 32 * 1024 * 1024;
pub const ALIAS_LIMIT: usize = 4096;
pub const ALIAS_KEY_LIMIT: usize = 1024;

#[derive(Clone, Copy, Debug)]
pub enum Role {
    Parent,
    Worker,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub qpc: u64,
}
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum AliasKind {
    Run,
    LogicalSource,
    NativeSource,
    Port,
    Action,
    Provider,
    Subscription,
    Instance,
}

/// Positional numeric payloads; source key fields include private queue epochs
/// whenever a native queue sequence can reset within a provider generation.
#[derive(Clone, Copy, Debug)]
pub enum Kind {
    /// capture, provider, physical source, wire session alias, wire generation,
    /// publication alias, wire sequence, change count
    SourceCapture,
    /// capture, accepted (0/1), normalized rejection code
    SourceResult,
    /// source instance, run session alias, run generation, logical source,
    /// provider, subscription, physical source, resource provider session alias
    SourceBound,
    /// capture, change index, native frame, provider, subscription, queue domain,
    /// physical source, native generation, native sequence, physical port,
    /// logical capturedUs (identity only), queue depth
    NativeEnqueue,
    /// native frame, provider, subscription, queue domain, physical source,
    /// native generation, native sequence, remaining queue depth
    NativeDequeue,
    /// One per port: instance, provider, subscription, queue domain, physical
    /// source, native generation, native sequence, typed frame, logical source,
    /// typed generation, typed sequence, logical port, physical port,
    /// logical capturedUs (identity only), synthetic kind (0 real input)
    TypedAlias,
    /// native frame, provider, subscription, queue domain, physical source,
    /// native generation, native sequence, kind, logical capturedUs (identity
    /// only). Kinds: 1 baseline, 2 renewal, 3 loss, 4 non-WSS; no input capture.
    SyntheticFrame,
    /// effect, desktop/shadow instance, run session alias, run generation,
    /// effect sequence, root count
    Effect,
    /// effect, logical source, logical port, dependency generation, dependency
    /// epoch, immutable observation sequence, logical observation atUs
    EffectRoot,
    /// effect, desktop/shadow instance, run session alias, run generation,
    /// delivery operation, action (immediate KeyHold only)
    DeliveryBound,
    /// desktop/shadow instance, run session alias, run generation, delivery
    /// operation, normalized reason (1 unarmed cleanup, 2 cancelled action,
    /// 3 Stop discard)
    DeliveryDropped,
    /// channel, exact child process PID; child header supplies parent identity
    ChannelOpened,
    /// channel, exact child process PID, exit code cast through u32 (u64::MAX
    /// absent), successful graceful exit (0/1), reason: 1 observed exit,
    /// 2 killed at the existing Drop deadline, 3 wait error. A wait error is a
    /// failed observation even if a later terminal event records exit zero.
    ChannelExited,
    /// channel, request sequence, operation (0 absent), command code,
    /// run session alias (0 absent), run generation (0 absent)
    RequestIssued,
    /// channel, request sequence, queued depth
    RequestQueued,
    /// channel, request sequence, remaining queue depth
    RequestDequeued,
    /// channel, request sequence, operation, category, attempted, confirmed
    SubmitReturned,
    /// channel, request sequence, operation, flags, attempted, confirmed
    WorkerOutcome,
    /// channel, request sequence, operation, flags, attempted, confirmed, error
    ParentOutcome,
    /// channel, current request sequence, QPC at begin/serialized/directory
    /// ready/temp created/written/synced/replaced. Successes only; journal
    /// failures invalidate the collector independently. No user data is logged.
    JournalPersist,
    /// queue kind, normalized reason, depth
    QueueFault,
    /// source, provider, subscription, normalized reason
    NativeGap,
    /// caller-owned marker, phase code
    Marker,
    /// normalized Fault discriminant
    Fault,
}
#[derive(Clone, Copy, Debug)]
#[repr(u64)]
pub enum Fault {
    Clock = 1,
    AliasCapacity,
    AliasKeyTooLarge,
    InvalidFields,
    RecordCap,
    QueueFull,
    Serialization,
    Io,
    FlushTimeout,
    Provenance,
    Context,
    AfterFinish,
    CounterExhausted,
}
#[derive(Clone, Copy, Debug)]
pub enum Checkpoint {
    Stopped,
    Manual,
    RunComplete,
}
#[derive(Clone, Copy, Debug)]
pub enum ExitReason {
    Normal,
    Stop,
    Error,
}

fn decimal<S: serde::Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&value.to_string())
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counters {
    #[serde(serialize_with = "decimal")]
    pub attempted: u64,
    #[serde(serialize_with = "decimal")]
    pub enqueued: u64,
    #[serde(serialize_with = "decimal")]
    pub written: u64,
    #[serde(serialize_with = "decimal")]
    pub queue_drops: u64,
    #[serde(serialize_with = "decimal")]
    pub cap_drops: u64,
    #[serde(serialize_with = "decimal")]
    pub alias_failures: u64,
    #[serde(serialize_with = "decimal")]
    pub qpc_failures: u64,
    #[serde(serialize_with = "decimal")]
    pub invalid_fields: u64,
    #[serde(serialize_with = "decimal")]
    pub serialization_errors: u64,
    #[serde(serialize_with = "decimal")]
    pub io_errors: u64,
    #[serde(serialize_with = "decimal")]
    pub flush_timeouts: u64,
    #[serde(serialize_with = "decimal")]
    pub evidence_faults: u64,
    #[serde(serialize_with = "decimal")]
    pub after_finish: u64,
    /// Upper bound on outstanding collector queue reservations, capped at capacity.
    #[serde(serialize_with = "decimal")]
    pub queue_high_water: u64,
    #[serde(serialize_with = "decimal")]
    pub last_enqueued_sequence: u64,
    #[serde(serialize_with = "decimal")]
    pub last_written_sequence: u64,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlushReport {
    pub configured: bool,
    pub enabled: bool,
    pub complete: bool,
    pub valid: bool,
    pub finished: bool,
    #[serde(serialize_with = "decimal")]
    pub through_sequence: u64,
    pub counters: Counters,
}

#[cfg(any(test, all(windows, debug_assertions)))]
#[path = "audit/collector.rs"]
mod collector;

/// Call before source admission or worker service startup. Initialization failure
/// is observable through `checkpoint`; it never changes an input operation.
pub fn initialize(role: Role) {
    #[cfg(all(windows, debug_assertions))]
    collector::initialize(role);
    #[cfg(not(all(windows, debug_assertions)))]
    let _ = role;
}
pub fn enabled() -> bool {
    #[cfg(all(windows, debug_assertions))]
    {
        collector::enabled()
    }
    #[cfg(not(all(windows, debug_assertions)))]
    {
        false
    }
}
pub fn stamp() -> Option<Stamp> {
    #[cfg(all(windows, debug_assertions))]
    {
        collector::stamp()
    }
    #[cfg(not(all(windows, debug_assertions)))]
    {
        None
    }
}
pub fn next_id() -> Option<Id> {
    #[cfg(all(windows, debug_assertions))]
    {
        collector::next_id()
    }
    #[cfg(not(all(windows, debug_assertions)))]
    {
        None
    }
}
pub fn alias(kind: AliasKind, key: &[u8]) -> Option<Id> {
    #[cfg(all(windows, debug_assertions))]
    {
        collector::alias(kind, key)
    }
    #[cfg(not(all(windows, debug_assertions)))]
    {
        let _ = (kind, key);
        None
    }
}
pub fn record(kind: Kind, fields: &[u64]) {
    if let Some(at) = stamp() {
        record_at(at, kind, fields);
    }
}
pub fn record_at(at: Stamp, kind: Kind, fields: &[u64]) {
    #[cfg(all(windows, debug_assertions))]
    collector::record_at(at, kind, fields);
    #[cfg(not(all(windows, debug_assertions)))]
    let _ = (at, kind, fields);
}
pub fn fault(reason: Fault) {
    #[cfg(all(windows, debug_assertions))]
    collector::fault(reason);
    #[cfg(not(all(windows, debug_assertions)))]
    let _ = reason;
}
pub fn process_instance() -> Option<Id> {
    #[cfg(all(windows, debug_assertions))]
    {
        collector::process_instance()
    }
    #[cfg(not(all(windows, debug_assertions)))]
    {
        None
    }
}
pub fn parent_instance() -> Option<Id> {
    #[cfg(all(windows, debug_assertions))]
    {
        collector::parent_instance()
    }
    #[cfg(not(all(windows, debug_assertions)))]
    {
        None
    }
}
pub fn parent_pid() -> Option<u64> {
    #[cfg(all(windows, debug_assertions))]
    {
        collector::parent_pid()
    }
    #[cfg(not(all(windows, debug_assertions)))]
    {
        None
    }
}
pub fn channel() -> Option<Id> {
    #[cfg(all(windows, debug_assertions))]
    {
        collector::channel()
    }
    #[cfg(not(all(windows, debug_assertions)))]
    {
        None
    }
}
/// Set only child-scoped diagnostic environment; the private wire is untouched.
pub fn configure_child(command: &mut Command, channel: Id) {
    #[cfg(all(windows, debug_assertions))]
    collector::configure_child(command, channel);
    #[cfg(not(all(windows, debug_assertions)))]
    let _ = (command, channel);
}
pub fn checkpoint(reason: Checkpoint, timeout: Duration) -> FlushReport {
    #[cfg(all(windows, debug_assertions))]
    {
        collector::checkpoint(reason, timeout)
    }
    #[cfg(not(all(windows, debug_assertions)))]
    {
        let _ = (reason, timeout);
        FlushReport::default()
    }
}
pub fn finish(reason: ExitReason, timeout: Duration) -> FlushReport {
    #[cfg(all(windows, debug_assertions))]
    {
        collector::finish(reason, timeout)
    }
    #[cfg(not(all(windows, debug_assertions)))]
    {
        let _ = (reason, timeout);
        FlushReport::default()
    }
}
