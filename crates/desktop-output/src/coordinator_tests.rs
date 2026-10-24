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
    let bytes = sql_read(&path).unwrap();
    c.renew(run()).unwrap();
    assert_eq!(sql_read(&path).unwrap(), bytes);
    assert!(Coordinator::spawn_with_journal(false, false, path.clone())
        .unwrap_err_string()
        .contains("owns"));
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
    sql_write(&path, b"{}").unwrap();
    assert!(Coordinator::inspect_journal(&path).is_err());
    assert!(Coordinator::recover_journal(false, false, path.clone()).is_err());
    sql_write(&path, vec![b' '; 131_073]).unwrap();
    assert!(Coordinator::inspect_journal(&path)
        .unwrap_err()
        .contains("bound"));
    let mut snapshot = fixture(Fault::None).snapshot();
    snapshot.possible_held = vec![Control::Key { key: Key(0xffff) }];
    sql_write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
    assert!(Coordinator::inspect_journal(&path).is_err());
    snapshot.possible_held.clear();
    sql_write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
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
fn submit_keeps_both_dispatch_writes_and_elides_only_unchanged_owner_projection() {
    let dir = tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target")).unwrap();
    let path = dir.path().join("journal.json");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut c = Coordinator::with_channel(Box::new(Fixture {
        worker: crate::worker::Worker::new(Recording),
        fault: Fault::None,
        inspect: Some(path.clone()),
        seen: seen.clone(),
    }), clock);
    c.journal = Some(path.clone());
    c.activate(run(), Scope::DesktopWide).unwrap();
    c.arm(token(), "action".into()).unwrap();

    let before = c.persist_attempts.get();
    c.submit(delivery(), 0).unwrap();
    assert_eq!(c.persist_attempts.get() - before, 3);
    let owners = vec![OwnerSnapshot { producer: token(), action_id: "action".into(), controls: vec![key()] }];
    assert_eq!(Coordinator::inspect_journal(&path).unwrap().owners, owners);

    for (operation, intent) in [
        (2, Intent::Pointer { dx: 1, dy: -1 }),
        (3, Intent::Repeat { key: Key(4) }),
        (4, Intent::Wheel { horizontal: 1, vertical: 0 }),
    ] {
        let mut value = delivery();
        value.operation_id = Counter(operation);
        value.sequence = Counter(operation);
        value.intent = intent;
        let before = c.persist_attempts.get();
        let ack = c.submit(value.clone(), 0).unwrap();
        assert!(ack.ok && !ack.skipped && !ack.uncertain);
        assert_eq!(ack.attempted, 1);
        assert_eq!(c.persist_attempts.get() - before, 2, "Pre-dispatch and post-ACK writes remain mandatory");
        let dispatched = seen.lock().unwrap().last().unwrap().clone();
        assert!(matches!(&dispatched.in_flight.unwrap().command, Command::Deliver { delivery } if delivery == &value));
        assert!(dispatched.possible_held.contains(&key()));
        let persisted = Coordinator::inspect_journal(&path).unwrap();
        assert_eq!(persisted.owners, owners);
        assert_eq!(persisted.held, vec![key()]);
        assert_eq!(persisted.possible_held, persisted.held);
        assert!(persisted.in_flight.is_none());
        assert_eq!(sql_read(&path).unwrap(), serde_json::to_vec(&c.snapshot()).unwrap());
        // Eliding serialized owners must not discard the reducer's new replay fence.
        assert!(c.submit(value, 0).unwrap_err().contains("Stale"));
        assert_eq!(c.persist_attempts.get() - before, 2);
    }

    let mut release = delivery();
    release.operation_id = Counter(5);
    release.sequence = Counter(5);
    release.intent = Intent::Held { controls: vec![] };
    let before = c.persist_attempts.get();
    c.submit(release, 0).unwrap();
    assert_eq!(c.persist_attempts.get() - before, 3);
    let persisted = Coordinator::inspect_journal(&path).unwrap();
    assert!(persisted.held.is_empty() && persisted.owners[0].controls.is_empty());
    assert!(persisted.in_flight.is_none());
    assert_eq!(sql_read(&path).unwrap(), serde_json::to_vec(&c.snapshot()).unwrap());
}

#[test]
fn shared_native_hold_changes_still_persist_each_full_owner_and_collision_count() {
    let dir = tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target")).unwrap();
    let path = dir.path().join("journal.json");
    let mut c = fixture(Fault::None);
    c.journal = Some(path.clone());
    c.activate(run(), Scope::DesktopWide).unwrap();
    c.arm(token(), "action".into()).unwrap();
    let other = Token { node_id: "other".into(), ..token() };
    c.arm(other.clone(), "other-action".into()).unwrap();
    c.submit(delivery(), 0).unwrap();
    let mut value = delivery();
    value.producer = other.clone();
    value.action_id = "other-action".into();
    let before = c.persist_attempts.get();
    let ack = c.submit(value, 0).unwrap();
    assert_eq!(ack.attempted, 0, "The second owner must not duplicate the native key edge");
    assert_eq!(c.persist_attempts.get() - before, 3);
    let shared = Coordinator::inspect_journal(&path).unwrap();
    assert_eq!(shared.held, vec![key()]);
    assert_eq!(shared.shared_controls, 1);
    assert!(shared.owners.contains(&OwnerSnapshot { producer: other, action_id: "other-action".into(), controls: vec![key()] }));

    let mut release = delivery();
    release.operation_id = Counter(2);
    release.sequence = Counter(2);
    release.intent = Intent::Held { controls: vec![] };
    let before = c.persist_attempts.get();
    assert_eq!(c.submit(release, 0).unwrap().attempted, 0);
    assert_eq!(c.persist_attempts.get() - before, 3);
    let remaining = Coordinator::inspect_journal(&path).unwrap();
    assert_eq!(remaining.held, vec![key()]);
    assert_eq!(remaining.shared_controls, 0);
    assert!(remaining.owners.contains(&OwnerSnapshot { producer: token(), action_id: "action".into(), controls: vec![] }));
    assert_eq!(sql_read(&path).unwrap(), serde_json::to_vec(&c.snapshot()).unwrap());
}


fn sql_read(path:&std::path::Path)->Result<Vec<u8>,String>{osg_persistence::test_support::read_evidence(path,"recovery-desktop")}
fn sql_write(path:&std::path::Path,data:impl AsRef<[u8]>)->Result<(),String>{osg_persistence::test_support::write_evidence(path,"recovery-desktop",Some(data.as_ref()))}

#[test]
fn stale_evidence_and_exclusive_lifetime_owner_prevent_helper_dispatch(){
    let directory=tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();let path=directory.path().join("journal.json");
    let mut c=fixture(Fault::None);c.journal=Some(path.clone());
    c.snapshot.journal_owner=Some(run());c.snapshot.journal_scope=Some(Scope::DesktopWide);
    c.activate(run(),Scope::DesktopWide).unwrap();
    assert!(Coordinator::spawn_with_journal(false,false,path.clone()).unwrap_err_string().contains("owns"));
    let original=sql_read(&path).unwrap();
    let mut foreign=original.clone();foreign.push(b' ');sql_write(&path,&foreign).unwrap();
    assert!(c.arm(token(),"action".into()).unwrap_err().contains("externally"));
    assert_eq!(sql_read(&path).unwrap(),foreign);
    sql_write(&path,&original).unwrap();
    assert!(c.stop_all().is_err()); // Equal bytes cannot rewind the owner revision.
    drop(c);
    let mut stale=run();stale.generation=Counter(9);
    assert!(Coordinator::recover_owned_journal(false,false,path.clone(),&stale,None).unwrap_err().contains("exact"));
    assert!(Coordinator::recover_owned_journal(false,false,path.clone(),&run(),Some(&Scope::DesktopWide)).unwrap_err().contains("Enable"));
    assert_eq!(sql_read(&path).unwrap(),original);
}

#[path="coordinator/lifecycle_tests.rs"]
mod lifecycle;
