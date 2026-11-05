use super::*;
use osg_mapping::desktop::Key;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct ReleaseState {
    events: Vec<NativeEvent>,
    dispatched: Vec<(Request, Vec<u8>)>,
    fault: Option<ReleaseFault>,
}

#[derive(Clone, Copy)]
enum ReleaseFault {
    Lost,
    Rejected,
    Uncertain,
    UnexpectedHold(Control),
}

struct ReleaseAdapter(Arc<Mutex<ReleaseState>>);
impl Adapter for ReleaseAdapter {
    fn eligible(&mut self, _: &Scope) -> Result<bool, String> {
        Ok(true)
    }
    fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String> {
        self.0.lock().unwrap().events.extend_from_slice(events);
        Ok(native_count(events))
    }
}

struct ReleaseChannel {
    worker: crate::worker::Worker<ReleaseAdapter>,
    state: Arc<Mutex<ReleaseState>>,
    path: PathBuf,
}
impl Channel for ReleaseChannel {
    fn request(&mut self, request: &Request) -> Result<Ack, String> {
        let is_delivery = matches!(request.command, Command::Deliver { .. });
        if is_delivery {
            let bytes = sql_read(&self.path)?;
            self.state
                .lock()
                .unwrap()
                .dispatched
                .push((request.clone(), bytes));
        }
        let mut ack = self.worker.receive(request.clone(), 1000);
        if is_delivery {
            let fault = self.state.lock().unwrap().fault.take();
            match fault {
                Some(ReleaseFault::Lost) => return Err("release acknowledgement lost".into()),
                Some(ReleaseFault::Rejected) => {
                    ack.ok = false;
                    ack.error = Some("release rejected".into());
                }
                Some(ReleaseFault::Uncertain) => {
                    ack.ok = false;
                    ack.uncertain = true;
                    ack.error = Some("release uncertain".into());
                }
                Some(ReleaseFault::UnexpectedHold(control)) => ack.held.push(control),
                None => {}
            }
        }
        Ok(ack)
    }
}

fn clock() -> u64 {
    1000
}
fn run() -> RunToken {
    RunToken {
        session: "release-journal-test".into(),
        generation: Counter(1),
    }
}
fn producer(node: &str) -> Token {
    Token {
        run: run(),
        node_id: node.into(),
        dependencies: vec![],
    }
}
fn key(usage: u16) -> Control {
    Control::Key { key: Key(usage) }
}
fn delivery(producer: Token, action_id: &str, operation: u64, intent: Intent) -> Delivery {
    Delivery {
        producer,
        action_id: action_id.into(),
        operation_id: Counter(operation),
        sequence: Counter(operation),
        at_us: Counter(0),
        deadline_us: Some(Counter(100)),
        intent,
    }
}
fn held(node: &str, action_id: &str, operation: u64, controls: Vec<Control>) -> Delivery {
    delivery(
        producer(node),
        action_id,
        operation,
        Intent::Held { controls },
    )
}
fn release(node: &str, action_id: &str, operation: u64) -> Delivery {
    held(node, action_id, operation, vec![])
}
fn sql_read(path: &std::path::Path) -> Result<Vec<u8>, String> {
    osg_persistence::test_support::read_evidence(path, "recovery-desktop")
}
fn sql_write(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    osg_persistence::test_support::write_evidence(path, "recovery-desktop", Some(bytes))
}
fn harness() -> (tempfile::TempDir, Coordinator, Arc<Mutex<ReleaseState>>) {
    let dir = tempfile::tempdir_in(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target"),
    )
    .unwrap();
    let path = dir.path().join("journal.json");
    let state = Arc::new(Mutex::new(ReleaseState::default()));
    let mut coordinator = Coordinator::with_channel(
        Box::new(ReleaseChannel {
            worker: crate::worker::Worker::new(ReleaseAdapter(state.clone())),
            state: state.clone(),
            path: path.clone(),
        }),
        clock,
    );
    coordinator.journal = Some(path);
    coordinator.activate(run(), Scope::DesktopWide).unwrap();
    (dir, coordinator, state)
}
fn arm_and_hold(coordinator: &mut Coordinator, node: &str, action_id: &str, controls: Vec<Control>) {
    let token = producer(node);
    coordinator.arm(token.clone(), action_id.into()).unwrap();
    coordinator
        .submit(held(node, action_id, 1, controls), 0)
        .unwrap();
}
fn recover_old_holds(path: &std::path::Path, snapshot: Snapshot, controls: Vec<Control>) {
    let mut fresh = Coordinator::with_channel(
        Box::new(ReleaseChannel {
            worker: crate::worker::Worker::new(ReleaseAdapter(Arc::default())),
            state: Arc::default(),
            path: path.into(),
        }),
        clock,
    );
    fresh.journal = Some(path.into());
    fresh.snapshot = snapshot;
    fresh.recover(controls).unwrap();
    let saved = Coordinator::inspect_journal(path).unwrap();
    assert!(saved.held.is_empty() && saved.possible_held.is_empty());
    assert!(saved.cleanup_complete && !saved.uncertain);
}

#[test]
fn release_dispatch_sees_old_durable_bytes_and_commits_once() {
    let (_dir, mut coordinator, state) = harness();
    arm_and_hold(&mut coordinator, "key", "action", vec![key(4)]);
    let path = coordinator.journal.clone().unwrap();
    let durable = sql_read(&path).unwrap();
    let before = coordinator.persist_attempts.get();

    coordinator.submit(release("key", "action", 2), 0).unwrap();

    assert_eq!(coordinator.persist_attempts.get() - before, 1);
    let state = state.lock().unwrap();
    let (request, bytes) = state.dispatched.last().unwrap();
    assert_eq!(bytes, &durable);
    assert!(matches!(request.command, Command::Deliver { .. }));
    let dispatched: Snapshot = serde_json::from_slice(bytes).unwrap();
    assert!(dispatched.in_flight.is_none());
    assert_eq!(dispatched.held, vec![key(4)]);
    assert_eq!(dispatched.possible_held, vec![key(4)]);
    drop(state);
    let saved = Coordinator::inspect_journal(&path).unwrap();
    assert!(saved.held.is_empty() && saved.possible_held.is_empty());
}

#[test]
fn subset_empty_and_cleanup_releases_each_keep_one_final_commit() {
    let (_dir, mut coordinator, _) = harness();
    arm_and_hold(&mut coordinator, "key", "action", vec![key(4)]);
    coordinator
        .submit(held("key", "action", 2, vec![key(4), key(5)]), 0)
        .unwrap();

    let before_subset = coordinator.persist_attempts.get();
    coordinator
        .submit(held("key", "action", 3, vec![key(4)]), 0)
        .unwrap();
    assert_eq!(coordinator.persist_attempts.get() - before_subset, 1);
    assert_eq!(coordinator.snapshot().held, vec![key(4)]);

    let before_empty = coordinator.persist_attempts.get();
    coordinator.submit(release("key", "action", 4), 0).unwrap();
    assert_eq!(coordinator.persist_attempts.get() - before_empty, 1);
    assert!(coordinator.snapshot().held.is_empty());

    let before_cleanup = coordinator.persist_attempts.get();
    coordinator
        .submit(
            delivery(producer("key"), "action", 5, Intent::Cleanup),
            0,
        )
        .unwrap();
    assert_eq!(coordinator.persist_attempts.get() - before_cleanup, 1);
}

#[test]
fn shared_control_release_does_not_acquire_and_commits_remaining_owner() {
    let (_dir, mut coordinator, state) = harness();
    arm_and_hold(
        &mut coordinator,
        "first",
        "first-action",
        vec![key(4), key(5)],
    );
    arm_and_hold(&mut coordinator, "second", "second-action", vec![key(4)]);
    assert_eq!(coordinator.snapshot().shared_controls, 1);
    let before = coordinator.persist_attempts.get();
    let events_before = state.lock().unwrap().events.len();

    let ack = coordinator
        .submit(held("first", "first-action", 2, vec![key(5)]), 0)
        .unwrap();

    assert!(ack.ok && !ack.uncertain);
    assert_eq!(coordinator.persist_attempts.get() - before, 1);
    assert_eq!(state.lock().unwrap().events.len(), events_before);
    assert_eq!(coordinator.snapshot().held, vec![key(4), key(5)]);
    assert_eq!(coordinator.snapshot().shared_controls, 0);
    assert!(coordinator.snapshot().owners.contains(&OwnerSnapshot {
        producer: producer("first"),
        action_id: "first-action".into(),
        controls: vec![key(5)],
    }));
    assert!(coordinator.snapshot().owners.contains(&OwnerSnapshot {
        producer: producer("second"),
        action_id: "second-action".into(),
        controls: vec![key(4)],
    }));
}

#[test]
fn acquisition_still_has_pre_dispatch_and_final_ack_commits() {
    let (_dir, mut coordinator, state) = harness();
    arm_and_hold(&mut coordinator, "key", "action", vec![key(4)]);
    let path = coordinator.journal.clone().unwrap();
    let before = coordinator.persist_attempts.get();

    coordinator
        .submit(held("key", "action", 2, vec![key(4), key(5)]), 0)
        .unwrap();

    assert_eq!(coordinator.persist_attempts.get() - before, 2);
    let state = state.lock().unwrap();
    let (_, bytes) = state.dispatched.last().unwrap();
    let prepared: Snapshot = serde_json::from_slice(bytes).unwrap();
    assert!(prepared.in_flight.is_some());
    assert!(prepared.possible_held.contains(&key(4)));
    assert!(prepared.possible_held.contains(&key(5)));
    assert!(Coordinator::inspect_journal(&path)
        .unwrap()
        .held
        .contains(&key(5)));
}

#[test]
fn release_ack_failures_keep_old_controls_recoverable() {
    for fault in [
        ReleaseFault::Lost,
        ReleaseFault::Rejected,
        ReleaseFault::Uncertain,
        ReleaseFault::UnexpectedHold(key(5)),
    ] {
        let (_dir, mut coordinator, state) = harness();
        arm_and_hold(&mut coordinator, "key", "action", vec![key(4)]);
        let path = coordinator.journal.clone().unwrap();
        state.lock().unwrap().fault = Some(fault);
        let before = coordinator.persist_attempts.get();

        assert!(coordinator
            .submit(release("key", "action", 2), 0)
            .is_err());

        assert_eq!(coordinator.persist_attempts.get() - before, 1);
        let durable = Coordinator::inspect_journal(&path).unwrap();
        assert!(durable.held.contains(&key(4)) || durable.possible_held.contains(&key(4)));
        assert!(durable.possible_held.contains(&key(4)));
        assert!(durable.in_flight.is_none());
        drop(coordinator);
        recover_old_holds(&path, durable, vec![key(4)]);
    }
}

#[test]
fn failed_release_final_commit_leaves_old_evidence_for_recovery() {
    let (_dir, mut coordinator, state) = harness();
    arm_and_hold(&mut coordinator, "key", "action", vec![key(4)]);
    let path = coordinator.journal.clone().unwrap();
    let durable = sql_read(&path).unwrap();
    let before = coordinator.persist_attempts.get();
    coordinator.fail_persist_at.set(Some(before + 1));

    assert!(coordinator
        .submit(release("key", "action", 2), 0)
        .unwrap_err()
        .contains("Injected"));

    assert_eq!(coordinator.persist_attempts.get() - before, 1);
    assert_eq!(sql_read(&path).unwrap(), durable);
    assert_eq!(state.lock().unwrap().events.last(), Some(&NativeEvent::Key {
        usage: 4,
        down: false,
        repeat: false,
    }));
    let saved = Coordinator::inspect_journal(&path).unwrap();
    assert_eq!(saved.held, vec![key(4)]);
    assert_eq!(saved.possible_held, vec![key(4)]);
    drop(coordinator);
    recover_old_holds(&path, saved, vec![key(4)]);
}

#[test]
fn stale_evidence_blocks_release_dispatch() {
    let (_dir, mut coordinator, state) = harness();
    arm_and_hold(&mut coordinator, "key", "action", vec![key(4)]);
    let path = coordinator.journal.clone().unwrap();
    let mut external = coordinator.snapshot();
    external.phase = "externally changed".into();
    sql_write(&path, &serde_json::to_vec(&external).unwrap()).unwrap();
    let dispatches = state.lock().unwrap().dispatched.len();

    assert!(coordinator
        .submit(release("key", "action", 2), 0)
        .unwrap_err()
        .contains("changed externally"));

    assert_eq!(state.lock().unwrap().dispatched.len(), dispatches);
}
