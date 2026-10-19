use super::*;
use std::io;

#[derive(Clone, Default)]
struct Memory(Arc<Mutex<Vec<u8>>>);
impl Write for Memory {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Memory {
    fn values(&self) -> Vec<serde_json::Value> {
        self.0
            .lock()
            .unwrap()
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect()
    }
}
fn context() -> Context {
    Context {
        run: "portable-unit-run".into(),
        role: Role::Worker,
        pid: 42,
        instance: 100,
        parent_pid: Some(41),
        parent_instance: Some(99),
        channel: Some(7),
    }
}
fn engine(writer: impl Write + Send + 'static, limits: Limits) -> Engine {
    Engine::new(
        writer,
        context(),
        10_000_000,
        Arc::new(|| Some(123)),
        limits,
        Arc::new(Stats::default()),
    )
    .ok()
    .unwrap()
}
fn finish(engine: &Engine) -> FlushReport {
    engine.barrier(true, "unit-complete", Duration::from_secs(2))
}

#[test]
fn complete_trace_keeps_exact_decimal_fields_and_checkpoints_a_prefix() {
    let memory = Memory::default();
    let engine = engine(memory.clone(), Limits::default());
    assert_eq!(engine.context.channel, Some(7));
    engine.record_at(
        Stamp { qpc: u64::MAX },
        Kind::SourceCapture,
        &[u64::MAX; MAX_FIELDS],
    );
    let prefix = engine.barrier(false, "stopped", Duration::from_secs(2));
    assert!(prefix.valid && prefix.complete && !prefix.finished);
    assert_eq!(prefix.through_sequence, 1);
    engine.record_at(
        Stamp { qpc: 200 },
        Kind::SubmitReturned,
        &[7, 1, 9, 1, 1, 1],
    );
    let report = finish(&engine);
    assert!(report.valid && report.complete && report.finished);
    assert_eq!(
        (
            report.counters.attempted,
            report.counters.enqueued,
            report.counters.written
        ),
        (2, 2, 2)
    );
    let rows = memory.values();
    assert_eq!(rows[0]["qpcFrequency"], "10000000");
    assert_eq!(rows[0]["parentInstance"], "99");
    assert_eq!(rows[1]["qpc"], u64::MAX.to_string());
    assert!(rows[1]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .all(|n| n == &u64::MAX.to_string()));
    assert_eq!(rows.last().unwrap()["type"], "footer");
    assert_eq!(rows.last().unwrap()["report"]["counters"]["written"], "2");
}

#[test]
fn record_cap_refuses_extra_events_and_invalidates_evidence() {
    let memory = Memory::default();
    let engine = engine(
        memory.clone(),
        Limits {
            records: 2,
            ..Limits::default()
        },
    );
    for n in 1..=3 {
        engine.record_at(Stamp { qpc: n }, Kind::Marker, &[n]);
    }
    let report = finish(&engine);
    assert!(report.complete && !report.valid);
    assert_eq!(
        (
            report.counters.attempted,
            report.counters.written,
            report.counters.cap_drops
        ),
        (3, 2, 1)
    );
    assert_eq!(
        memory
            .values()
            .iter()
            .filter(|value| value["type"] == "event")
            .count(),
        2
    );
}

struct BlockingWriter {
    memory: Memory,
    block: Arc<AtomicBool>,
    entered: SyncSender<()>,
    release: mpsc::Receiver<()>,
}
impl Write for BlockingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.block.swap(false, Ordering::AcqRel) {
            let _ = self.entered.try_send(());
            self.release
                .recv_timeout(Duration::from_secs(5))
                .map_err(|_| io::Error::other("unit writer release timed out"))?;
        }
        self.memory.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn blocked_engine() -> (Engine, Arc<AtomicBool>, mpsc::Receiver<()>, SyncSender<()>) {
    let block = Arc::new(AtomicBool::new(false));
    let (entered, waiting) = mpsc::sync_channel(1);
    let (release, resume) = mpsc::sync_channel(1);
    let writer = BlockingWriter {
        memory: Memory::default(),
        block: block.clone(),
        entered,
        release: resume,
    };
    let engine = engine(
        writer,
        Limits {
            queue: 1,
            buffer: 1,
            ..Limits::default()
        },
    );
    (engine, block, waiting, release)
}
#[test]
fn saturated_queue_never_waits_for_the_writer_and_accounts_for_loss() {
    let (engine, block, waiting, release) = blocked_engine();
    block.store(true, Ordering::Release);
    engine.record_at(Stamp { qpc: 1 }, Kind::Marker, &[1]);
    waiting.recv_timeout(Duration::from_secs(2)).unwrap();
    engine.record_at(Stamp { qpc: 2 }, Kind::Marker, &[2]);
    engine.record_at(Stamp { qpc: 3 }, Kind::Marker, &[3]);
    assert_eq!(engine.stats.get(Stat::QueueDrops), 1);
    release.send(()).unwrap();
    let report = finish(&engine);
    assert!(!report.valid && report.complete);
    assert_eq!((report.counters.enqueued, report.counters.written), (2, 2));
}
#[test]
fn bounded_flush_timeout_is_observable_and_cannot_be_valid() {
    let (engine, block, waiting, release) = blocked_engine();
    block.store(true, Ordering::Release);
    engine.record_at(Stamp { qpc: 1 }, Kind::Marker, &[1]);
    waiting.recv_timeout(Duration::from_secs(2)).unwrap();
    let report = engine.barrier(false, "unit-timeout", Duration::from_millis(10));
    assert!(!report.complete && !report.valid);
    assert!(report.counters.flush_timeouts > 0);
    release.send(()).unwrap();
    assert!(!finish(&engine).valid);
}

struct FinalControlBlockingWriter {
    memory: Memory,
    entered: SyncSender<()>,
    release: mpsc::Receiver<()>,
}
impl Write for FinalControlBlockingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let row: serde_json::Value = serde_json::from_slice(bytes)?;
        if row["type"] == "footer" {
            let _ = self.entered.try_send(());
            self.release.recv_timeout(Duration::from_secs(5))
                .map_err(|_| io::Error::other("unit final-control release timed out"))?;
        }
        self.memory.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

#[test]
fn final_control_write_timeout_requires_an_independent_diagnostic_worker_exit() {
    let memory = Memory::default();
    let (entered, waiting) = mpsc::sync_channel(1);
    let (release, resume) = mpsc::sync_channel(1);
    let engine = Arc::new(engine(FinalControlBlockingWriter {
        memory: memory.clone(), entered, release: resume,
    }, Limits { buffer: 1, ..Limits::default() }));
    engine.record_at(Stamp { qpc: 1 }, Kind::Marker, &[1]);
    let pending = engine.clone();
    let finishing = thread::spawn(move || pending.barrier(true, "unit-final-control", Duration::from_millis(200)));
    waiting.recv_timeout(Duration::from_secs(2)).unwrap();
    let report = finishing.join().unwrap();
    assert_eq!(report.counters.written, 1, "all event writes completed before the final control write");
    assert!(!report.complete && !report.valid && !report.finished);
    assert!(report.counters.flush_timeouts > 0);
    assert_eq!(crate::entry::audit_exit_code(0, &report), 87);
    // A delayed control write may serialize an earlier report. Its bytes cannot
    // override the independent exit proof after the bounded caller timed out.
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !memory.values().iter().any(|row| row["type"] == "footer") {
        assert!(Instant::now() < deadline, "released final-control writer did not finish");
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(memory.values().iter().filter(|row| row["type"] == "footer").count(), 1);
}
#[test]
fn alias_keys_are_bounded_stable_kind_scoped_and_never_written() {
    let memory = Memory::default();
    let engine = engine(
        memory.clone(),
        Limits {
            aliases: 2,
            alias_bytes: 64,
            ..Limits::default()
        },
    );
    let secret = b"private-key-must-not-appear-in-sidecar";
    let first = engine.alias(AliasKind::Run, secret).unwrap();
    assert_eq!(engine.alias(AliasKind::Run, secret), Some(first));
    let second = engine.alias(AliasKind::Port, secret).unwrap();
    assert_ne!(first, second);
    assert_eq!(engine.alias(AliasKind::Run, b"third"), None);
    assert_eq!(engine.alias(AliasKind::Run, &[1; 65]), None);
    engine.record_at(Stamp { qpc: 1 }, Kind::Marker, &[first, second]);
    let report = finish(&engine);
    assert_eq!(report.counters.alias_failures, 2);
    assert!(!report.valid);
    assert!(!String::from_utf8(memory.0.lock().unwrap().clone())
        .unwrap()
        .contains(std::str::from_utf8(secret).unwrap()));
}
#[test]
fn invalid_fields_clock_and_after_finish_calls_invalidate_the_report() {
    let engine = engine(Memory::default(), Limits::default());
    engine.record_at(Stamp { qpc: 1 }, Kind::Marker, &[0; MAX_FIELDS + 1]);
    engine.record_at(Stamp { qpc: 0 }, Kind::Marker, &[1]);
    assert!(!finish(&engine).valid);
    engine.record_at(Stamp { qpc: 3 }, Kind::Marker, &[1]);
    let report = finish(&engine);
    assert_eq!(
        (
            report.counters.invalid_fields,
            report.counters.qpc_failures,
            report.counters.after_finish
        ),
        (1, 1, 1)
    );
    assert!(!report.valid);
}
struct FailingWriter {
    memory: Memory,
    fail: Arc<AtomicBool>,
    flush_only: bool,
}
impl Write for FailingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !self.flush_only && self.fail.load(Ordering::Acquire) {
            return Err(io::Error::other("unit write failure"));
        }
        self.memory.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.flush_only && self.fail.load(Ordering::Acquire) {
            return Err(io::Error::other("unit flush failure"));
        }
        Ok(())
    }
}
#[test]
fn write_and_flush_errors_never_produce_valid_completion() {
    for flush_only in [false, true] {
        let fail = Arc::new(AtomicBool::new(false));
        let writer = FailingWriter {
            memory: Memory::default(),
            fail: fail.clone(),
            flush_only,
        };
        let engine = engine(
            writer,
            Limits {
                buffer: 1,
                ..Limits::default()
            },
        );
        fail.store(true, Ordering::Release);
        engine.record_at(Stamp { qpc: 1 }, Kind::Marker, &[1]);
        let report = finish(&engine);
        assert!(!report.valid && !report.complete);
        assert!(report.counters.io_errors > 0);
    }
}
#[test]
fn byte_limit_is_hard_and_missing_footer_cannot_be_success() {
    let memory = Memory::default();
    let engine = engine(
        memory.clone(),
        Limits {
            bytes: 768,
            ..Limits::default()
        },
    );
    engine.record_at(Stamp { qpc: 1 }, Kind::Marker, &[1]);
    let report = finish(&engine);
    assert!(!report.valid && !report.complete);
    assert!(report.counters.cap_drops > 0);
    assert!(memory.0.lock().unwrap().len() <= 768);
}
#[test]
fn injected_clock_failure_and_identity_exhaustion_are_not_hidden() {
    let engine = Engine::new(
        Memory::default(),
        context(),
        1,
        Arc::new(|| None),
        Limits::default(),
        Arc::new(Stats::default()),
    )
    .ok()
    .unwrap();
    assert!(engine.stamp().is_none());
    engine.gate.lock().unwrap().next_id = u64::MAX;
    assert_eq!(engine.next_id(), None);
    let report = finish(&engine);
    assert_eq!(report.counters.qpc_failures, 1);
    assert_eq!(report.counters.evidence_faults, 1);
    assert!(!report.valid);
}
#[cfg(not(all(windows, debug_assertions)))]
#[test]
fn portable_public_api_is_a_complete_noop() {
    super::super::initialize(Role::Parent);
    assert!(!super::super::enabled());
    assert_eq!(super::super::stamp(), None);
    assert_eq!(super::super::next_id(), None);
    assert_eq!(super::super::alias(AliasKind::Run, b"private"), None);
    super::super::record(Kind::Marker, &[1]);
    assert!(!super::super::checkpoint(Checkpoint::Stopped, Duration::ZERO).valid);
}
