use super::*;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

#[derive(Clone, Copy, Default)]
enum GuardFault { #[default] None, Lost, Unknown, Duplicate, Mixed, Budget }
struct State {
    now: AtomicU64,
    expire_after_ack: AtomicBool,
    requests: AtomicUsize,
    fault: Mutex<GuardFault>,
    prepared: Mutex<Vec<(Request, Snapshot)>>,
    events: Mutex<Vec<Vec<NativeEvent>>>,
    replies: Mutex<Vec<Ack>>,
    native_after_ack: Mutex<Vec<Control>>,
}
impl State {
    fn new() -> Arc<Self> {
        Arc::new(Self { now: AtomicU64::new(clock()), expire_after_ack: AtomicBool::new(true),
            requests: AtomicUsize::new(0), fault: Mutex::new(GuardFault::None),
            prepared: Mutex::default(), events: Mutex::default(), replies: Mutex::default(), native_after_ack: Mutex::default() })
    }
}
struct Native(Arc<State>);
impl Adapter for Native {
    fn eligible(&mut self, _: &Scope) -> Result<bool, String> { Ok(true) }
    fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String> {
        self.0.events.lock().unwrap().push(events.to_vec());
        Ok(native_count(events))
    }
}
struct GuardChannel { worker: crate::worker::Worker<Native>, state: Arc<State>, path: PathBuf }
impl Channel for GuardChannel {
    fn request(&mut self, request: &Request) -> Result<Ack, String> {
        self.state.requests.fetch_add(1, Ordering::Relaxed);
        self.state.prepared.lock().unwrap().push((request.clone(), Coordinator::inspect_journal(&self.path)?));
        let now = self.state.now.load(Ordering::Relaxed);
        let mut ack = self.worker.receive(request.clone(), now);
        if matches!(request.command, Command::DeliverGuarded { .. }) && ack.ok && !ack.skipped
            && self.state.expire_after_ack.load(Ordering::Relaxed) {
            // Preserve the truthful down ACK, then release while the parent has
            // not even received it, let alone persisted its final owner view.
            let later = now + 1_000;
            self.worker.tick(later)?;
            self.state.now.store(later, Ordering::Relaxed);
            *self.state.native_after_ack.lock().unwrap() = self.worker.held();
        }
        match std::mem::take(&mut *self.state.fault.lock().unwrap()) {
            GuardFault::None => {}
            GuardFault::Lost => return Err("guard ACK lost after native submission".into()),
            GuardFault::Unknown => ack.released_guards = vec![Counter(u64::MAX)],
            GuardFault::Duplicate => ack.released_guards.push(ack.released_guards[0]),
            GuardFault::Mixed => ack.released_guards.push(Counter(u64::MAX)),
            GuardFault::Budget => ack.released_guards = vec![Counter(1); MAX_OWNERS + 1],
        }
        self.state.replies.lock().unwrap().push(ack.clone());
        Ok(ack)
    }
}
struct Harness { c: Coordinator, state: Arc<State>, path: PathBuf, _dir: tempfile::TempDir }
impl Harness {
    fn new() -> Self {
        let dir = tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target")).unwrap();
        let path = dir.path().join("journal.json");
        let state = State::new();
        let mut c = Coordinator::with_channel(Box::new(GuardChannel {
            worker: crate::worker::Worker::new(Native(state.clone())), state: state.clone(), path: path.clone(),
        }), clock);
        c.journal = Some(path.clone());
        c.activate(run(), Scope::DesktopWide).unwrap();
        c.arm(token(), "action".into()).unwrap();
        Self { c, state, path, _dir: dir }
    }
    fn arm(&mut self, delivery: &Delivery) {
        self.c.arm(delivery.producer.clone(), delivery.action_id.clone()).unwrap();
    }
    fn guarded(&mut self, down: Delivery) -> Ack { self.c.submit_guarded(down, 0, Counter(1_000)).unwrap() }
    fn saved(&self) -> Snapshot { Coordinator::inspect_journal(&self.path).unwrap() }
    fn assert_empty_projection(&self) {
        assert!(self.c.reducer.held().is_empty());
        assert!(self.c.snapshot.held.is_empty());
        assert!(self.c.snapshot.owners.iter().all(|owner| owner.controls.is_empty()));
        let saved = self.saved();
        assert!(saved.held.is_empty() && saved.owners.iter().all(|owner| owner.controls.is_empty()));
        assert_eq!(sql_read(&self.path).unwrap(), serde_json::to_vec(&self.c.snapshot()).unwrap());
    }
}
fn down() -> Delivery { let mut delivery = delivery(); delivery.deadline_us = None; delivery }
fn named(name: &str, operation: u64, intent: Intent) -> Delivery {
    let mut delivery = down();
    delivery.producer.node_id = name.into();
    delivery.action_id = name.into();
    delivery.operation_id = Counter(operation);
    delivery.sequence = Counter(operation);
    delivery.intent = intent;
    delivery
}
fn pointer(name: &str) -> Delivery { named(name, 1, Intent::Pointer { dx: 1, dy: 2 }) }
fn up() -> Delivery { let mut up = down(); up.operation_id = Counter(2); up.sequence = Counter(2); up.intent = Intent::Held { controls: vec![] }; up }

#[test]
fn guarded_down_keeps_durable_preacquisition_and_successful_ack_final_commit() {
    let mut h = Harness::new();
    let before = h.c.persist_attempts.get();
    let ack = h.guarded(down());
    assert!(ack.ok && ack.held == vec![key()] && ack.released_guards.is_empty());
    assert_eq!(h.c.persist_attempts.get() - before, 2);
    assert!(h.state.native_after_ack.lock().unwrap().is_empty());
    let seen = h.state.prepared.lock().unwrap();
    let (request, prepared) = seen.last().unwrap();
    assert!(matches!(request.command, Command::DeliverGuarded { .. }));
    assert!(matches!(prepared.in_flight.as_ref().unwrap().command, Command::DeliverGuarded { .. }));
    assert!(prepared.possible_held.contains(&key()));
    assert!(prepared.held.is_empty());
    drop(seen);
    let saved = h.saved();
    assert!(saved.in_flight.is_none() && saved.held == vec![key()] && saved.possible_held == vec![key()]);
    assert_eq!(saved.owners[0].controls, vec![key()]);
    assert!(h.c.guard_deliveries.contains_key(&ack.sequence.0));
}

#[test]
fn pointer_receipt_commits_owner_and_held_atomically_and_original_up_stays_valid() {
    let mut h = Harness::new();
    let pointer = pointer("pointer");
    h.arm(&pointer);
    let down = h.guarded(down());
    let before = h.c.persist_attempts.get();
    let ack = h.c.submit(pointer, 0).unwrap();
    assert_eq!(ack.released_guards, vec![down.sequence]);
    assert_eq!(h.c.persist_attempts.get() - before, 1);
    h.assert_empty_projection();
    assert!(h.c.guard_deliveries.is_empty());
    let events = h.state.events.lock().unwrap().len();
    let ack = h.c.submit(up(), 0).unwrap();
    assert!(ack.ok && ack.attempted == 0 && ack.released_guards.is_empty());
    assert_eq!(h.state.events.lock().unwrap().len(), events);
    h.assert_empty_projection();
}

#[test]
fn pointer_batch_arm_pause_and_renew_do_not_restore_pre_ack_guard_owners() {
    for next in 0..4 {
        let mut h = Harness::new();
        let first = pointer("x");
        let second = pointer("y");
        h.arm(&first);
        h.arm(&second);
        h.guarded(down());
        let before = h.c.persist_attempts.get();
        match next {
            0 => { let acks = h.c.submit_pointer_batch(vec![first, second], 0).unwrap(); assert_eq!(acks[0].released_guards.len(), 1); }
            1 => h.c.arm(token(), "fresh".into()).unwrap(),
            2 => h.c.pause(run()).unwrap(),
            _ => h.c.renew(run()).unwrap(),
        }
        assert!(h.c.persist_attempts.get() > before, "release metadata must become durable even on Renew");
        assert!(h.c.guard_deliveries.is_empty());
        h.assert_empty_projection();
    }
}

#[test]
fn shared_native_hold_survives_receipt_and_original_up_without_false_edges() {
    let mut h = Harness::new();
    let hold = named("hold", 1, Intent::Held { controls: vec![key()] });
    h.arm(&hold);
    h.c.submit(hold, 0).unwrap();
    let ack = h.guarded(down());
    assert!(ack.collision && ack.attempted == 0);
    assert_eq!(*h.state.native_after_ack.lock().unwrap(), vec![key()]);
    h.c.renew(run()).unwrap();
    let saved = h.saved();
    assert_eq!(saved.held, vec![key()]);
    assert_eq!(saved.shared_controls, 0);
    assert!(saved.owners.iter().find(|owner| owner.action_id == "action").unwrap().controls.is_empty());
    assert_eq!(saved.owners.iter().find(|owner| owner.action_id == "hold").unwrap().controls, vec![key()]);
    h.c.submit(up(), 0).unwrap();
    assert_eq!(h.state.events.lock().unwrap().len(), 1);
    let release = named("hold", 2, Intent::Held { controls: vec![] });
    h.c.submit(release, 0).unwrap();
    h.assert_empty_projection();
    assert_eq!(h.state.events.lock().unwrap().len(), 2);
}

#[test]
fn unknown_duplicate_or_mixed_receipts_fail_before_partial_projection() {
    for fault in [GuardFault::Unknown, GuardFault::Duplicate, GuardFault::Mixed] {
        let mut h = Harness::new();
        let pointer = pointer("pointer");
        h.arm(&pointer);
        let down = h.guarded(down());
        let owners = h.c.snapshot.owners.clone();
        *h.state.fault.lock().unwrap() = fault;
        assert!(h.c.submit(pointer, 0).unwrap_err().contains("receipt"));
        assert!(h.c.snapshot.uncertain && !h.c.snapshot.cleanup_complete);
        assert_eq!(h.c.reducer.held(), vec![key()]);
        assert_eq!(h.c.snapshot.owners, owners);
        assert!(h.c.guard_deliveries.contains_key(&down.sequence.0));
        assert!(h.saved().possible_held.contains(&key()));
    }
}

#[test]
fn lost_down_ack_and_failed_down_final_commit_retain_recoverable_possible_holds() {
    for lost in [true, false] {
        let mut h = Harness::new();
        if lost { *h.state.fault.lock().unwrap() = GuardFault::Lost; }
        else { h.c.fail_persist_at.set(Some(h.c.persist_attempts.get() + 2)); }
        assert!(h.c.submit_guarded(down(), 0, Counter(1_000)).is_err());
        assert!(h.c.snapshot.uncertain && !h.c.snapshot.cleanup_complete);
        assert!(h.state.native_after_ack.lock().unwrap().is_empty());
        let saved = h.saved();
        assert!(saved.possible_held.contains(&key()) && saved.in_flight.is_some());
        let bytes = sql_read(&h.path).unwrap();
        assert!(h.c.submit(up(), 0).unwrap_err().contains("uncertain"));
        assert_eq!(sql_read(&h.path).unwrap(), bytes);
        let path = h.path.clone();
        drop(h.c);
        let mut fresh = fixture(Fault::None);
        fresh.journal = Some(path.clone());
        fresh.snapshot = saved;
        fresh.recover(vec![key()]).unwrap();
        let recovered = Coordinator::inspect_journal(&path).unwrap();
        assert!(!recovered.uncertain && recovered.cleanup_complete && recovered.possible_held.is_empty());
    }
}

#[test]
fn malformed_guard_duration_or_empty_contribution_never_dispatches() {
    for duration in [0, 999, 60_000_001] {
        let mut h = Harness::new();
        let before = h.state.requests.load(Ordering::Relaxed);
        let writes = h.c.persist_attempts.get();
        assert!(h.c.submit_guarded(down(), 0, Counter(duration)).is_err());
        assert_eq!(h.state.requests.load(Ordering::Relaxed), before);
        assert_eq!(h.c.persist_attempts.get(), writes);
    }
    let mut h = Harness::new();
    let before = h.state.requests.load(Ordering::Relaxed);
    assert!(h.c.submit_guarded(up(), 0, Counter(1_000)).is_err());
    assert_eq!(h.state.requests.load(Ordering::Relaxed), before);
}

#[test]
fn failed_receipt_commit_faults_and_retains_prior_durable_holds() {
    let mut h = Harness::new();
    let pointer = pointer("pointer");
    h.arm(&pointer);
    h.guarded(down());
    let prior = sql_read(&h.path).unwrap();
    h.c.fail_persist_at.set(Some(h.c.persist_attempts.get() + 1));
    assert!(h.c.submit(pointer, 0).is_err());
    assert!(h.c.snapshot.uncertain && !h.c.snapshot.cleanup_complete);
    assert!(h.c.reducer.held().is_empty());
    assert_eq!(sql_read(&h.path).unwrap(), prior);
    assert_eq!(h.saved().possible_held, vec![key()]);
    assert_eq!(h.saved().held, vec![key()]);
    assert_eq!(h.saved().owners.iter().find(|owner| owner.action_id == "action").unwrap().controls, vec![key()]);
}

#[test]
fn new_guarded_down_reconciles_prior_expiry_before_registering_its_own_guard() {
    let mut h = Harness::new();
    let other_key = Control::Key { key: Key(5) };
    let second = named("second", 1, Intent::Held { controls: vec![other_key] });
    h.arm(&second);
    let first = h.guarded(down());
    let next = h.guarded(second);
    assert_eq!(next.released_guards, vec![first.sequence]);
    assert_eq!(next.held, vec![other_key]);
    assert!(!h.c.guard_deliveries.contains_key(&first.sequence.0));
    assert!(h.c.guard_deliveries.contains_key(&next.sequence.0));
    let saved = h.saved();
    assert_eq!(saved.held, vec![other_key]);
    assert!(saved.owners.iter().find(|owner| owner.action_id == "action").unwrap().controls.is_empty());
    assert_eq!(saved.owners.iter().find(|owner| owner.action_id == "second").unwrap().controls, vec![other_key]);
    h.c.renew(run()).unwrap();
    h.assert_empty_projection();
    assert!(h.c.guard_deliveries.is_empty());
}

#[test]
fn live_guards_retire_on_owner_advancement_and_lifecycle_without_later_phantom_releases() {
    for cancellation in 0..6 {
        let mut h = Harness::new();
        h.state.expire_after_ack.store(false, Ordering::Relaxed);
        let extra = pointer("extra");
        h.arm(&extra);
        let guarded = h.guarded(down());
        assert!(h.c.guard_deliveries.contains_key(&guarded.sequence.0));
        let mut motion = down();
        motion.operation_id = Counter(2);
        motion.sequence = Counter(2);
        motion.intent = Intent::Pointer { dx: 3, dy: 4 };
        match cancellation {
            0 => { h.c.submit(up(), 0).unwrap(); }
            1 => { h.c.submit(motion, 0).unwrap(); }
            2 => { h.c.submit_pointer_batch(vec![motion, extra], 0).unwrap(); }
            3 => { h.c.arm(token(), "action".into()).unwrap(); }
            4 => { h.c.pause(run()).unwrap(); }
            _ => { h.c.stop_all().unwrap(); }
        }
        assert!(h.c.guard_deliveries.is_empty());
        let events = h.state.events.lock().unwrap().len();
        h.state.now.store(clock() + 1_001, Ordering::Relaxed);
        if cancellation == 5 {
            h.c.stop_all().unwrap();
            assert!(h.c.snapshot.cleanup_complete && h.c.snapshot.held.is_empty());
        } else {
            h.c.renew(run()).unwrap();
            assert!(h.state.replies.lock().unwrap().last().unwrap().released_guards.is_empty());
        }
        assert_eq!(h.state.events.lock().unwrap().len(), events,
            "a retired guard must not manufacture a later native edge");
        if matches!(cancellation, 1 | 2) {
            assert_eq!(h.c.snapshot.held, vec![key()], "ordinary motion advances the owner without releasing its hold");
            h.c.stop_all().unwrap();
        }
        h.assert_empty_projection();
    }
}

#[test]
fn canceling_one_live_owner_preserves_the_other_owners_scheduled_release() {
    let mut h = Harness::new();
    h.state.expire_after_ack.store(false, Ordering::Relaxed);
    let other_key = Control::Key { key: Key(5) };
    let other = named("other", 1, Intent::Held { controls: vec![other_key] });
    h.arm(&other);
    let first = h.guarded(down());
    let second = h.guarded(other);
    h.c.submit(up(), 0).unwrap();
    assert!(!h.c.guard_deliveries.contains_key(&first.sequence.0));
    assert!(h.c.guard_deliveries.contains_key(&second.sequence.0));
    assert_eq!(h.c.snapshot.held, vec![other_key]);
    assert_eq!(h.state.events.lock().unwrap().len(), 3);
    h.state.now.store(clock() + 1_001, Ordering::Relaxed);
    h.c.renew(run()).unwrap();
    assert_eq!(h.state.replies.lock().unwrap().last().unwrap().released_guards, vec![second.sequence]);
    assert_eq!(h.state.events.lock().unwrap().len(), 4);
    assert!(h.c.guard_deliveries.is_empty());
    h.assert_empty_projection();
}

#[test]
fn oversized_receipt_list_faults_without_installing_partial_owner_changes() {
    let mut h = Harness::new();
    let pointer = pointer("pointer");
    h.arm(&pointer);
    let down = h.guarded(down());
    let owners = h.c.snapshot.owners.clone();
    *h.state.fault.lock().unwrap() = GuardFault::Budget;
    assert!(h.c.submit(pointer, 0).unwrap_err().contains("budget"));
    assert!(h.c.snapshot.uncertain && !h.c.snapshot.cleanup_complete);
    assert_eq!(h.c.reducer.held(), vec![key()]);
    assert_eq!(h.c.snapshot.owners, owners);
    assert!(h.c.guard_deliveries.contains_key(&down.sequence.0));
    assert!(h.saved().possible_held.contains(&key()));
}
