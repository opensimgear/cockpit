use super::*;
#[test]
fn successful_pointer_updates_leave_recovery_evidence_unchanged() {
    let dir = tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target")).unwrap();
    let path = dir.path().join("journal.json");
    let mut c = fixture(Fault::None);
    c.journal = Some(path.clone());
    c.activate(run(), Scope::DesktopWide).unwrap();
    c.arm(token(), "action".into()).unwrap();
    let before = c.persist_attempts.get();
    let durable = sql_read(&path).unwrap();
    for operation in 1..=32 {
        let mut value = delivery();
        value.operation_id = Counter(operation);
        value.intent = Intent::Pointer { dx: 1, dy: -1 };
        c.submit(value.clone(), 0).unwrap();
        assert!(c.submit(value, 0).unwrap_err().contains("Stale"));
    }
    assert_eq!(c.persist_attempts.get(), before, "motion cannot change held-control recovery evidence");
    assert_eq!(sql_read(&path).unwrap(), durable);
}
#[test]
fn held_key_recovery_evidence_survives_pointer_updates() {
    let dir = tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target")).unwrap();
    let path = dir.path().join("journal.json");
    let mut c = fixture(Fault::None);
    c.journal = Some(path.clone());
    c.activate(run(), Scope::DesktopWide).unwrap();
    c.arm(token(), "action".into()).unwrap();
    c.submit(delivery(), 0).unwrap();
    let durable = sql_read(&path).unwrap();
    let before = c.persist_attempts.get();
    for operation in 2..=16 {
        let mut value = delivery();
        value.operation_id = Counter(operation);
        value.intent = Intent::Pointer { dx: 2, dy: 0 };
        c.submit(value, 0).unwrap();
    }
    assert_eq!(c.persist_attempts.get(), before);
    assert_eq!(sql_read(&path).unwrap(), durable);
    let retained = Coordinator::inspect_journal(&path).unwrap();
    assert_eq!(retained.held, vec![key()]);
    assert_eq!(retained.possible_held, vec![key()]);
    drop(c);
    let mut recovery = fixture(Fault::None);
    recovery.journal = Some(path.clone());
    recovery.snapshot = retained;
    recovery.recover(vec![key()]).unwrap();
    let recovered = Coordinator::inspect_journal(&path).unwrap();
    assert!(recovered.held.is_empty() && recovered.possible_held.is_empty());
}
#[test]
fn pointer_traffic_does_not_wait_on_a_locked_sqlite_writer_but_hold_changes_do() {
    let dir = tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target")).unwrap();
    let path = dir.path().join("journal.json");
    let events = Arc::new(Mutex::new(vec![]));
    let mut c = Coordinator::with_channel(Box::new(PointerChannel {
        worker: crate::worker::Worker::new(PointerRecording(events.clone())),
        fault: Arc::new(Mutex::new(PointerFault::None)),
    }), clock);
    c.journal = Some(path.clone());
    c.activate(run(), Scope::DesktopWide).unwrap();
    c.arm(token(), "action".into()).unwrap();
    c.submit(delivery(), 0).unwrap();
    let database = osg_persistence::Database::for_namespace(&path).unwrap();
    let lock = osg_persistence::test_support::hold_writer(&database).unwrap();
    let before = c.persist_attempts.get();
    for operation in 2..=128 {
        let mut value = delivery();
        value.operation_id = Counter(operation);
        value.intent = Intent::Pointer { dx: 1, dy: 0 };
        c.submit(value, 0).unwrap();
    }
    assert_eq!(c.persist_attempts.get(), before);
    let dispatched = events.lock().unwrap().len();
    let mut release = delivery();
    release.operation_id = Counter(129);
    release.intent = Intent::Held { controls: vec![] };
    let started = std::time::Instant::now();
    assert!(c.submit(release, 0).is_err(), "hold changes still require a durable pre-dispatch ledger");
    assert!(started.elapsed() >= std::time::Duration::from_millis(200));
    assert_eq!(events.lock().unwrap().len(), dispatched);
    drop(lock);
    let retained = Coordinator::inspect_journal(&path).unwrap();
    assert_eq!(retained.held, vec![key()]);
    assert_eq!(retained.possible_held, vec![key()]);
}

#[derive(Clone, Copy)]
enum PointerFault { None, Transport, Rejected, MissingHold, ExtraHold, Operation, Uncertain }
struct PointerChannel {
    worker: crate::worker::Worker<PointerRecording>,
    fault: Arc<Mutex<PointerFault>>,
}
struct PointerRecording(Arc<Mutex<Vec<NativeEvent>>>);
impl Adapter for PointerRecording {
    fn eligible(&mut self, _: &Scope) -> Result<bool, String> { Ok(true) }
    fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String> {
        self.0.lock().unwrap().extend_from_slice(events);
        Ok(native_count(events))
    }
}
impl Channel for PointerChannel {
    fn request(&mut self, request: &Request) -> Result<Ack, String> {
        let mut ack = self.worker.receive(request.clone(), clock());
        match std::mem::replace(&mut *self.fault.lock().unwrap(), PointerFault::None) {
            PointerFault::None => {},
            PointerFault::Transport => return Err("pointer transport failure".into()),
            PointerFault::Rejected => { ack.ok = false; ack.error = Some("pointer rejected".into()); },
            PointerFault::MissingHold => ack.held.clear(),
            PointerFault::ExtraHold => ack.held.push(Control::Key { key: Key(5) }),
            PointerFault::Operation => ack.operation_id = Some(Counter(999)),
            PointerFault::Uncertain => { ack.ok = false; ack.uncertain = true; },
        }
        Ok(ack)
    }
}
#[test]
fn failed_pointer_acknowledgements_preserve_holds_and_block_replay() {
    for fault in [PointerFault::Transport, PointerFault::MissingHold,
        PointerFault::ExtraHold, PointerFault::Operation, PointerFault::Uncertain] {
        let dir = tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target")).unwrap();
        let path = dir.path().join("journal.json");
        let fault_slot = Arc::new(Mutex::new(PointerFault::None));
        let events = Arc::new(Mutex::new(vec![]));
        let mut c = Coordinator::with_channel(Box::new(PointerChannel {
            worker: crate::worker::Worker::new(PointerRecording(events.clone())),
            fault: fault_slot.clone(),
        }), clock);
        c.journal = Some(path.clone());
        c.activate(run(), Scope::DesktopWide).unwrap();
        c.arm(token(), "action".into()).unwrap();
        c.submit(delivery(), 0).unwrap();
        *fault_slot.lock().unwrap() = fault;
        let before = c.persist_attempts.get();
        let mut value = delivery();
        value.operation_id = Counter(2);
        value.intent = Intent::Pointer { dx: 1, dy: 0 };
        assert!(c.submit(value.clone(), 0).is_err());
        assert_eq!(c.persist_attempts.get(), before + 1);
        let retained = Coordinator::inspect_journal(&path).unwrap();
        assert!(retained.uncertain);
        assert_eq!(retained.held, vec![key()]);
        assert_eq!(retained.possible_held, vec![key()]);
        let durable = sql_read(&path).unwrap();
        let submitted = events.lock().unwrap().len();
        value.operation_id = Counter(3);
        assert!(c.submit(value, 0).unwrap_err().contains("uncertain"));
        assert_eq!(events.lock().unwrap().len(), submitted);
        assert_eq!(sql_read(&path).unwrap(), durable);
        let motions = events.lock().unwrap().iter().filter(|event| matches!(event, NativeEvent::Pointer { .. })).count();
        drop(c);
        let mut recovery = Coordinator::with_channel(Box::new(PointerChannel {
            worker: crate::worker::Worker::new(PointerRecording(events.clone())),
            fault: fault_slot.clone(),
        }), clock);
        recovery.journal = Some(path.clone());
        recovery.snapshot = retained;
        recovery.recover(vec![key()]).unwrap();
        assert_eq!(events.lock().unwrap().iter().filter(|event| matches!(event, NativeEvent::Pointer { .. })).count(), motions);
        assert!(Coordinator::inspect_journal(&path).unwrap().held.is_empty());
    }
}
#[test]
fn definitive_pointer_rejection_persists_paused_holds_without_replaying_motion() {
    let dir = tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target")).unwrap();
    let path = dir.path().join("journal.json");
    let fault = Arc::new(Mutex::new(PointerFault::None));
    let events = Arc::new(Mutex::new(vec![]));
    let mut c = Coordinator::with_channel(Box::new(PointerChannel {
        worker: crate::worker::Worker::new(PointerRecording(events.clone())), fault: fault.clone(),
    }), clock);
    c.journal = Some(path.clone());
    c.activate(run(), Scope::DesktopWide).unwrap();
    c.arm(token(), "action".into()).unwrap();
    c.submit(delivery(), 0).unwrap();
    *fault.lock().unwrap() = PointerFault::Rejected;
    let before = c.persist_attempts.get();
    let mut value = delivery();
    value.operation_id = Counter(2);
    value.intent = Intent::Pointer { dx: 1, dy: 0 };
    assert!(c.submit(value, 0).unwrap_err().contains("pointer rejected"));
    assert_eq!(c.persist_attempts.get(), before + 1);
    let retained = Coordinator::inspect_journal(&path).unwrap();
    assert_eq!(retained.phase, "paused");
    assert!(!retained.uncertain);
    assert_eq!(retained.held, vec![key()]);
    assert_eq!(retained.possible_held, vec![key()]);
    assert_eq!(events.lock().unwrap().iter().filter(|event| matches!(event, NativeEvent::Pointer { .. })).count(), 1);
}
#[test]
fn expired_pointer_motion_never_reaches_the_adapter() {
    let events = Arc::new(Mutex::new(vec![]));
    let mut c = Coordinator::with_channel(Box::new(PointerChannel {
        worker: crate::worker::Worker::new(PointerRecording(events.clone())),
        fault: Arc::new(Mutex::new(PointerFault::None)),
    }), clock);
    c.activate(run(), Scope::DesktopWide).unwrap();
    c.arm(token(), "action".into()).unwrap();
    let mut value = delivery();
    value.intent = Intent::Pointer { dx: 1, dy: 0 };
    assert!(c.submit(value, 100).unwrap().skipped);
    assert!(events.lock().unwrap().is_empty());
}
#[test]
fn lifecycle_final_writes_remain_even_when_owners_are_unchanged() {
    let dir = tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target")).unwrap();
    let mut c = fixture(Fault::None);
    c.journal = Some(dir.path().join("journal.json"));
    let operations: [fn(&mut Coordinator) -> Result<(), String>; 6] = [
        |c| c.activate(run(), Scope::DesktopWide),
        |c| c.arm(token(), "action".into()),
        |c| c.arm(token(), "action".into()),
        |c| c.pause(run()),
        |c| c.pause(run()),
        |c| c.stop_all(),
    ];
    for operation in operations {
        let before = c.persist_attempts.get();
        operation(&mut c).unwrap();
        assert_eq!(c.persist_attempts.get() - before, 3);
        assert_eq!(sql_read(c.journal.as_ref().unwrap()).unwrap(), serde_json::to_vec(&c.snapshot()).unwrap());
    }
    let mut recovery = fixture(Fault::None);
    recovery.journal = c.journal.clone();
    recovery.snapshot = c.snapshot();
    drop(c);
    recovery.recover(vec![]).unwrap();
    assert_eq!(recovery.persist_attempts.get(), 3);
    assert_eq!(sql_read(recovery.journal.as_ref().unwrap()).unwrap(), serde_json::to_vec(&recovery.snapshot()).unwrap());
}

#[test]
fn post_valid_ack_write_failure_preserves_pre_dispatch_journal_and_requires_recovery() {
    for intent in [Intent::Held { controls: vec![key()] }] {
        let dir = tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target")).unwrap();
        let path = dir.path().join("journal.json");
        let mut c = fixture(Fault::None);
        c.journal = Some(path.clone());
        c.activate(run(), Scope::DesktopWide).unwrap();
        c.arm(token(), "action".into()).unwrap();
        let before = c.persist_attempts.get();
        c.fail_persist_at.set(Some(before + 2));
        let mut value = delivery();
        value.intent = intent;
        assert!(c.submit(value.clone(), 0).unwrap_err().contains("Injected"));
        assert_eq!(c.persist_attempts.get() - before, 2);
        assert!(c.snapshot().uncertain && !c.snapshot().cleanup_complete);
        let recorded = Coordinator::inspect_journal(&path).unwrap();
        let pending = recorded.in_flight.as_ref().unwrap();
        assert!(matches!(&pending.command, Command::Deliver { delivery } if delivery == &value));
        assert!(recorded.held.is_empty());
        let controls = if matches!(value.intent, Intent::Held { .. }) { vec![key()] } else { vec![] };
        assert_eq!(recorded.possible_held, controls);
        let durable = sql_read(&path).unwrap();
        value.operation_id = Counter(2);
        assert!(c.submit(value, 0).unwrap_err().contains("uncertain"));
        assert_eq!(c.persist_attempts.get() - before, 2);
        assert_eq!(sql_read(&path).unwrap(), durable);
        drop(c);

        let mut fresh = fixture(Fault::None);
        fresh.snapshot = recorded;
        fresh.journal = Some(path.clone());
        fresh.recover(controls).unwrap();
        fresh.stop_all().unwrap();
        let recovered = Coordinator::inspect_journal(&path).unwrap();
        assert!(!recovered.uncertain && recovered.cleanup_complete && recovered.in_flight.is_none());
        assert!(recovered.held.is_empty() && recovered.possible_held.is_empty() && recovered.owners.is_empty() && recovered.runs.is_empty());
    }
}
#[test]
fn sequence_exhaustion_and_invalid_owner_are_refused_before_dispatch() {
    let mut c = fixture(Fault::None);
    c.sequence = u64::MAX;
    assert!(c.activate(run(), Scope::DesktopWide).is_err());
    let mut c = fixture(Fault::None);
    assert!(c.arm(token(), String::new()).is_err());
    assert!(c.submit(delivery(), 0).is_err());
}
#[test]
fn worker_clock_expiry_cancels_held_action_and_parent_ownership() {
    struct Delayed {
        worker: crate::worker::Worker<Recording>,
    }
    impl Channel for Delayed {
        fn request(&mut self, r: &Request) -> Result<Ack, String> {
            Ok(self.worker.receive(
                r.clone(),
                if matches!(r.command, Command::Deliver { .. }) {
                    1101
                } else {
                    1000
                },
            ))
        }
    }
    let mut c = Coordinator::with_channel(
        Box::new(Delayed {
            worker: crate::worker::Worker::new(Recording),
        }),
        clock,
    );
    c.activate(run(), Scope::DesktopWide).unwrap();
    c.arm(token(), "action".into()).unwrap();
    let ack = c.submit(delivery(), 0).unwrap();
    assert!(ack.ok && ack.skipped && ack.held.is_empty());
    assert_eq!(ack.attempted, 0);
    assert!(c.snapshot().owners[0].controls.is_empty());
}
#[test]
fn acknowledged_stop_is_idempotent_and_cannot_reuse_a_dead_worker() {
    struct OneStop {
        stops: Arc<Mutex<usize>>,
    }
    impl Channel for OneStop {
        fn request(&mut self, r: &Request) -> Result<Ack, String> {
            assert!(matches!(r.command, Command::Stop));
            let mut count = self.stops.lock().unwrap();
            *count += 1;
            assert_eq!(
                *count, 1,
                "second Stop would reach a worker which already exited"
            );
            Ok(Ack {
                sequence: r.sequence,
                operation_id: None,
                pointer_batch: None,
                ok: true,
                attempted: 0,
                confirmed: 0,
                uncertain: false,
                cleanup_complete: true,
                skipped: false,
                collision: false,
                error: None,
                held: vec![],
            })
        }
    }
    let stops = Arc::new(Mutex::new(0));
    let mut c = Coordinator::with_channel(
        Box::new(OneStop {
            stops: stops.clone(),
        }),
        clock,
    );
    c.stop_all().unwrap();
    let before = c.snapshot();
    c.stop_all().unwrap();
    assert_eq!(*stops.lock().unwrap(), 1);
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(c.snapshot()).unwrap()
    );
    assert!(c.activate(run(), Scope::DesktopWide).is_err());
    assert!(c.recover(vec![key()]).is_err());
}
#[test]
fn owned_journal_refuses_other_run_scope_and_missing_or_corrupt_records() {
    let directory =
        tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
            .unwrap();
    let path = directory.path().join("journal.json");
    let expected = run();
    assert!(Coordinator::inspect_owned_journal(&path, &expected, None)
        .unwrap_err()
        .contains("missing"));
    sql_write(&path, b"{broken").unwrap();
    assert!(Coordinator::inspect_owned_journal(&path, &expected, None).is_err());
    osg_persistence::test_support::write_evidence(&path,"recovery-desktop",None).unwrap();
    let mut c = fixture(Fault::None);
    c.journal = Some(path.clone());
    c.snapshot.journal_owner=Some(expected.clone());c.snapshot.journal_scope=Some(Scope::DesktopWide);
    c.activate(expected.clone(), Scope::DesktopWide).unwrap();
    c.arm(token(), "action".into()).unwrap();
    c.submit(delivery(), 0).unwrap();
    assert!(
        Coordinator::inspect_owned_journal(&path, &expected, Some(&Scope::DesktopWide)).is_ok()
    );
    let mut other = expected.clone();
    other.generation = Counter(2);
    assert!(Coordinator::inspect_owned_journal(&path, &other, None)
        .unwrap_err()
        .contains("ProfileSession run"));
    let application = Scope::Application {
        identity: ProcessIdentity {
            executable: "target.exe".into(),
            pid: 1,
            created: "1".into(),
        },
    };
    assert!(
        Coordinator::inspect_owned_journal(&path, &expected, Some(&application))
            .unwrap_err()
            .contains("scope")
    );
    c.stop_all().unwrap();
    assert!(
        Coordinator::inspect_owned_journal(&path, &expected, Some(&Scope::DesktopWide)).is_ok()
    );
    let mut record = c.snapshot();
    record.in_flight = Some(Request {
        sequence: Counter(1),
        deadline_us: None,
        command: Command::Arm {
            producer: Token {
                run: other.clone(),
                ..token()
            },
            action_id: "action".into(),
        },
    });
    sql_write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    assert!(Coordinator::inspect_owned_journal(&path, &expected, None).is_err());
    record.in_flight = Some(Request {
        sequence: Counter(2),
        deadline_us: None,
        command: Command::Activate {
            run: expected.clone(),
            scope: application,
        },
    });
    sql_write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    assert!(
        Coordinator::inspect_owned_journal(&path, &expected, Some(&Scope::DesktopWide)).is_err()
    );
}
#[test]
fn journal_size_limit_is_enforced_before_native_dispatch() {
    struct Never;
    impl Channel for Never {
        fn request(&mut self, _: &Request) -> Result<Ack, String> {
            panic!("oversized ledger must fail before native dispatch")
        }
    }
    let dir = tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
        .unwrap();
    let mut c = Coordinator::with_channel(Box::new(Never), clock);
    c.journal = Some(dir.path().join("journal.json"));
    c.snapshot.error = Some("x".repeat(MAX_JOURNAL_BYTES));
    assert!(c
        .activate(run(), Scope::DesktopWide)
        .unwrap_err()
        .contains("128 KiB"));
    assert!(!c.journal.as_ref().unwrap().exists());
}
#[test]fn deferred_scope_binds_once_before_activate_and_never_changes_owner(){
    let mut c=fixture(Fault::None);c.snapshot.journal_owner=Some(run());c.bind_scope_before_activate(&run(),&Scope::DesktopWide).unwrap();assert_eq!(c.snapshot().journal_scope,Some(Scope::DesktopWide));
    let application=Scope::Application{identity:ProcessIdentity{executable:"target.exe".into(),pid:1,created:"1".into()}};
    assert!(c.bind_scope_before_activate(&run(),&application).is_err());let mut other=run();other.generation=Counter(2);assert!(c.bind_scope_before_activate(&other,&Scope::DesktopWide).is_err());assert!(c.activate(other,Scope::DesktopWide).is_err());assert!(c.activate(run(),application).is_err());
    c.activate(run(),Scope::DesktopWide).unwrap();assert!(c.bind_scope_before_activate(&run(),&Scope::DesktopWide).is_err());
}
