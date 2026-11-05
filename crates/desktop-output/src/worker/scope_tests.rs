use super::*;
use osg_mapping::{desktop::{Delivery, Key}, engine::typed::Token};
use std::sync::Mutex;

#[path = "retirement_tests.rs"]
mod retirement_tests;

#[derive(Clone)]
struct Source(Arc<Mutex<State>>);
struct State {
    loss: u64,
    eligible: bool,
    immediate: bool,
    failed: bool,
    bound: Vec<u64>,
    retired: Vec<u64>,
    events: Vec<NativeEvent>,
    immediate_reads: usize,
}
impl Source {
    fn new() -> Self { Self(Arc::new(Mutex::new(State {
        loss: 0, eligible: true, immediate: true, failed: false,
        bound: vec![], retired: vec![], events: vec![], immediate_reads: 0,
    }))) }
}
impl Adapter for Source {
    fn bind_scope(&mut self, registration: u64, scope: &Scope) -> Result<ScopeEvidence, String> {
        self.0.lock().unwrap().bound.push(registration);
        self.scope_evidence(registration, scope)
    }
    fn scope_evidence(&mut self, _: u64, _: &Scope) -> Result<ScopeEvidence, String> {
        let state = self.0.lock().unwrap();
        if state.failed { return Err("observer stopped".into()); }
        Ok(ScopeEvidence { loss_epoch: state.loss, eligible: state.eligible })
    }
    fn retire_scope(&mut self, registration: u64) { self.0.lock().unwrap().retired.push(registration); }
    fn notification_driven(&self) -> bool { true }
    fn eligible(&mut self, _: &Scope) -> Result<bool, String> {
        let mut state = self.0.lock().unwrap();
        state.immediate_reads += 1;
        Ok(state.immediate)
    }
    fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String> {
        self.0.lock().unwrap().events.extend_from_slice(events);
        Ok(native_count(events))
    }
}
fn run() -> RunToken { RunToken { session: "native-scope".into(), generation: Counter(1) } }
fn producer() -> Token { Token { run: run(), node_id: "button".into(), dependencies: vec![] } }
fn arm() -> Command { Command::Arm { producer: producer(), action_id: "action".into() } }
fn deliver(operation: u64, intent: Intent) -> Command {
    Command::Deliver { delivery: Delivery { producer: producer(), action_id: "action".into(),
        operation_id: Counter(operation), sequence: Counter(operation), at_us: Counter(0),
        deadline_us: None, intent } }
}
fn down(operation: u64) -> Command {
    deliver(operation, Intent::Held { controls: vec![Control::Key { key: Key(4) }] })
}
fn send(worker: &mut Worker<Source>, sequence: &mut u64, proof: Option<ScopeAdmission>, command: Command, now: u64) -> Ack {
    *sequence += 1;
    worker.receive(Request { sequence: Counter(*sequence), admission: proof, deadline_us: None, command }, now)
}
fn setup() -> (Worker<Source>, Source, u64, ScopeAdmission) {
    let source = Source::new();
    let mut worker = Worker::new(source.clone());
    let mut sequence = 0;
    let ack = send(&mut worker, &mut sequence, None, Command::Activate { run: run(), scope: Scope::DesktopWide }, 0);
    assert!(ack.ok);
    let proof = ack.scopes[0].admission;
    assert_eq!(proof, ScopeAdmission { registration: Counter(1), epoch: Counter(1) });
    assert!(send(&mut worker, &mut sequence, Some(proof), arm(), 0).ok);
    (worker, source, sequence, proof)
}

#[test]
fn retained_away_return_revokes_without_new_input_and_stale_packets_cannot_release_new_hold() {
    let (mut worker, source, mut sequence, old) = setup();
    assert!(send(&mut worker, &mut sequence, Some(old), down(1), 1).ok);
    source.0.lock().unwrap().loss += 1; // retained away/back while currently eligible
    worker.tick(2).unwrap();
    assert!(worker.held().is_empty());
    assert_eq!(source.0.lock().unwrap().immediate_reads, 2); // idle never calls eligible
    let fresh = worker.scope_receipts()[0].admission;
    assert_eq!(fresh.epoch, Counter(2));
    assert!(send(&mut worker, &mut sequence, Some(fresh), arm(), 3).ok);
    assert!(send(&mut worker, &mut sequence, Some(fresh), down(1), 3).ok);
    let mut deliveries = Vec::new();
    for (operation, node) in [(2, "pointer-a"), (3, "pointer-b")] {
        let Command::Deliver { mut delivery } = deliver(operation, Intent::Pointer { dx: 1, dy: 0 }) else { unreachable!() };
        delivery.producer.node_id = node.into();
        let ack = send(&mut worker, &mut sequence, Some(fresh), Command::Arm {
            producer: delivery.producer.clone(), action_id: delivery.action_id.clone() }, 3);
        assert!(ack.ok && !ack.skipped, "{ack:?}");
        deliveries.push(delivery);
    }
    let pointer = Command::DeliverPointerBatch { deliveries };
    for command in [arm(), down(2), pointer] {
        let ack = send(&mut worker, &mut sequence, Some(old), command, 4);
        assert!(ack.ok && ack.skipped && ack.scope_rejected, "{ack:?}");
        assert_eq!((ack.attempted, ack.confirmed, ack.held.len()), (0, 0, 1), "{ack:?}");
    }
    assert_eq!(source.0.lock().unwrap().events.len(), 3);
}

#[test]
fn synchronous_rejection_releases_outside_incoming_counts_and_advances_epoch() {
    let (mut worker, source, mut sequence, old) = setup();
    assert!(send(&mut worker, &mut sequence, Some(old), down(1), 0).ok);
    source.0.lock().unwrap().immediate = false;
    let ack = send(&mut worker, &mut sequence, Some(old), down(2), 1);
    assert!(ack.ok && ack.skipped && ack.scope_rejected && ack.held.is_empty());
    assert_eq!((ack.attempted, ack.confirmed), (0, 0));
    assert_eq!(ack.scopes[0].admission.epoch, Counter(2));
    assert_eq!(source.0.lock().unwrap().events.len(), 2);
    source.0.lock().unwrap().immediate = true;
    let ack = send(&mut worker, &mut sequence, Some(ack.scopes[0].admission), arm(), 2);
    assert!(ack.ok && !ack.skipped);
}

#[test]
fn expiry_retires_immediately_and_retains_one_ack_tombstone() {
    let (mut worker, source, mut sequence, proof) = setup();
    assert!(send(&mut worker, &mut sequence, Some(proof), down(1), 0).ok);
    worker.tick(LEASE_US).unwrap();
    assert_eq!(source.0.lock().unwrap().retired, [1]);
    assert!(worker.held().is_empty());
    assert_eq!(worker.wait_duration(LEASE_US), None);
    let ack = send(&mut worker, &mut sequence, None, Command::Renew { run: run() }, LEASE_US);
    assert!(!ack.ok && ack.scopes.len() == 1 && !ack.scopes[0].live);
    assert_eq!(ack.scopes[0].admission.epoch, Counter(2));
    let ack = send(&mut worker, &mut sequence, None, Command::Pause { run: run() }, LEASE_US);
    assert!(ack.ok && ack.scopes.is_empty());
    assert_eq!(source.0.lock().unwrap().retired, [1]);
}

#[test]
fn paused_lease_wait_and_guard_deadline_are_exact_and_observer_failure_cleans_all() {
    let (mut worker, source, mut sequence, proof) = setup();
    assert_eq!(worker.wait_duration(7), Some(Duration::from_micros(LEASE_US - 7)));
    let Command::Deliver { delivery } = down(1) else { unreachable!() };
    assert!(send(&mut worker, &mut sequence, Some(proof), Command::DeliverGuarded {
        delivery, release_after_us: Counter(1_000) }, 10).ok);
    assert_eq!(worker.wait_duration(11), Some(Duration::from_micros(999)));
    source.0.lock().unwrap().failed = true;
    assert!(worker.tick(12).is_err());
    assert!(worker.fault().is_some() && worker.held().is_empty());
    assert!(worker.guards.wait(12).is_none() && worker.scope_receipts().is_empty());
    assert_eq!(source.0.lock().unwrap().retired, [1]);
}

#[test]
fn ordinary_pause_does_not_advance_epoch_and_deadline_cancel_remains_healthy() {
    let (mut worker, source, mut sequence, proof) = setup();
    let ack = send(&mut worker, &mut sequence, None, Command::Pause { run: run() }, 1);
    assert!(ack.ok && ack.scopes[0].admission == proof);
    assert_eq!(worker.wait_duration(1), Some(Duration::from_micros(LEASE_US - 1)));
    source.0.lock().unwrap().immediate = false;
    sequence += 1;
    let ack = worker.receive(Request { sequence: Counter(sequence), admission: Some(proof), deadline_us: Some(Counter(0)), command: arm() }, 2);
    assert!(ack.ok && ack.skipped && !ack.scope_rejected);
    assert_eq!(ack.scopes[0].admission, proof);
}

#[test]
fn tombstones_count_toward_the_bound_until_the_next_ack() {
    let source = Source::new();
    let mut worker = Worker::new(source.clone());
    let mut sequence = 0;
    for index in 0..64 {
        let mut token = run();
        token.session = format!("bounded-{index}");
        assert!(send(&mut worker, &mut sequence, None, Command::Activate {
            run: token, scope: Scope::DesktopWide }, 0).ok);
    }
    worker.tick(LEASE_US).unwrap();
    assert_eq!(source.0.lock().unwrap().retired.len(), 64);
    let ack = send(&mut worker, &mut sequence, None, Command::Activate {
        run: run(), scope: Scope::DesktopWide }, LEASE_US);
    assert!(!ack.ok && ack.scopes.len() == 64 && ack.scopes.iter().all(|scope| !scope.live));
    let ack = send(&mut worker, &mut sequence, None, Command::Activate {
        run: run(), scope: Scope::DesktopWide }, LEASE_US);
    assert!(ack.ok && ack.scopes.len() == 1 && ack.scopes[0].live);
    let ack = send(&mut worker, &mut sequence, None, Command::Stop, LEASE_US);
    assert!(ack.ok && ack.scopes.is_empty());
    assert_eq!(source.0.lock().unwrap().retired.len(), 65);
}

#[test]
fn initial_ineligibility_does_not_prevent_current_proof_neutral_rearm() {
    let source = Source::new();
    source.0.lock().unwrap().eligible = false;
    let mut worker = Worker::new(source.clone());
    let mut sequence = 0;
    let ack = send(&mut worker, &mut sequence, None, Command::Activate {
        run: run(), scope: Scope::DesktopWide }, 0);
    let proof = ack.scopes[0].admission;
    assert!(!ack.scopes[0].eligible);
    let ack = send(&mut worker, &mut sequence, Some(proof), arm(), 1);
    assert!(ack.ok && !ack.skipped && ack.scopes[0].admission == proof);
    // The cached source remains false until its own event. It must not produce
    // a second loss merely because synchronous admission observed a return.
    let ack = send(&mut worker, &mut sequence, Some(proof), down(1), 2);
    assert!(ack.ok && !ack.skipped && ack.held.len() == 1);
}

#[test]
fn missing_proof_is_a_protocol_error_and_recording_wait_stays_bounded() {
    let (mut worker, _, mut sequence, _) = setup();
    let ack = send(&mut worker, &mut sequence, None, arm(), 1);
    assert!(!ack.ok && !ack.scope_rejected && !ack.uncertain);
    struct Recording;
    impl Adapter for Recording {
        fn eligible(&mut self, _: &Scope) -> Result<bool, String> { Ok(true) }
        fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String> { Ok(native_count(events)) }
    }
    let mut worker = Worker::new(Recording);
    assert_eq!(worker.wait_duration(0), Some(Duration::from_millis(5)));
    assert!(worker.receive(Request { sequence: Counter(1), admission: None, deadline_us: None,
        command: Command::Activate { run: run(), scope: Scope::DesktopWide } }, 0).ok);
    assert_eq!(worker.wait_duration(LEASE_US - 1), Some(Duration::from_micros(1)));
}

#[test]
fn native_loss_while_already_paused_still_advances_and_epoch_exhaustion_faults_closed() {
    let (mut worker, source, mut sequence, proof) = setup();
    assert!(send(&mut worker, &mut sequence, None, Command::Pause { run: run() }, 1).ok);
    source.0.lock().unwrap().eligible = false;
    worker.tick(2).unwrap();
    assert_eq!(worker.scope_receipts()[0].admission.epoch, Counter(proof.epoch.0 + 1));
    worker.tick(3).unwrap();
    assert_eq!(worker.scope_receipts()[0].admission.epoch, Counter(proof.epoch.0 + 1));
    worker.runs[0].epoch = u64::MAX;
    source.0.lock().unwrap().loss += 1;
    assert!(worker.tick(4).unwrap_err().contains("exhausted"));
    assert!(worker.fault().is_some() && worker.scope_receipts().is_empty());
    assert_eq!(source.0.lock().unwrap().retired, [1]);
}
