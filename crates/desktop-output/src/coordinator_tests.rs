use super::*;
use osg_mapping::desktop::Key;
use std::sync::{Arc, Mutex};
struct Recording;
impl Adapter for Recording {
    fn eligible(&mut self, _: &Scope) -> Result<bool, String> {
        Ok(true)
    }
    fn submit(&mut self, e: &[NativeEvent]) -> Result<usize, String> {
        Ok(native_count(e))
    }
}
#[derive(Clone, Copy)]
enum Fault {
    None,
    Sequence,
    Short,
    Rejected,
    Transport,
    Uncertain,
}
struct Fixture {
    worker: crate::worker::Worker<Recording>,
    fault: Fault,
    inspect: Option<PathBuf>,
    seen: Arc<Mutex<Vec<Snapshot>>>,
}
impl Channel for Fixture {
    fn request(&mut self, r: &Request) -> Result<Ack, String> {
        if let Some(path) = &self.inspect {
            self.seen
                .lock()
                .unwrap()
                .push(Coordinator::inspect_journal(path)?);
        }
        let mut ack = self.worker.receive(r.clone(), 1000);
        let fault = std::mem::replace(&mut self.fault, Fault::None);
        match fault {
            Fault::None => {}
            Fault::Sequence => ack.sequence = Counter(999),
            Fault::Short => {
                ack.attempted = 2;
                ack.confirmed = 1;
            }
            Fault::Rejected => {
                ack.ok = false;
                ack.error = Some("fixture scope paused".into())
            }
            Fault::Transport => return Err("fixture worker death".into()),
            Fault::Uncertain => {
                ack.ok = false;
                ack.uncertain = true;
                ack.error = Some("uncertain fixture submission".into());
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
        session: "journal-test".into(),
        generation: Counter(1),
    }
}
fn token() -> Token {
    Token {
        run: run(),
        node_id: "node".into(),
        dependencies: vec![],
    }
}
fn key() -> Control {
    Control::Key { key: Key(4) }
}
fn delivery() -> Delivery {
    Delivery {
        producer: token(),
        action_id: "action".into(),
        operation_id: Counter(1),
        sequence: Counter(1),
        at_us: Counter(0),
        deadline_us: Some(Counter(100)),
        intent: Intent::Held {
            controls: vec![key()],
        },
    }
}
fn fixture(fault: Fault) -> Coordinator {
    Coordinator::with_channel(
        Box::new(Fixture {
            worker: crate::worker::Worker::new(Recording),
            fault,
            inspect: None,
            seen: Arc::default(),
        }),
        clock,
    )
}
#[test]
fn journal_is_durable_before_dispatch_and_retains_scope_and_owners() {
    let dir = tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
        .unwrap();
    let path = dir.path().join("journal.json");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut c = Coordinator::with_channel(
        Box::new(Fixture {
            worker: crate::worker::Worker::new(Recording),
            fault: Fault::None,
            inspect: Some(path.clone()),
            seen: seen.clone(),
        }),
        clock,
    );
    c.journal = Some(path.clone());
    c.activate(run(), Scope::DesktopWide).unwrap();
    c.arm(token(), "action".into()).unwrap();
    let ack = c.submit(delivery(), 0).unwrap();
    assert_eq!(ack.confirmed, 1);
    let before = seen.lock().unwrap();
    let snapshot = before.last().unwrap();
    assert!(snapshot.in_flight.is_some());
    assert!(snapshot.possible_held.contains(&key()));
    drop(before);
    let persisted = Coordinator::inspect_journal(&path).unwrap();
    assert_eq!(persisted.runs.len(), 1);
    assert_eq!(persisted.owners.len(), 1);
    assert_eq!(persisted.held, vec![key()]);
    assert!(persisted.in_flight.is_none());
    // A heartbeat has no new native operation and does not rewrite its durable record.
    let bytes = std::fs::read(&path).unwrap();
    c.renew(run()).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(Coordinator::spawn_with_journal(false, false, path.clone())
        .unwrap_err_string()
        .contains("Interrupted"));
    c.pause(run()).unwrap();
    assert!(c.snapshot().owners.is_empty());
    c.arm(token(), "action".into()).unwrap();
    c.submit(delivery(), 0).unwrap();
    c.stop_all().unwrap();
    let stopped = Coordinator::inspect_journal(&path).unwrap();
    assert!(stopped.held.is_empty() && stopped.owners.is_empty() && stopped.runs.is_empty());
}
trait ErrorString {
    fn unwrap_err_string(self) -> String;
}
impl ErrorString for Result<Coordinator, String> {
    fn unwrap_err_string(self) -> String {
        match self {
            Err(e) => e,
            Ok(_) => panic!("expected failure"),
        }
    }
}
#[test]
fn invalid_and_overlarge_journals_fail_closed() {
    let dir = tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
        .unwrap();
    let path = dir.path().join("journal.json");
    assert!(Coordinator::inspect_journal(&path).is_err());
    std::fs::write(&path, b"{}").unwrap();
    assert!(Coordinator::inspect_journal(&path).is_err());
    assert!(Coordinator::recover_journal(false, false, path.clone()).is_err());
    std::fs::write(&path, vec![b' '; 131_073]).unwrap();
    assert!(Coordinator::inspect_journal(&path)
        .unwrap_err()
        .contains("bound"));
    let mut snapshot = fixture(Fault::None).snapshot();
    snapshot.possible_held = vec![Control::Key { key: Key(0xffff) }];
    std::fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
    assert!(Coordinator::inspect_journal(&path).is_err());
    snapshot.possible_held.clear();
    std::fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
    assert!(Coordinator::spawn_with_journal(false, false, path.clone())
        .unwrap_err_string()
        .contains("Enable"));
    assert!(Coordinator::recover_journal(false, false, path).is_err());
}
#[test]
fn stale_and_incomplete_acknowledgements_preserve_inflight_uncertainty() {
    for fault in [
        Fault::Sequence,
        Fault::Short,
        Fault::Transport,
        Fault::Uncertain,
    ] {
        let mut c = fixture(fault);
        assert!(c.activate(run(), Scope::DesktopWide).is_err());
        let snapshot = c.snapshot();
        assert!(snapshot.uncertain && snapshot.in_flight.is_some());
        assert_eq!(snapshot.phase, "faulted");
        assert!(c.activate(run(), Scope::DesktopWide).is_err());
    }
    let mut c = fixture(Fault::Rejected);
    assert!(c.activate(run(), Scope::DesktopWide).is_err());
    assert_eq!(c.snapshot().phase, "paused");
    assert!(!c.snapshot().uncertain);
}
#[test]
fn wrong_operation_ack_cannot_clear_a_completed_submission_record() {
    struct Wrong {
        worker: crate::worker::Worker<Recording>,
    }
    impl Channel for Wrong {
        fn request(&mut self, r: &Request) -> Result<Ack, String> {
            let mut ack = self.worker.receive(r.clone(), 1000);
            if matches!(r.command, Command::Deliver { .. }) {
                ack.operation_id = Some(Counter(99));
            }
            Ok(ack)
        }
    }
    let mut c = Coordinator::with_channel(
        Box::new(Wrong {
            worker: crate::worker::Worker::new(Recording),
        }),
        clock,
    );
    c.activate(run(), Scope::DesktopWide).unwrap();
    c.arm(token(), "action".into()).unwrap();
    assert!(c.submit(delivery(), 0).is_err());
    assert!(c.snapshot().possible_held.contains(&key()));
    c.stop_all().unwrap();
    assert!(c.snapshot().uncertain);
    assert!(c.recover(vec![key()]).is_err());
    let previous = c.snapshot();
    let mut fresh = fixture(Fault::None);
    fresh.snapshot = previous;
    fresh.recover(vec![key()]).unwrap();
    assert!(!fresh.snapshot().uncertain);
    assert_eq!(fresh.snapshot().phase, "stopped");
}
#[test]
fn journal_write_failure_never_dispatches() {
    struct Never;
    impl Channel for Never {
        fn request(&mut self, _: &Request) -> Result<Ack, String> {
            panic!("journal failure must prevent dispatch")
        }
    }
    let dir = tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
        .unwrap();
    let mut c = Coordinator::with_channel(Box::new(Never), clock);
    let file = dir.path().join("file");
    std::fs::write(&file, b"x").unwrap();
    c.journal = Some(file.join("journal.json"));
    assert!(c.activate(run(), Scope::DesktopWide).is_err());
    assert!(c.snapshot().in_flight.is_some());
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
    std::fs::write(&path, b"{broken").unwrap();
    assert!(Coordinator::inspect_owned_journal(&path, &expected, None).is_err());
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
    std::fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    assert!(Coordinator::inspect_owned_journal(&path, &expected, None).is_err());
    record.in_flight = Some(Request {
        sequence: Counter(2),
        deadline_us: None,
        command: Command::Activate {
            run: expected.clone(),
            scope: application,
        },
    });
    std::fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
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
