use super::*;
use crate::worker::Worker;
use osg_mapping::desktop::Key;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Mutex,
};

#[derive(Clone, Default)]
struct Recording {
    events: Arc<Mutex<Vec<NativeEvent>>>,
    fail: Arc<AtomicBool>,
}
impl Adapter for Recording {
    fn eligible(&mut self, _: &Scope) -> Result<bool, String> {
        Ok(true)
    }
    fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String> {
        self.events.lock().unwrap().extend_from_slice(events);
        if self.fail.load(Ordering::SeqCst) {
            Err("native release failed".into())
        } else {
            Ok(native_count(events))
        }
    }
}
struct Pipe {
    worker: Worker<Recording>,
    requests: Arc<Mutex<Vec<Request>>>,
}
impl Channel for Pipe {
    fn request(&mut self, request: &Request) -> Result<Ack, String> {
        self.requests.lock().unwrap().push(request.clone());
        Ok(self.worker.receive(request.clone(), 1000))
    }
}
fn run() -> RunToken {
    RunToken {
        session: "dispatch-gate".into(),
        generation: Counter(1),
    }
}
fn token(node: &str) -> Token {
    Token {
        run: run(),
        node_id: node.into(),
        dependencies: vec![],
    }
}
fn key(usage: u16) -> Control {
    Control::Key { key: Key(usage) }
}
fn delivery(node: &str, operation: u64, intent: Intent) -> Delivery {
    Delivery {
        producer: token(node),
        action_id: "action".into(),
        operation_id: Counter(operation),
        sequence: Counter(operation),
        at_us: Counter(0),
        deadline_us: Some(Counter(100)),
        intent,
    }
}
fn held(operation: u64, controls: Vec<Control>) -> Delivery {
    delivery("key", operation, Intent::Held { controls })
}
fn setup() -> (Coordinator, Recording, Arc<Mutex<Vec<Request>>>) {
    let record = Recording::default();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut coordinator = Coordinator::with_channel(
        Box::new(Pipe {
            worker: Worker::new(record.clone()),
            requests: requests.clone(),
        }),
        || 1000,
    );
    coordinator.activate(run(), Scope::DesktopWide).unwrap();
    coordinator.arm(token("key"), "action".into()).unwrap();
    (coordinator, record, requests)
}
fn assert_key_event(event: &NativeEvent, usage: u16, down: bool) {
    assert_eq!(
        *event,
        NativeEvent::Key {
            usage,
            down,
            repeat: false
        }
    );
}

#[test]
fn revocation_after_durable_preparation_skips_down_and_consumes_identity() {
    let (mut coordinator, record, requests) = setup();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.json");
    coordinator.journal = Some(path.clone());
    let allowed = Arc::new(AtomicBool::new(true));
    let gate_allowed = allowed.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let gate_calls = calls.clone();
    coordinator.set_dispatch_gate(Arc::new(move |request| {
        // Simulate authority being revoked while the durable preparation ran.
        // Reading retained evidence here proves the check follows that write.
        let prepared = Coordinator::inspect_journal(&path).unwrap();
        assert_eq!(
            prepared.in_flight.as_ref().unwrap().sequence,
            request.sequence
        );
        assert_eq!(
            prepared.in_flight.as_ref().unwrap().deadline_us,
            Some(Counter(1100))
        );
        assert_eq!(prepared.possible_held, vec![key(4)]);
        gate_allowed.store(false, Ordering::SeqCst);
        gate_calls.fetch_add(1, Ordering::SeqCst);
        gate_allowed.load(Ordering::SeqCst)
    }));
    let value = held(1, vec![key(4)]);
    let ack = coordinator.submit(value.clone(), 0).unwrap();
    assert!(ack.ok && ack.skipped && !ack.uncertain);
    assert_eq!((ack.attempted, ack.confirmed), (0, 0));
    assert!(!allowed.load(Ordering::SeqCst));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        requests.lock().unwrap().last().unwrap().deadline_us,
        Some(Counter(0))
    );
    assert!(record.events.lock().unwrap().is_empty());
    assert!(coordinator.snapshot().held.is_empty());
    assert!(coordinator.snapshot().owners[0].controls.is_empty());
    assert!(coordinator.submit(value, 0).unwrap_err().contains("Stale"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn failed_preparation_never_checks_gate_or_dispatches() {
    let (mut coordinator, record, requests) = setup();
    let dir = tempfile::tempdir().unwrap();
    coordinator.journal = Some(dir.path().join("journal.json"));
    coordinator.fail_persist_at.set(Some(1));
    coordinator.set_dispatch_gate(Arc::new(|_| panic!("failed persist must precede gate")));
    let before = requests.lock().unwrap().len();
    assert!(coordinator
        .submit(held(1, vec![key(4)]), 0)
        .unwrap_err()
        .contains("Injected"));
    assert_eq!(requests.lock().unwrap().len(), before);
    assert!(record.events.lock().unwrap().is_empty());
}

#[test]
fn revoked_replacement_releases_previous_contribution_and_reconciles_owner() {
    let (mut coordinator, record, _) = setup();
    coordinator.submit(held(1, vec![key(4)]), 0).unwrap();
    coordinator.set_dispatch_gate(Arc::new(|_| false));
    let ack = coordinator.submit(held(2, vec![key(5)]), 0).unwrap();
    assert!(ack.ok && ack.skipped);
    assert_eq!((ack.attempted, ack.confirmed), (1, 1));
    let events = record.events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_key_event(&events[0], 4, true);
    assert_key_event(&events[1], 4, false);
    assert!(coordinator.snapshot().held.is_empty());
    assert!(coordinator.snapshot().owners[0].controls.is_empty());
}

#[test]
fn cleanup_and_empty_held_bypass_gate_and_release() {
    for intent in [Intent::Cleanup, Intent::Held { controls: vec![] }] {
        let (mut coordinator, record, requests) = setup();
        coordinator.submit(held(1, vec![key(4)]), 0).unwrap();
        coordinator.set_dispatch_gate(Arc::new(|_| panic!("cleanup must bypass gate")));
        let ack = coordinator.submit(delivery("key", 2, intent), 0).unwrap();
        assert!(ack.ok && !ack.skipped);
        assert_eq!(ack.confirmed, 1);
        assert_eq!(
            requests.lock().unwrap().last().unwrap().deadline_us,
            Some(Counter(1100))
        );
        assert_key_event(record.events.lock().unwrap().last().unwrap(), 4, false);
        assert!(coordinator.snapshot().held.is_empty());
    }
}

#[test]
fn lifecycle_commands_ignore_denial() {
    let (mut coordinator, record, _) = setup();
    coordinator.submit(held(1, vec![key(4)]), 0).unwrap();
    coordinator.set_dispatch_gate(Arc::new(|_| panic!("lifecycle must bypass gate")));
    coordinator.renew(run()).unwrap();
    let other = RunToken {
        generation: Counter(2),
        ..run()
    };
    coordinator
        .activate(other.clone(), Scope::DesktopWide)
        .unwrap();
    coordinator.pause(other).unwrap();
    coordinator.stop_all().unwrap();
    assert_eq!(coordinator.snapshot().phase, "stopped");
    assert_key_event(record.events.lock().unwrap().last().unwrap(), 4, false);
}

#[test]
fn revoking_an_arm_after_evidence_validation_pauses_without_rearming() {
    let (mut coordinator, record, requests) = setup();
    coordinator.submit(held(1, vec![key(4)]), 0).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let gate_calls = calls.clone();
    coordinator.set_dispatch_gate(Arc::new(move |request| {
        assert!(matches!(request.command, Command::Arm { .. }));
        gate_calls.fetch_add(1, Ordering::SeqCst);
        false
    }));
    coordinator.arm(token("replacement"), "action".into()).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(requests.lock().unwrap().last().unwrap().deadline_us, Some(Counter(0)));
    assert_eq!(coordinator.snapshot().phase, "paused");
    assert!(coordinator.snapshot().held.is_empty());
    assert!(coordinator.snapshot().owners.is_empty());
    assert_key_event(record.events.lock().unwrap().last().unwrap(), 4, false);
    coordinator.set_dispatch_gate(Arc::new(|_| true));
    let value = delivery("replacement", 1, Intent::Held { controls: vec![key(5)] });
    assert!(coordinator.submit(value, 0).is_err());
    assert_eq!(record.events.lock().unwrap().len(), 2);
    coordinator.stop_all().unwrap();
}

#[test]
fn one_revoked_member_expires_entire_atomic_pointer_batch() {
    let (mut coordinator, record, requests) = setup();
    let batch = vec![
        delivery("horizontal", 1, Intent::Pointer { dx: 7, dy: 0 }),
        delivery("vertical", 1, Intent::Pointer { dx: 0, dy: -9 }),
    ];
    for value in &batch {
        coordinator
            .arm(value.producer.clone(), value.action_id.clone())
            .unwrap();
    }
    coordinator.set_dispatch_gate(Arc::new(|request| match &request.command {
        Command::DeliverPointerBatch { deliveries } => deliveries
            .iter()
            .all(|delivery| delivery.producer.node_id != "vertical"),
        _ => true,
    }));
    let acks = coordinator.submit_pointer_batch(batch.clone(), 0).unwrap();
    assert!(acks
        .iter()
        .all(|ack| ack.ok && ack.skipped && ack.confirmed == 0));
    assert!(record.events.lock().unwrap().is_empty());
    assert_eq!(
        requests.lock().unwrap().last().unwrap().deadline_us,
        Some(Counter(0))
    );
    assert!(coordinator.submit_pointer_batch(batch, 0).is_err());
}

#[test]
fn denial_cannot_turn_failed_native_release_into_success() {
    let (mut coordinator, record, _) = setup();
    coordinator.submit(held(1, vec![key(4)]), 0).unwrap();
    record.fail.store(true, Ordering::SeqCst);
    coordinator.set_dispatch_gate(Arc::new(|_| false));
    assert!(coordinator
        .submit(held(2, vec![key(5)]), 0)
        .unwrap_err()
        .contains("native release failed"));
    let snapshot = coordinator.snapshot();
    assert!(snapshot.uncertain && !snapshot.cleanup_complete);
    assert_eq!(snapshot.phase, "faulted");
    assert!(snapshot.in_flight.is_some());
    assert!(snapshot.possible_held.contains(&key(4)));
    assert!(snapshot.possible_held.contains(&key(5)));
}

#[test]
fn absent_or_allowing_gate_preserves_normal_delivery_and_single_pointer() {
    let (mut coordinator, record, _) = setup();
    coordinator.submit(held(1, vec![key(4)]), 0).unwrap();
    coordinator.set_dispatch_gate(Arc::new(|_| true));
    let ack = coordinator.submit(held(2, vec![key(5)]), 0).unwrap();
    assert!(ack.ok && !ack.skipped);
    assert_eq!(coordinator.snapshot().held, vec![key(5)]);
    coordinator.set_dispatch_gate(Arc::new(|_| false));
    let ack = coordinator
        .submit(delivery("key", 3, Intent::Pointer { dx: 3, dy: 9 }), 0)
        .unwrap();
    assert!(ack.ok && ack.skipped && ack.attempted == 0);
    assert!(record
        .events
        .lock()
        .unwrap()
        .iter()
        .all(|event| !matches!(event, NativeEvent::Pointer { .. })));
}

fn assert_journal_owned(path: &Path) {
    assert!(matches!(
        Coordinator::spawn_with_journal(false, false, path.to_path_buf()),
        Err(error) if error.contains("owns")
    ));
}

#[test]
fn retirement_before_terminal_stop_preserves_exact_journal_ownership() {
    let (mut coordinator, _, _) = setup();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.json");
    coordinator.journal = Some(path.clone());
    coordinator.persist().unwrap();
    assert!(coordinator.snapshot().cleanup_complete);
    assert!(coordinator.snapshot().in_flight.is_none());
    assert!(coordinator.retire_stopped_journal().is_err());
    assert_eq!(coordinator.journal.as_ref(), Some(&path));
    assert!(coordinator.journal_store.borrow().is_some());
    assert_journal_owned(&path);
    coordinator.submit(held(1, vec![key(4)]), 0).unwrap();
    coordinator.stop_all().unwrap();
}

#[test]
fn retirement_after_stop_releases_lock_while_original_coordinator_is_alive() {
    let (mut coordinator, record, _) = setup();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.json");
    coordinator.journal = Some(path.clone());
    coordinator.submit(held(1, vec![key(4)]), 0).unwrap();
    coordinator.stop_all().unwrap();
    assert_key_event(record.events.lock().unwrap().last().unwrap(), 4, false);
    let stopped = Coordinator::inspect_journal(&path).unwrap();
    assert!(stopped.cleanup_complete && stopped.in_flight.is_none());
    assert!(stopped.held.is_empty() && stopped.owners.is_empty());
    assert_journal_owned(&path);

    coordinator.retire_stopped_journal().unwrap();
    assert!(coordinator.journal.is_none());
    assert!(coordinator.journal_store.borrow().is_none());
    let (mut replacement, _, _) = setup();
    replacement.snapshot = stopped;
    replacement.journal = Some(path.clone());
    // persist() opens the same Store and acquires its OS lock successfully.
    replacement.persist().unwrap();
    assert!(replacement.journal_store.borrow().is_some());
    assert_journal_owned(&path);
    assert_eq!(coordinator.snapshot().phase, "stopped");
    assert!(coordinator.activate(run(), Scope::DesktopWide).is_err());
}

#[test]
fn failed_stop_cannot_retire_or_release_recovery_ownership() {
    let (mut coordinator, record, _) = setup();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.json");
    coordinator.journal = Some(path.clone());
    coordinator.submit(held(1, vec![key(4)]), 0).unwrap();
    record.fail.store(true, Ordering::SeqCst);
    assert!(coordinator.stop_all().is_err());
    let failed = coordinator.snapshot();
    assert!(failed.uncertain && !failed.cleanup_complete);
    assert!(failed.in_flight.is_some());
    assert!(coordinator.retire_stopped_journal().is_err());
    assert_eq!(coordinator.journal.as_ref(), Some(&path));
    assert!(coordinator.journal_store.borrow().is_some());
    assert_journal_owned(&path);
    let persisted = Coordinator::inspect_journal(&path).unwrap();
    assert!(persisted.uncertain && persisted.possible_held.contains(&key(4)));
}

#[test]
fn failed_final_stop_commit_cannot_become_success_on_a_repeated_stop() {
    let (mut coordinator, record, requests) = setup();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.json");
    coordinator.journal = Some(path.clone());
    coordinator.submit(held(1, vec![key(4)]), 0).unwrap();
    coordinator.fail_persist_at.set(Some(coordinator.persist_attempts.get() + 3));
    assert!(coordinator.stop_all().unwrap_err().contains("Injected"));
    assert_key_event(record.events.lock().unwrap().last().unwrap(), 4, false);
    assert!(coordinator.snapshot().cleanup_complete);
    let sent = requests.lock().unwrap().len();
    assert!(coordinator.stop_all().unwrap_err().contains("journal commit failed"));
    assert_eq!(requests.lock().unwrap().len(), sent, "A failed commit is never a replay request");
    assert!(coordinator.retire_stopped_journal().is_err());
    assert_journal_owned(&path);
}
