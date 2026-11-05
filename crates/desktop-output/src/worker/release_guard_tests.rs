use super::*;
use osg_mapping::{desktop::{Control, Key}, engine::typed::{Token, Dependency}, program::Endpoint};
use std::sync::{Mutex, atomic::{AtomicBool, AtomicUsize}};

#[derive(Clone)]
struct Recording {
    batches: Arc<Mutex<Vec<Vec<NativeEvent>>>>,
    contexts: Arc<Mutex<Vec<(u64, u64, u64)>>>,
    eligible: Arc<AtomicBool>,
    short: Arc<AtomicUsize>,
}
impl Recording {
    fn new() -> Self {
        Self { batches: Arc::default(), contexts: Arc::default(), eligible: Arc::new(AtomicBool::new(true)), short: Arc::new(AtomicUsize::new(usize::MAX)) }
    }
}
impl Adapter for Recording {
    fn eligible(&mut self, _: &Scope) -> Result<bool, String> { Ok(self.eligible.load(Ordering::Relaxed)) }
    fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String> {
        self.batches.lock().unwrap().push(events.to_vec());
        let context = submission_context().expect("native submissions carry audit identity");
        self.contexts.lock().unwrap().push((context.request, context.operation, context.category));
        let short = self.short.swap(usize::MAX, Ordering::Relaxed);
        Ok(if short == usize::MAX { native_count(events) } else { short })
    }
}
fn run() -> RunToken { RunToken { session: "guard-run".into(), generation: Counter(1) } }
fn delivery(node: &str, operation: u64, intent: Intent) -> Delivery {
    Delivery { producer: Token { run: run(), node_id: node.into(), dependencies: vec![] },
        action_id: node.into(), operation_id: Counter(operation), sequence: Counter(operation),
        at_us: Counter(0), deadline_us: None, intent }
}
fn down(node: &str, operation: u64) -> Delivery {
    delivery(node, operation, Intent::Held { controls: vec![Control::Key { key: Key(4) }] })
}
fn send(worker: &mut Worker<Recording>, sequence: &mut u64, command: Command, now: u64) -> Ack {
    *sequence += 1;
    worker.receive(Request { sequence: Counter(*sequence), deadline_us: None, command }, now)
}
fn setup() -> (Worker<Recording>, Recording, u64) {
    let recording = Recording::new();
    let mut worker = Worker::new(recording.clone());
    let mut sequence = 0;
    assert!(send(&mut worker, &mut sequence, Command::Activate { run: run(), scope: Scope::DesktopWide }, 0).ok);
    arm(&mut worker, &mut sequence, &down("tap", 1), 0);
    (worker, recording, sequence)
}
fn arm(worker: &mut Worker<Recording>, sequence: &mut u64, delivery: &Delivery, now: u64) {
    assert!(send(worker, sequence, Command::Arm { producer: delivery.producer.clone(), action_id: delivery.action_id.clone() }, now).ok);
}
fn guard(worker: &mut Worker<Recording>, sequence: &mut u64, delivery: Delivery, now: u64) -> Ack {
    send(worker, sequence, Command::DeliverGuarded { delivery, release_after_us: Counter(1_000) }, now)
}

#[test]
fn native_release_runs_before_parent_up_and_is_reported_once_on_pointer_ack() {
    let (mut worker, recording, mut sequence) = setup();
    let pointer = delivery("pointer", 1, Intent::Pointer { dx: 1, dy: 2 });
    arm(&mut worker, &mut sequence, &pointer, 0);
    let ack = guard(&mut worker, &mut sequence, down("tap", 1), 10);
    let guard_id = ack.sequence;
    assert!(ack.ok && ack.released_guards.is_empty() && ack.held.len() == 1);
    worker.tick(1_009).unwrap();
    assert_eq!(worker.held().len(), 1);
    worker.tick(1_010).unwrap();
    assert!(worker.held().is_empty());
    let ack = send(&mut worker, &mut sequence, Command::Deliver { delivery: pointer }, 1_011);
    assert!(ack.ok && ack.held.is_empty());
    assert_eq!(ack.released_guards, vec![guard_id]);
    let up = delivery("tap", 2, Intent::Held { controls: vec![] });
    let ack = send(&mut worker, &mut sequence, Command::Deliver { delivery: up }, 1_012);
    assert!(ack.ok && ack.released_guards.is_empty());
    assert_eq!(recording.batches.lock().unwrap().len(), 3);
    let contexts = recording.contexts.lock().unwrap();
    assert_eq!(contexts[0], (guard_id.0, 1, 1));
    assert_eq!(contexts[1], (guard_id.0, 1, 7));
}

#[test]
fn fresh_post_submit_anchor_prevents_old_request_time_shortening_the_dwell() {
    let (mut worker, _, mut sequence) = setup();
    let ack = guard(&mut worker, &mut sequence, down("tap", 1), 0);
    worker.anchor_release_guard(&ack, 5_000);
    worker.tick(5_999).unwrap();
    assert_eq!(worker.held().len(), 1);
    assert_eq!(worker.guards.wait(5_999), Duration::from_micros(1));
    worker.tick(6_000).unwrap();
    assert!(worker.held().is_empty());
}

#[test]
fn shared_hold_survives_guard_without_a_manufactured_release_edge() {
    let (mut worker, recording, mut sequence) = setup();
    let hold = down("hold", 1);
    arm(&mut worker, &mut sequence, &hold, 0);
    assert!(send(&mut worker, &mut sequence, Command::Deliver { delivery: hold }, 0).ok);
    let ack = guard(&mut worker, &mut sequence, down("tap", 1), 1);
    assert!(ack.ok && ack.collision && ack.attempted == 0);
    worker.tick(1_001).unwrap();
    assert_eq!(worker.held().len(), 1);
    assert_eq!(recording.batches.lock().unwrap().len(), 1);
    let ack = send(&mut worker, &mut sequence, Command::Renew { run: run() }, 1_002);
    assert_eq!(ack.released_guards.len(), 1);
}

#[test]
fn ordinary_up_or_new_operation_cancels_old_guard() {
    for replacement in [Intent::Held { controls: vec![] }, down("tap", 2).intent] {
        let (mut worker, recording, mut sequence) = setup();
        assert!(guard(&mut worker, &mut sequence, down("tap", 1), 0).ok);
        assert!(send(&mut worker, &mut sequence, Command::Deliver { delivery: delivery("tap", 2, replacement.clone()) }, 1).ok);
        let count = recording.batches.lock().unwrap().len();
        worker.tick(1_001).unwrap();
        assert_eq!(recording.batches.lock().unwrap().len(), count);
        assert!(worker.guards.active.is_empty() && worker.guards.released.is_empty());
    }
}

#[test]
fn arm_replacement_does_not_let_old_generation_guard_clear_new_contribution() {
    let (mut worker, _, mut sequence) = setup();
    assert!(guard(&mut worker, &mut sequence, down("tap", 1), 0).ok);
    let mut fresh = down("tap", 1);
    fresh.producer.dependencies = vec![Dependency {
        endpoint: Endpoint { node_id: "physical".into(), port_id: "button".into() },
        generation: Counter(2), epoch: Counter(3),
    }];
    // A new arm replaces the same ownership slot and resets its operation.
    arm(&mut worker, &mut sequence, &fresh, 1);
    assert!(send(&mut worker, &mut sequence, Command::Deliver { delivery: fresh }, 2).ok);
    worker.tick(1_001).unwrap();
    assert_eq!(worker.held().len(), 1);
    assert!(worker.guards.active.is_empty());
}

#[test]
fn focus_lease_pause_and_stop_cancel_guards() {
    for boundary in 0..4 {
        let (mut worker, recording, mut sequence) = setup();
        assert!(send(&mut worker, &mut sequence, Command::DeliverGuarded {
            delivery: down("tap", 1), release_after_us: Counter(1_000_000) }, 0).ok);
        match boundary {
            0 => { recording.eligible.store(false, Ordering::Relaxed); worker.tick(10).unwrap(); }
            1 => worker.tick(LEASE_US).unwrap(),
            2 => { assert!(send(&mut worker, &mut sequence, Command::Pause { run: run() }, 10).ok); }
            _ => { assert!(send(&mut worker, &mut sequence, Command::Stop, 10).ok); }
        }
        assert!(worker.held().is_empty() && worker.guards.active.is_empty());
        assert!(worker.guards.released.is_empty());
        assert_eq!(recording.batches.lock().unwrap().len(), 2);
    }
}

#[test]
fn short_guard_release_faults_and_attempts_cleanup_without_success_receipt() {
    let (mut worker, recording, mut sequence) = setup();
    assert!(guard(&mut worker, &mut sequence, down("tap", 1), 0).ok);
    recording.short.store(0, Ordering::Relaxed);
    assert!(worker.tick(1_000).is_err());
    assert!(worker.fault().is_some() && worker.cleanup_complete && worker.held().is_empty());
    assert!(worker.guards.active.is_empty() && worker.guards.released.is_empty());
    assert_eq!(recording.batches.lock().unwrap().len(), 3);
    let ack = send(&mut worker, &mut sequence, Command::Renew { run: run() }, 1_001);
    assert!(!ack.ok && ack.uncertain && ack.released_guards.is_empty());
}

#[test]
fn malformed_or_expired_guard_never_installs_a_timer_or_acquires_input() {
    for (intent, duration) in [
        (down("tap", 1).intent, 999), (down("tap", 1).intent, 60_000_001),
        (Intent::Held { controls: vec![] }, 1_000), (Intent::Cleanup, 1_000),
        (Intent::Pointer { dx: 1, dy: 0 }, 1_000),
    ] {
        let (mut worker, recording, mut sequence) = setup();
        let ack = send(&mut worker, &mut sequence, Command::DeliverGuarded {
            delivery: delivery("tap", 1, intent), release_after_us: Counter(duration) }, 0);
        assert!(!ack.ok && worker.guards.active.is_empty() && recording.batches.lock().unwrap().is_empty());
    }
    let (mut worker, recording, mut sequence) = setup();
    sequence += 1;
    let ack = worker.receive(Request { sequence: Counter(sequence), deadline_us: Some(Counter(0)),
        command: Command::DeliverGuarded { delivery: down("tap", 1), release_after_us: Counter(1_000) } }, 0);
    assert!(ack.ok && ack.skipped && worker.guards.active.is_empty());
    assert!(recording.batches.lock().unwrap().is_empty());
}

#[test]
fn guard_bound_is_checked_before_native_acquisition() {
    let (mut worker, recording, mut sequence) = setup();
    for id in 1..=MAX_OWNERS {
        worker.guards.released.push(Counter(id as u64 + 100));
    }
    let ack = guard(&mut worker, &mut sequence, down("tap", 1), 0);
    assert!(!ack.ok && worker.held().is_empty() && recording.batches.lock().unwrap().is_empty());
    assert_eq!(ack.released_guards.len(), MAX_OWNERS);
    assert!(worker.guards.active.is_empty());
}

#[test]
fn unarmed_invalid_and_short_down_do_not_install_a_guard() {
    for case in 0..3 {
        let (mut worker, recording, mut sequence) = setup();
        let mut attempted = down("tap", 1);
        match case {
            0 => attempted.producer.node_id = "unarmed".into(),
            1 => attempted.intent = Intent::Held { controls: vec![Control::Key { key: Key(0x48) }] },
            _ => recording.short.store(0, Ordering::Relaxed),
        }
        let ack = guard(&mut worker, &mut sequence, attempted, 0);
        assert!(!ack.ok && worker.guards.active.is_empty() && worker.held().is_empty());
        assert_eq!(ack.uncertain, case == 2);
        if case < 2 { assert!(recording.batches.lock().unwrap().is_empty()); }
    }
}
