use std::{
    sync::{mpsc::Receiver, Arc, Condvar, Mutex, MutexGuard},
    time::Instant,
};

use super::{Fault, Message, Stats};

#[derive(Default)]
pub(super) struct CapacityWake {
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Default)]
struct State {
    generation: u64,
    terminated: bool,
}

pub(super) struct CapacityGuard<'a> {
    wake: &'a CapacityWake,
    state: Option<MutexGuard<'a, State>>,
}

pub(super) struct WorkerExit {
    receiver: Option<Receiver<Message>>,
    capacity: Arc<CapacityWake>,
    stats: Arc<Stats>,
}

impl WorkerExit {
    pub(super) fn new(
        receiver: Receiver<Message>,
        capacity: Arc<CapacityWake>,
        stats: Arc<Stats>,
    ) -> Self {
        Self { receiver: Some(receiver), capacity, stats }
    }

    pub(super) fn receiver(&self) -> &Receiver<Message> {
        self.receiver.as_ref().unwrap()
    }

    pub(super) fn capacity(&self) -> &CapacityWake {
        &self.capacity
    }
}

impl Drop for WorkerExit {
    fn drop(&mut self) {
        self.receiver.take();
        if std::thread::panicking() {
            self.stats.fault(Fault::Io);
        }
        self.capacity.terminated();
    }
}

impl CapacityWake {
    pub(super) fn lock(&self) -> CapacityGuard<'_> {
        CapacityGuard {
            wake: self,
            state: Some(self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())),
        }
    }

    pub(super) fn dequeued(&self) {
        self.signal(false);
    }

    pub(super) fn terminated(&self) {
        self.signal(true);
    }

    fn signal(&self, terminated: bool) {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.generation = state.generation.wrapping_add(1);
        state.terminated |= terminated;
        self.changed.notify_all();
    }

}

impl CapacityGuard<'_> {
    pub(super) fn generation(&self) -> u64 {
        self.state.as_ref().unwrap().generation
    }

    pub(super) fn wait_until(mut self, generation: u64, deadline: Instant) {
        while self.state.as_ref().unwrap().generation == generation
            && !self.state.as_ref().unwrap().terminated
        {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let (state, timeout) = self
                .wake
                .changed
                .wait_timeout(self.state.take().unwrap(), remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            self.state = Some(state);
            if timeout.timed_out() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use std::{io, sync::{Arc, Mutex}};

    fn context() -> Context {
        Context {
            run: "capacity-wake-test".into(),
            role: Role::Worker,
            pid: 42,
            instance: 100,
            parent_pid: Some(41),
            parent_instance: Some(99),
            channel: Some(7),
        }
    }

    struct BlockingWriter {
        entered: SyncSender<()>,
        release: mpsc::Receiver<()>,
        bytes: Arc<Mutex<Vec<u8>>>,
        blocked_first_event: bool,
    }

    impl Write for BlockingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if !self.blocked_first_event
                && bytes.windows(b"\"event\"".len()).any(|part| part == b"\"event\"")
            {
                self.blocked_first_event = true;
                let _ = self.entered.try_send(());
                self.release
                    .recv_timeout(Duration::from_secs(4))
                    .map_err(|_| io::Error::other("blocked audit writer timed out"))?;
            }
            self.bytes.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn blocked_engine() -> (Engine, mpsc::Receiver<()>, SyncSender<()>, Arc<Mutex<Vec<u8>>>) {
        let (entered, waiting) = mpsc::sync_channel(1);
        let (release, resume) = mpsc::sync_channel(1);
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let engine = Engine::new(
            BlockingWriter {
                entered,
                release: resume,
                bytes: bytes.clone(),
                blocked_first_event: false,
            },
            context(),
            10_000_000,
            Arc::new(|| Some(123)),
            Limits { queue: 1, buffer: 1, ..Limits::default() },
            Arc::new(Stats::default()),
        ).unwrap();
        (engine, waiting, release, bytes)
    }

    fn saturate(engine: &Engine, waiting: &mpsc::Receiver<()>) {
        engine.record_at(Stamp { qpc: 1 }, Kind::Marker, &[1]);
        waiting.recv_timeout(Duration::from_secs(2)).unwrap();
        engine.record_at(Stamp { qpc: 2 }, Kind::Marker, &[2]);
    }

    #[test]
    fn capacity_dequeue_wakes_pending_barrier_before_its_deadline() {
        let (engine, waiting, release, _) = blocked_engine();
        saturate(&engine, &waiting);
        let engine = Arc::new(engine);
        let pending = engine.clone();
        let started = Instant::now();
        let barrier = thread::spawn(move || pending.barrier(false, "capacity-wake", Duration::from_secs(2)));
        thread::sleep(Duration::from_millis(30));
        release.send(()).unwrap();
        let report = barrier.join().unwrap();
        assert!(report.complete && !report.finished, "capacity wake report: {report:?}");
        assert_eq!(report.through_sequence, 2);
        assert!(started.elapsed() < Duration::from_secs(1), "capacity wake took {:?}; report: {report:?}", started.elapsed());
        assert!(engine.barrier(true, "finish", Duration::from_secs(2)).complete);
    }

    #[test]
    fn full_queue_wait_expires_at_the_original_deadline() {
        let (engine, waiting, release, _) = blocked_engine();
        saturate(&engine, &waiting);
        let started = Instant::now();
        let report = engine.barrier(false, "capacity-timeout", Duration::from_millis(40));
        assert!(!report.complete && !report.valid);
        assert!(report.counters.flush_timeouts > 0);
        assert!(started.elapsed() < Duration::from_millis(500));
        release.send(()).unwrap();
        let _ = engine.barrier(true, "finish", Duration::from_secs(2));
    }

    struct SwitchFailWriter(Arc<AtomicBool>);
    impl Write for SwitchFailWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.0.load(Ordering::Acquire) {
                return Err(io::Error::other("injected audit write failure"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> { Ok(()) }
    }

    #[test]
    fn writer_failure_disconnects_pending_barrier_without_waiting_for_deadline() {
        let fail = Arc::new(AtomicBool::new(false));
        let engine = Engine::new(
            SwitchFailWriter(fail.clone()), context(), 10_000_000,
            Arc::new(|| Some(123)), Limits { queue: 1, buffer: 1, ..Limits::default() },
            Arc::new(Stats::default()),
        ).unwrap();
        fail.store(true, Ordering::Release);
        engine.record_at(Stamp { qpc: 1 }, Kind::Marker, &[1]);
        let deadline = Instant::now() + Duration::from_secs(1);
        while engine.stats.get(Stat::IoErrors) == 0 {
            assert!(Instant::now() < deadline, "writer did not reach its injected failure");
            thread::yield_now();
        }
        let started = Instant::now();
        let report = engine.barrier(false, "writer-disconnected", Duration::from_secs(2));
        assert!(!report.complete && !report.valid);
        assert!(report.counters.io_errors > 0);
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn concurrent_finishers_emit_only_one_footer_after_capacity_wait() {
        let (engine, waiting, release, bytes) = blocked_engine();
        saturate(&engine, &waiting);
        let engine = Arc::new(engine);
        let first_engine = engine.clone();
        let second_engine = engine.clone();
        let first = thread::spawn(move || first_engine.barrier(true, "finish-one", Duration::from_secs(2)));
        let second = thread::spawn(move || second_engine.barrier(true, "finish-two", Duration::from_secs(2)));
        thread::sleep(Duration::from_millis(30));
        release.send(()).unwrap();
        let first_report = first.join().unwrap();
        let second_report = second.join().unwrap();
        let text = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
        assert_eq!(
            text.matches("\"type\":\"footer\"").count(),
            1,
            "finish reports: first={first_report:?}, second={second_report:?}"
        );
    }

    struct BlockingTerminalWriter {
        entered: SyncSender<()>,
        release: mpsc::Receiver<()>,
        panic_on_event: bool,
    }

    impl Write for BlockingTerminalWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.windows(b"\"event\"".len()).any(|part| part == b"\"event\"") {
                let _ = self.entered.try_send(());
                self.release
                    .recv_timeout(Duration::from_secs(4))
                    .map_err(|_| io::Error::other("blocked terminal writer timed out"))?;
                if self.panic_on_event {
                    panic!("injected audit writer panic");
                }
                return Err(io::Error::other("injected terminal audit write failure"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> { Ok(()) }
    }

    fn queued_finish_then_terminal_exit(panic_on_event: bool) {
        let (entered, waiting) = mpsc::sync_channel(1);
        let (release, resume) = mpsc::sync_channel(1);
        let engine = Arc::new(Engine::new(
            BlockingTerminalWriter { entered, release: resume, panic_on_event },
            context(),
            10_000_000,
            Arc::new(|| Some(123)),
            Limits { queue: 1, buffer: 1, ..Limits::default() },
            Arc::new(Stats::default()),
        ).unwrap());
        engine.record_at(Stamp { qpc: 1 }, Kind::Marker, &[1]);
        waiting.recv_timeout(Duration::from_secs(2)).unwrap();

        let finishing_engine = engine.clone();
        let barrier = thread::spawn(move || {
            finishing_engine.barrier(true, "queued-terminal-exit", Duration::from_secs(2))
        });
        let queued_deadline = Instant::now() + Duration::from_secs(1);
        while !engine.gate.lock().unwrap().closed {
            assert!(Instant::now() < queued_deadline, "finish barrier was not enqueued");
            thread::yield_now();
        }

        let started = Instant::now();
        release.send(()).unwrap();
        let report = barrier.join().unwrap();
        assert!(!report.complete && !report.valid && !report.finished);
        assert!(report.counters.io_errors > 0);
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn queued_barrier_gets_io_failure_when_event_write_terminates_worker() {
        queued_finish_then_terminal_exit(false);
    }

    #[test]
    fn queued_barrier_gets_io_failure_when_worker_panics() {
        queued_finish_then_terminal_exit(true);
    }
}
