//! A native watch can block while its observer owner shuts down. Keep this test
//! entirely on a recording adapter while exercising that blocking boundary.
use super::*;
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc};

struct BlockingRetirement {
    source: Source,
    entered: mpsc::Sender<u64>,
    resume: mpsc::Receiver<()>,
    short: Arc<AtomicBool>,
}
impl Adapter for BlockingRetirement {
    fn bind_scope(&mut self, registration: u64, scope: &Scope) -> Result<ScopeEvidence, String> {
        self.source.bind_scope(registration, scope)
    }
    fn scope_evidence(&mut self, registration: u64, scope: &Scope) -> Result<ScopeEvidence, String> {
        self.source.scope_evidence(registration, scope)
    }
    fn eligible(&mut self, scope: &Scope) -> Result<bool, String> { self.source.eligible(scope) }
    fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String> {
        let count = self.source.submit(events)?;
        Ok(if self.short.load(Ordering::Acquire) { 0 } else { count })
    }
    fn retire_scope(&mut self, registration: u64) {
        self.entered.send(registration).unwrap();
        self.resume.recv_timeout(Duration::from_secs(2)).expect("test unblocks retirement");
        self.source.retire_scope(registration);
    }
}

#[test]
fn releases_precede_blocking_retirement_on_stop_expiry_and_failure() {
    // Expiry-short additionally checks that cleanup retains a binding whose run
    // has already been marked expired but whose pause submission failed.
    for mode in ["stop", "expiry", "observer-failure", "stop-short", "expiry-short"] {
        let source = Source::new();
        let (entered, retiring) = mpsc::channel();
        let (resume, resuming) = mpsc::channel();
        let short = Arc::new(AtomicBool::new(false));
        let mut worker = Worker::new(BlockingRetirement {
            source: source.clone(), entered, resume: resuming, short: short.clone(),
        });
        let proof = Some(ScopeAdmission { registration: Counter(1), epoch: Counter(1) });
        for (index, command) in [Command::Activate { run: run(), scope: Scope::DesktopWide }, arm(), down(1)]
            .into_iter().enumerate() {
            let ack = worker.receive(Request { sequence: Counter(index as u64 + 1), admission: proof,
                deadline_us: None, command }, 0);
            assert!(ack.ok && !ack.skipped);
        }
        short.store(mode.ends_with("short"), Ordering::Release);
        source.0.lock().unwrap().failed = mode == "observer-failure";
        let thread = std::thread::spawn(move || {
            let result = if mode.starts_with("stop") { worker.stop() }
                else { worker.tick(if mode.starts_with("expiry") { LEASE_US } else { 1 }).is_ok() };
            (worker, result)
        });
        let entered = retiring.recv_timeout(Duration::from_secs(1));
        // Observe submission while retirement is still blocked, before allowing
        // any observer teardown to complete.
        let events_before_retirement = source.0.lock().unwrap().events.clone();
        let _ = resume.send(());
        let (worker, result) = thread.join().unwrap();
        assert_eq!(entered.unwrap(), 1, "{mode}");
        assert!(matches!(events_before_retirement.last(), Some(NativeEvent::Key { usage: 4, down: false, .. })), "{mode}");
        assert_eq!(source.0.lock().unwrap().retired, [1], "{mode}");
        assert!(worker.held().is_empty(), "{mode}");
        assert_eq!(result, matches!(mode, "stop" | "expiry"), "{mode}");
    }
}
