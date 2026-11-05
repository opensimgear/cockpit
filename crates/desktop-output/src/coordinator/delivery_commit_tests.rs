use super::*;

#[derive(Clone, Copy)]
enum AckFault {
    Sequence,
    Operation,
    Short,
    Held,
    Rejected,
    Uncertain,
    Transport,
}
struct DeliveryFault {
    worker: crate::worker::Worker<Recording>,
    fault: AckFault,
}
impl Channel for DeliveryFault {
    fn request(&mut self, request: &Request) -> Result<Ack, String> {
        let mut ack = self.worker.receive(request.clone(), clock());
        if matches!(request.command, Command::Deliver { .. }) {
            match self.fault {
                AckFault::Sequence => ack.sequence = Counter(999),
                AckFault::Operation => ack.operation_id = Some(Counter(999)),
                AckFault::Short => {
                    ack.attempted = 2;
                    ack.confirmed = 1;
                }
                AckFault::Held => ack.held.push(Control::Key { key: Key(0xffff) }),
                AckFault::Rejected => {
                    ack.ok = false;
                    ack.error = Some("rejected delivery".into());
                }
                AckFault::Uncertain => {
                    ack.uncertain = true;
                }
                AckFault::Transport => return Err("lost delivery acknowledgement".into()),
            }
        }
        Ok(ack)
    }
}

#[test]
fn delivery_commit_callback_never_runs_for_invalid_rejected_or_uncertain_ack() {
    for fault in [
        AckFault::Sequence,
        AckFault::Operation,
        AckFault::Short,
        AckFault::Held,
        AckFault::Rejected,
        AckFault::Uncertain,
        AckFault::Transport,
    ] {
        let mut c = Coordinator::with_channel(
            Box::new(DeliveryFault {
                worker: crate::worker::Worker::new(Recording),
                fault,
            }),
            clock,
        );
        c.activate(run(), Scope::DesktopWide).unwrap();
        c.arm(token(), "action".into()).unwrap();
        let called = std::cell::Cell::new(false);
        assert!(c
            .send_with_commit(
                Command::Deliver {
                    delivery: delivery()
                },
                Some(Counter(1100)),
                vec![key()],
                |_, _| {
                    called.set(true);
                    Ok(true)
                },
            )
            .is_err());
        assert!(
            !called.get(),
            "only a fully validated successful acknowledgement may commit metadata"
        );
        assert!(c.snapshot().owners[0].controls.is_empty());
    }
}

#[test]
fn successful_ack_commits_metadata_even_without_a_held_ledger_change() {
    let dir = tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
        .unwrap();
    let path = dir.path().join("journal.json");
    let mut c = fixture(Fault::None);
    c.journal = Some(path.clone());
    c.activate(run(), Scope::DesktopWide).unwrap();
    let before = c.persist_attempts.get();
    let called = std::cell::Cell::new(0);
    c.send_with_commit(Command::Renew { run: run() }, None, vec![], |c, ack| {
        called.set(called.get() + 1);
        assert!(ack.ok && !ack.uncertain);
        c.snapshot.phase = "metadata committed".into();
        Ok(true)
    })
    .unwrap();
    assert_eq!(called.get(), 1);
    assert_eq!(c.persist_attempts.get() - before, 1);
    assert_eq!(
        Coordinator::inspect_journal(&path).unwrap().phase,
        "metadata committed"
    );
    assert_eq!(
        sql_read(&path).unwrap(),
        serde_json::to_vec(&c.snapshot()).unwrap()
    );
}

#[test]
fn failed_metadata_commit_fences_acknowledged_delivery_until_recovery() {
    for fail_fault_write in [false, true] {
        let dir =
            tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
                .unwrap();
        let path = dir.path().join("journal.json");
        let mut c = fixture(Fault::None);
        c.journal = Some(path.clone());
        c.activate(run(), Scope::DesktopWide).unwrap();
        c.arm(token(), "action".into()).unwrap();
        let before = c.persist_attempts.get();
        if fail_fault_write {
            c.fail_persist_at.set(Some(before + 2));
        }
        let called = std::cell::Cell::new(0);
        let error = c
            .send_with_commit(
                Command::Deliver {
                    delivery: delivery(),
                },
                Some(Counter(1100)),
                vec![key()],
                |_, ack| {
                    called.set(called.get() + 1);
                    assert!(ack.ok && !ack.uncertain && ack.held == vec![key()]);
                    Err("metadata projection failed".into())
                },
            )
            .unwrap_err();
        assert!(error.contains("metadata projection failed"));
        assert_eq!(called.get(), 1);
        assert!(c.snapshot().uncertain && !c.snapshot().cleanup_complete);
        let durable = Coordinator::inspect_journal(&path).unwrap();
        if fail_fault_write {
            assert!(durable.in_flight.is_some());
            assert!(durable.held.is_empty());
        } else {
            assert!(durable.in_flight.is_some(), "A failed projection must retain the prepared recovery request");
            assert!(durable.uncertain && !durable.cleanup_complete);
            assert_eq!(durable.held, vec![key()]);
            assert_eq!(durable.phase, "faulted");
            assert_eq!(durable.error.as_deref(), Some("metadata projection failed"));
        }
        assert_eq!(durable.possible_held, vec![key()]);
        assert!(durable.owners[0].controls.is_empty());
        assert_eq!(c.persist_attempts.get() - before, 2);
        let evidence = sql_read(&path).unwrap();
        assert!(c.submit(delivery(), 0).unwrap_err().contains("uncertain"));
        assert_eq!(sql_read(&path).unwrap(), evidence);
        drop(c);
        let mut fresh = fixture(Fault::None);
        fresh.journal = Some(path.clone());
        fresh.snapshot = durable;
        fresh.recover(vec![key()]).unwrap();
        let recovered = Coordinator::inspect_journal(&path).unwrap();
        assert!(!recovered.uncertain && recovered.cleanup_complete);
        assert!(recovered.held.is_empty() && recovered.possible_held.is_empty());
    }
}

#[test]
fn worker_expiry_commits_canceled_owner_projection_with_ack() {
    struct Delayed {
        worker: crate::worker::Worker<Recording>,
        expire: Arc<Mutex<bool>>,
        path: PathBuf,
        seen: Arc<Mutex<Vec<Snapshot>>>,
    }
    impl Channel for Delayed {
        fn request(&mut self, request: &Request) -> Result<Ack, String> {
            let delayed =
                *self.expire.lock().unwrap() && matches!(request.command, Command::Deliver { .. });
            if delayed {
                self.seen
                    .lock()
                    .unwrap()
                    .push(Coordinator::inspect_journal(&self.path)?);
            }
            Ok(self
                .worker
                .receive(request.clone(), if delayed { 1101 } else { clock() }))
        }
    }
    let dir = tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
        .unwrap();
    let path = dir.path().join("journal.json");
    let expire = Arc::new(Mutex::new(false));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut c = Coordinator::with_channel(
        Box::new(Delayed {
            worker: crate::worker::Worker::new(Recording),
            expire: expire.clone(),
            path: path.clone(),
            seen: seen.clone(),
        }),
        clock,
    );
    c.journal = Some(path.clone());
    c.activate(run(), Scope::DesktopWide).unwrap();
    c.arm(token(), "action".into()).unwrap();
    let other = Token {
        node_id: "other".into(),
        ..token()
    };
    c.arm(other.clone(), "other-action".into()).unwrap();
    c.submit(delivery(), 0).unwrap();
    let mut second = delivery();
    second.producer = other.clone();
    second.action_id = "other-action".into();
    c.submit(second, 0).unwrap();
    let mut value = delivery();
    value.operation_id = Counter(2);
    value.intent = Intent::Held {
        controls: vec![Control::Key { key: Key(5) }],
    };
    *expire.lock().unwrap() = true;
    let before = c.persist_attempts.get();
    let ack = c.submit(value.clone(), 0).unwrap();
    assert!(ack.ok && ack.skipped && !ack.uncertain);
    assert_eq!(ack.held, vec![key()]);
    assert_eq!(c.persist_attempts.get() - before, 2);
    let dispatched = seen.lock().unwrap().last().unwrap().clone();
    assert!(dispatched.in_flight.is_some());
    assert!(dispatched.possible_held.contains(&key()));
    assert!(dispatched
        .possible_held
        .contains(&Control::Key { key: Key(5) }));
    let persisted = Coordinator::inspect_journal(&path).unwrap();
    assert!(persisted.owners.contains(&OwnerSnapshot {
        producer: token(),
        action_id: "action".into(),
        controls: vec![]
    }));
    assert!(persisted.owners.contains(&OwnerSnapshot {
        producer: other,
        action_id: "other-action".into(),
        controls: vec![key()]
    }));
    assert_eq!(persisted.shared_controls, 0);
    assert_eq!(persisted.held, vec![key()]);
    assert_eq!(persisted.possible_held, persisted.held);
    assert!(persisted.in_flight.is_none());
    assert_eq!(
        sql_read(&path).unwrap(),
        serde_json::to_vec(&c.snapshot()).unwrap()
    );
    assert!(c.submit(value, 0).unwrap_err().contains("Stale"));
}
