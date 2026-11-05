use super::*;

#[derive(Default)]
struct State {
    events: Vec<NativeEvent>,
    dispatched: Vec<Vec<u8>>,
    fault: Option<Fault>,
    extra_hold: bool,
}
struct ArmRecording(Arc<Mutex<State>>);
impl Adapter for ArmRecording {
    fn eligible(&mut self, _: &Scope) -> Result<bool, String> { Ok(true) }
    fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String> {
        self.0.lock().unwrap().events.extend_from_slice(events);
        Ok(native_count(events))
    }
}
struct ArmChannel {
    worker: crate::worker::Worker<ArmRecording>,
    state: Arc<Mutex<State>>,
    path: PathBuf,
}
impl Channel for ArmChannel {
    fn request(&mut self, request: &Request) -> Result<Ack, String> {
        let arm = matches!(request.command, Command::Arm { .. });
        if arm { self.state.lock().unwrap().dispatched.push(sql_read(&self.path)?); }
        let mut ack = self.worker.receive(request.clone(), clock());
        if arm {
            let mut state = self.state.lock().unwrap();
            if state.extra_hold { ack.held.push(Control::Key { key: Key(5) }); }
            match state.fault.take().unwrap_or(Fault::None) {
                Fault::None => {},
                Fault::Sequence => ack.sequence = Counter(999),
                Fault::Short => { ack.attempted = 2; ack.confirmed = 1; },
                Fault::Transport => return Err("Arm transport failure".into()),
                Fault::Rejected => { ack.ok = false; ack.error = Some("Arm rejected".into()); },
                Fault::Uncertain => { ack.ok = false; ack.uncertain = true; },
            }
        }
        Ok(ack)
    }
}
fn harness(held: bool) -> (tempfile::TempDir, Coordinator, Arc<Mutex<State>>) {
    let dir = tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target")).unwrap();
    let path = dir.path().join("journal.json");
    let state = Arc::new(Mutex::new(State::default()));
    let mut c = Coordinator::with_channel(Box::new(ArmChannel {
        worker: crate::worker::Worker::new(ArmRecording(state.clone())),
        state: state.clone(), path: path.clone(),
    }), clock);
    c.journal = Some(path);
    c.activate(run(), Scope::DesktopWide).unwrap();
    if held {
        c.arm(token(), "action".into()).unwrap();
        c.submit(delivery(), 0).unwrap();
    }
    (dir, c, state)
}
#[test]
fn arm_commits_once_after_dispatch_and_rearm_only_releases_prior_holds() {
    for held in [false, true] {
        let (_dir, mut c, state) = harness(held);
        let path = c.journal.clone().unwrap();
        let durable = sql_read(&path).unwrap();
        let before = c.persist_attempts.get();
        state.lock().unwrap().events.clear();
        c.arm(token(), "action".into()).unwrap();
        assert_eq!(c.persist_attempts.get() - before, 1);
        let state = state.lock().unwrap();
        assert_eq!(state.dispatched.last().unwrap(), &durable);
        assert_eq!(state.events.len(), usize::from(held));
        assert!(state.events.iter().all(|event| matches!(event, NativeEvent::Key { down: false, .. })));
        let saved = Coordinator::inspect_journal(&path).unwrap();
        assert!(saved.in_flight.is_none() && saved.held.is_empty() && saved.possible_held.is_empty());
        assert_eq!(saved.phase, "active");
        assert_eq!(saved.owners, vec![OwnerSnapshot {
            producer: token(), action_id: "action".into(), controls: vec![],
        }]);
        assert_eq!(sql_read(&path).unwrap(), serde_json::to_vec(&c.snapshot()).unwrap());
    }
}
#[test]
fn arm_preserves_other_owners_and_rearm_keeps_shared_control_held() {
    let (_dir, mut c, state) = harness(true);
    let other = Token { node_id: "other".into(), ..token() };
    c.arm(other.clone(), "other-action".into()).unwrap();
    assert_eq!(c.snapshot().held, vec![key()]);
    let mut value = delivery();
    value.producer = other.clone();
    value.action_id = "other-action".into();
    assert!(c.submit(value, 0).unwrap().collision);
    assert_eq!(c.snapshot().shared_controls, 1);
    let before = c.persist_attempts.get();
    state.lock().unwrap().events.clear();
    c.arm(token(), "action".into()).unwrap();
    assert_eq!(c.persist_attempts.get() - before, 1);
    assert!(state.lock().unwrap().events.is_empty());
    let saved = Coordinator::inspect_journal(c.journal.as_ref().unwrap()).unwrap();
    assert_eq!(saved.held, vec![key()]);
    assert_eq!(saved.possible_held, vec![key()]);
    assert_eq!(saved.shared_controls, 0);
    assert!(saved.owners.contains(&OwnerSnapshot {
        producer: other, action_id: "other-action".into(), controls: vec![key()],
    }));
    assert!(saved.owners.contains(&OwnerSnapshot {
        producer: token(), action_id: "action".into(), controls: vec![],
    }));
}
#[test]
fn arm_ack_cannot_acquire_an_unjournaled_control() {
    let (_dir, mut c, state) = harness(true);
    state.lock().unwrap().extra_hold = true;
    let before = c.persist_attempts.get();
    assert!(c.arm(token(), "action".into()).is_err());
    assert_eq!(c.persist_attempts.get() - before, 1);
    let saved = Coordinator::inspect_journal(c.journal.as_ref().unwrap()).unwrap();
    assert!(saved.uncertain && !saved.cleanup_complete);
    assert_eq!(saved.held, vec![key()]);
    assert_eq!(saved.possible_held, vec![key()]);
    let dispatched = state.lock().unwrap().dispatched.len();
    let mut next = delivery();
    next.operation_id = Counter(2);
    assert!(c.submit(next, 0).unwrap_err().contains("uncertain"));
    assert_eq!(state.lock().unwrap().dispatched.len(), dispatched);
}
#[test]
fn failed_arm_acknowledgements_still_persist_existing_hold_evidence() {
    for fault in [Fault::Sequence, Fault::Short, Fault::Transport, Fault::Uncertain, Fault::Rejected] {
        let (_dir, mut c, state) = harness(true);
        state.lock().unwrap().fault = Some(fault);
        let before = c.persist_attempts.get();
        assert!(c.arm(token(), "action".into()).is_err());
        assert_eq!(c.persist_attempts.get() - before, 1);
        let saved = Coordinator::inspect_journal(c.journal.as_ref().unwrap()).unwrap();
        assert!(saved.held.iter().chain(&saved.possible_held).any(|control| control == &key()));
        assert_eq!(saved.uncertain, !matches!(fault, Fault::Rejected));
    }
}
#[test]
fn failed_arm_final_commit_retains_old_hold_for_explicit_recovery() {
    let (_dir, mut c, state) = harness(true);
    let path = c.journal.clone().unwrap();
    let durable = sql_read(&path).unwrap();
    let before = c.persist_attempts.get();
    c.fail_persist_at.set(Some(before + 1));
    state.lock().unwrap().events.clear();
    assert!(c.arm(token(), "action".into()).unwrap_err().contains("Injected"));
    assert_eq!(c.persist_attempts.get() - before, 1);
    assert_eq!(sql_read(&path).unwrap(), durable);
    assert!(matches!(state.lock().unwrap().events.as_slice(), [NativeEvent::Key { down: false, .. }]));
    let saved = Coordinator::inspect_journal(&path).unwrap();
    assert_eq!(saved.held, vec![key()]);
    assert_eq!(saved.possible_held, vec![key()]);
    drop(c);
    let mut fresh = fixture(Fault::None);
    fresh.journal = Some(path.clone());
    fresh.snapshot = saved;
    fresh.recover(vec![key()]).unwrap();
    let recovered = Coordinator::inspect_journal(&path).unwrap();
    assert!(recovered.held.is_empty() && recovered.possible_held.is_empty());
    assert!(recovered.cleanup_complete && !recovered.uncertain);
}
