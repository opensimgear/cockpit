#[cfg(all(windows, debug_assertions))]
use super::ExitReason;
use super::{
    AliasKind, Checkpoint, Counters, Fault, FlushReport, Id, Kind, Role, Stamp, ALIAS_KEY_LIMIT,
    ALIAS_LIMIT, BYTE_LIMIT, MAX_FIELDS, QUEUE_LIMIT, RECORD_LIMIT,
};
#[cfg(all(windows, debug_assertions))]
use super::{ENV_CHANNEL, ENV_DIRECTORY, ENV_PARENT_INSTANCE, ENV_PARENT_PID, ENV_RUN};
use serde::Serialize;
#[cfg(all(windows, debug_assertions))]
use std::process::Command;
use std::time::Duration;
use std::{
    collections::HashMap,
    io::{BufWriter, Write},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread,
    time::Instant,
};

#[path = "collector/capacity.rs"]
mod capacity;
#[path = "collector/writer.rs"]
mod writer;
use capacity::CapacityWake;

#[derive(Clone)]
struct Context {
    run: String,
    role: Role,
    pid: u64,
    instance: u64,
    parent_pid: Option<u64>,
    parent_instance: Option<u64>,
    channel: Option<u64>,
}
#[derive(Clone, Copy)]
struct Limits {
    queue: usize,
    records: u64,
    bytes: u64,
    aliases: usize,
    alias_bytes: usize,
    buffer: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            queue: QUEUE_LIMIT,
            records: RECORD_LIMIT,
            bytes: BYTE_LIMIT,
            aliases: ALIAS_LIMIT,
            alias_bytes: ALIAS_KEY_LIMIT,
            buffer: 64 * 1024,
        }
    }
}
#[derive(Clone, Copy)]
#[repr(usize)]
enum Stat {
    Attempted,
    Enqueued,
    Written,
    QueueDrops,
    CapDrops,
    AliasFailures,
    QpcFailures,
    InvalidFields,
    SerializationErrors,
    IoErrors,
    FlushTimeouts,
    EvidenceFaults,
    AfterFinish,
    QueueHighWater,
    LastEnqueued,
    LastWritten,
}
struct Stats {
    values: [AtomicU64; 16],
    queued: AtomicU64,
    finished: AtomicBool,
}
impl Default for Stats {
    fn default() -> Self {
        Self {
            values: std::array::from_fn(|_| AtomicU64::new(0)),
            queued: AtomicU64::new(0),
            finished: AtomicBool::new(false),
        }
    }
}
impl Stats {
    fn get(&self, key: Stat) -> u64 {
        self.values[key as usize].load(Ordering::Acquire)
    }
    fn bump(&self, key: Stat) -> u64 {
        let value = &self.values[key as usize];
        value
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                Some(n.saturating_add(1))
            })
            .unwrap_or(u64::MAX)
            .saturating_add(1)
    }
    fn max(&self, key: Stat, value: u64) {
        self.values[key as usize].fetch_max(value, Ordering::AcqRel);
    }
    fn fault(&self, reason: Fault) {
        let counter = match reason {
            Fault::Clock => Stat::QpcFailures,
            Fault::AliasCapacity | Fault::AliasKeyTooLarge => Stat::AliasFailures,
            Fault::InvalidFields => Stat::InvalidFields,
            Fault::RecordCap => Stat::CapDrops,
            Fault::QueueFull => Stat::QueueDrops,
            Fault::Serialization => Stat::SerializationErrors,
            Fault::Io => Stat::IoErrors,
            Fault::FlushTimeout => Stat::FlushTimeouts,
            Fault::AfterFinish => Stat::AfterFinish,
            Fault::Provenance | Fault::Context | Fault::CounterExhausted => Stat::EvidenceFaults,
        };
        self.bump(counter);
    }
    fn snapshot(&self) -> Counters {
        Counters {
            attempted: self.get(Stat::Attempted),
            enqueued: self.get(Stat::Enqueued),
            written: self.get(Stat::Written),
            queue_drops: self.get(Stat::QueueDrops),
            cap_drops: self.get(Stat::CapDrops),
            alias_failures: self.get(Stat::AliasFailures),
            qpc_failures: self.get(Stat::QpcFailures),
            invalid_fields: self.get(Stat::InvalidFields),
            serialization_errors: self.get(Stat::SerializationErrors),
            io_errors: self.get(Stat::IoErrors),
            flush_timeouts: self.get(Stat::FlushTimeouts),
            evidence_faults: self.get(Stat::EvidenceFaults),
            after_finish: self.get(Stat::AfterFinish),
            queue_high_water: self.get(Stat::QueueHighWater),
            last_enqueued_sequence: self.get(Stat::LastEnqueued),
            last_written_sequence: self.get(Stat::LastWritten),
        }
    }
    fn report(&self, through: u64, complete: bool, finished: bool) -> FlushReport {
        let counters = self.snapshot();
        let valid = complete
            && counters.queue_drops == 0
            && counters.cap_drops == 0
            && counters.alias_failures == 0
            && counters.qpc_failures == 0
            && counters.invalid_fields == 0
            && counters.serialization_errors == 0
            && counters.io_errors == 0
            && counters.flush_timeouts == 0
            && counters.evidence_faults == 0
            && counters.after_finish == 0;
        FlushReport {
            configured: true,
            enabled: true,
            complete,
            valid,
            finished,
            through_sequence: through,
            counters,
        }
    }
}
struct Gate {
    next_id: u64,
    next_sequence: u64,
    closed: bool,
    aliases: HashMap<(AliasKind, Vec<u8>), Id>,
}
struct Event {
    sequence: u64,
    at: Stamp,
    kind: Kind,
    fields: [u64; MAX_FIELDS],
    len: usize,
}
enum Message {
    Event(Event),
    Barrier {
        through: u64,
        finish: bool,
        reason: &'static str,
        deadline: Instant,
        reply: SyncSender<FlushReport>,
    },
}
struct Engine {
    context: Context,
    sender: SyncSender<Message>,
    capacity: Arc<CapacityWake>,
    stats: Arc<Stats>,
    gate: Mutex<Gate>,
    clock: Arc<dyn Fn() -> Option<u64> + Send + Sync>,
    limits: Limits,
}

fn line(value: &impl Serialize) -> Result<Vec<u8>, Fault> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| Fault::Serialization)?;
    bytes.push(b'\n');
    Ok(bytes)
}
fn write_line(
    writer: &mut impl Write,
    bytes: &[u8],
    count: &mut u64,
    limit: u64,
) -> Result<(), Fault> {
    let next = count
        .checked_add(bytes.len() as u64)
        .filter(|next| *next <= limit)
        .ok_or(Fault::RecordCap)?;
    writer.write_all(bytes).map_err(|_| Fault::Io)?;
    *count = next;
    Ok(())
}
fn decimal_option(value: Option<u64>) -> Option<String> {
    value.map(|n| n.to_string())
}
impl Engine {
    fn new(
        writer: impl Write + Send + 'static,
        context: Context,
        frequency: u64,
        clock: Arc<dyn Fn() -> Option<u64> + Send + Sync>,
        limits: Limits,
        stats: Arc<Stats>,
    ) -> Result<Self, Fault> {
        if frequency == 0 || context.instance == 0 {
            return Err(Fault::Clock);
        }
        let mut writer = BufWriter::with_capacity(limits.buffer, writer);
        let header = line(
            &serde_json::json!({"type":"header","version":"1","run":context.run,
            "role":match context.role {Role::Parent=>"parent",Role::Worker=>"worker"},
            "processPid":context.pid.to_string(),"processInstance":context.instance.to_string(),
            "parentPid":decimal_option(context.parent_pid),"parentInstance":decimal_option(context.parent_instance),
            "channel":decimal_option(context.channel),"qpcFrequency":frequency.to_string(),
            "queueLimit":limits.queue.to_string(),"recordLimit":limits.records.to_string(),"byteLimit":limits.bytes.to_string(),
            "aliasLimit":limits.aliases.to_string(),"aliasKeyLimit":limits.alias_bytes.to_string(),"fieldLimit":MAX_FIELDS.to_string()}),
        )?;
        let mut bytes_written = 0;
        write_line(&mut writer, &header, &mut bytes_written, limits.bytes)?;
        writer.flush().map_err(|_| Fault::Io)?;
        let (sender, receiver) = mpsc::sync_channel(limits.queue);
        let capacity = Arc::new(CapacityWake::default());
        let worker_capacity = capacity.clone();
        let worker_stats = stats.clone();
        thread::Builder::new()
            .name("desktop-qpc-audit".into())
            .spawn(move || writer::run(writer, receiver, worker_capacity, worker_stats, limits, bytes_written))
            .map_err(|_| Fault::Io)?;
        Ok(Self {
            context,
            sender,
            capacity,
            stats,
            gate: Mutex::new(Gate {
                next_id: 1,
                next_sequence: 1,
                closed: false,
                aliases: HashMap::new(),
            }),
            clock,
            limits,
        })
    }
    fn stamp(&self) -> Option<Stamp> {
        match (self.clock)() {
            Some(qpc) if qpc > 0 => Some(Stamp { qpc }),
            _ => {
                self.stats.fault(Fault::Clock);
                None
            }
        }
    }
    fn allocate(gate: &mut Gate, stats: &Stats) -> Option<Id> {
        let value = gate.next_id;
        if value == 0 || value == u64::MAX {
            stats.fault(Fault::CounterExhausted);
            return None;
        }
        gate.next_id += 1;
        Some(value)
    }
    fn next_id(&self) -> Option<Id> {
        let mut gate = self.gate.lock().unwrap_or_else(|p| p.into_inner());
        if gate.closed {
            self.stats.fault(Fault::AfterFinish);
            return None;
        }
        Self::allocate(&mut gate, &self.stats)
    }
    fn alias(&self, kind: AliasKind, key: &[u8]) -> Option<Id> {
        if key.len() > self.limits.alias_bytes {
            self.stats.fault(Fault::AliasKeyTooLarge);
            return None;
        }
        let mut gate = self.gate.lock().unwrap_or_else(|p| p.into_inner());
        if gate.closed {
            self.stats.fault(Fault::AfterFinish);
            return None;
        }
        if let Some(id) = gate.aliases.get(&(kind, key.to_vec())) {
            return Some(*id);
        }
        if gate.aliases.len() >= self.limits.aliases {
            self.stats.fault(Fault::AliasCapacity);
            return None;
        }
        let id = Self::allocate(&mut gate, &self.stats)?;
        gate.aliases.insert((kind, key.to_vec()), id);
        Some(id)
    }
    fn record_at(&self, at: Stamp, kind: Kind, fields: &[u64]) {
        let mut gate = self.gate.lock().unwrap_or_else(|p| p.into_inner());
        let attempted = self.stats.bump(Stat::Attempted);
        if gate.closed {
            self.stats.fault(Fault::AfterFinish);
            return;
        }
        if fields.len() > MAX_FIELDS {
            self.stats.fault(Fault::InvalidFields);
            return;
        }
        if at.qpc == 0 {
            self.stats.fault(Fault::Clock);
            return;
        }
        if attempted > self.limits.records {
            self.stats.fault(Fault::RecordCap);
            return;
        }
        let sequence = gate.next_sequence;
        if sequence == u64::MAX {
            self.stats.fault(Fault::CounterExhausted);
            return;
        }
        gate.next_sequence += 1;
        let mut values = [0; MAX_FIELDS];
        values[..fields.len()].copy_from_slice(fields);
        let reserved = self
            .stats
            .queued
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        let message = Message::Event(Event {
            sequence,
            at,
            kind,
            fields: values,
            len: fields.len(),
        });
        match self.sender.try_send(message) {
            Ok(()) => {
                self.stats.bump(Stat::Enqueued);
                self.stats.max(Stat::LastEnqueued, sequence);
                self.stats
                    .max(Stat::QueueHighWater, reserved.min(self.limits.queue as u64));
            }
            Err(error) => {
                self.stats.queued.fetch_sub(1, Ordering::AcqRel);
                self.stats.fault(match error {
                    TrySendError::Full(_) => Fault::QueueFull,
                    TrySendError::Disconnected(_) => Fault::Io,
                });
            }
        }
    }
    fn barrier(&self, finish: bool, reason: &'static str, timeout: Duration) -> FlushReport {
        let deadline = Instant::now()
            .checked_add(timeout)
            .unwrap_or_else(Instant::now);
        let (reply, receive) = mpsc::sync_channel(1);
        loop {
            // Hold the wake predicate across try_send. If the writer dequeues
            // between a Full result and the wait, its notification cannot be
            // lost because it must acquire this same mutex.
            let wake = self.capacity.lock();
            let generation = wake.generation();
            let mut gate = self.gate.lock().unwrap_or_else(|p| p.into_inner());
            if gate.closed {
                let finished = self.stats.finished.load(Ordering::Acquire);
                return self
                    .stats
                    .report(self.stats.get(Stat::LastEnqueued), finished, finished);
            }
            let through = self.stats.get(Stat::LastEnqueued);
            let message = Message::Barrier {
                through,
                finish,
                reason,
                deadline,
                reply: reply.clone(),
            };
            match self.sender.try_send(message) {
                Ok(()) => {
                    if finish {
                        gate.closed = true;
                    }
                    drop(gate);
                    drop(wake);
                    drop(reply);
                    match receive.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                        Ok(report) => return report,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            self.stats.fault(Fault::FlushTimeout);
                            return self.stats.report(through, false, false);
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            self.stats.fault(Fault::Io);
                            return self.stats.report(through, false, false);
                        }
                    }
                }
                Err(TrySendError::Disconnected(_)) => {
                    self.stats.fault(Fault::Io);
                    return self.stats.report(through, false, false);
                }
                Err(TrySendError::Full(_)) => {
                    drop(gate);
                    wake.wait_until(generation, deadline);
                }
            }
            // The Full path waits on the captured generation; successful sends
            // release the guard before waiting for the writer reply.
            if Instant::now() >= deadline {
                self.stats.fault(Fault::FlushTimeout);
                return self
                    .stats
                    .report(self.stats.get(Stat::LastEnqueued), false, false);
            }
        }
    }
}

#[cfg(all(windows, debug_assertions))]
#[path = "collector/native.rs"]
mod native;
#[cfg(all(windows, debug_assertions))]
pub(super) use native::*;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
