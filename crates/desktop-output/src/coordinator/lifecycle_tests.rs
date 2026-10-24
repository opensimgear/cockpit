use super::*;
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
    for intent in [Intent::Held { controls: vec![key()] }, Intent::Pointer { dx: 1, dy: 0 }] {
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
