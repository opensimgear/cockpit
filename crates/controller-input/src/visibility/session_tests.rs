use super::super::policy::{
    tests::{capture, selected, state},
    Policy, State,
};
use super::*;
use std::sync::{Arc, Mutex};
struct Driver(Arc<Mutex<State>>);
impl Backend for Driver {
    fn read(&mut self) -> Result<State, String> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn change(&mut self, expected: &State, desired: &State) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        if *state != *expected {
            return Err("external change".into());
        }
        *state = desired.clone();
        Ok(())
    }
}
fn observed() -> Source {
    Source {
        id: uuid::Uuid::new_v4().to_string(),
        session: uuid::Uuid::new_v4().to_string(),
        generation: 1,
        sequence: 1,
        interface: "exact-interface".into(),
    }
}
fn plan() -> Plan {
    Plan::build(state(), selected(Policy::HideAll), capture()).unwrap()
}
#[test]
fn retained_inspection_errors_and_journal_edits_never_stop_or_rewrite_the_session() {
    let temp = crate::test_directory();
    let path = temp.path().join("inspect.json");
    let state = Arc::new(Mutex::new(state()));
    let source = observed();
    let mut session = Session::start(
        Driver(state.clone()),
        Store::open(path.clone()).unwrap(),
        plan(),
        vec![source.clone()],
        |fresh| {
            let mut source = source.clone();
            if fresh {
                source.sequence += 1;
            }
            Ok(vec![source])
        },
    )
    .unwrap();
    let active = state.lock().unwrap().clone();
    let original = sql_read(&path).unwrap();
    assert!(session
        .inspect::<()>(|_, _| Err("Read-only preparation rejected".into()))
        .unwrap_err()
        .contains("preparation rejected"));
    assert_eq!(*state.lock().unwrap(), active);
    assert_eq!(sql_read(&path).unwrap(), original);
    assert!(!session.record.clean);
    let mut edited = original.clone();
    edited.push(b' ');
    assert!(session
        .inspect::<()>(|_, _| {
            sql_write(&path, &edited).unwrap();
            Ok(())
        })
        .unwrap_err()
        .contains("changed externally"));
    assert_eq!(*state.lock().unwrap(), active);
    assert_eq!(sql_read(&path).unwrap(), edited);
    assert!(session
        .inspect::<()>(|_, _| panic!("Do not inspect a changed journal"))
        .is_err());
    sql_write(&path, original).unwrap();
    assert!(session.stop().is_err()); // Equal bytes at a new revision cannot revive this owner.
    assert!(session
        .inspect::<()>(|_, _| panic!("Do not inspect a stopped session"))
        .is_err());
}
#[test]
fn fresh_capture_is_required_and_standalone_stop_or_drop_restores() {
    let temp = crate::test_directory();
    let path = temp.path().join("v.json");
    let state = Arc::new(Mutex::new(state()));
    let source = observed();
    let before = state.lock().unwrap().clone();
    assert!(Session::start(
        Driver(state.clone()),
        Store::open(path.clone()).unwrap(),
        plan(),
        vec![source.clone()],
        |_| Ok(vec![source.clone()])
    )
    .is_err());
    assert_eq!(*state.lock().unwrap(), before);
    // A preflight report that already advanced before hiding cannot satisfy the
    // post-hide capture proof if no later report arrives.
    assert!(Session::start(
        Driver(state.clone()),
        Store::open(path.clone()).unwrap(),
        plan(),
        vec![source.clone()],
        |_| {
            let mut preflight = source.clone();
            preflight.sequence += 1;
            Ok(vec![preflight])
        }
    )
    .is_err());
    assert_eq!(*state.lock().unwrap(), before);
    let mut session = Session::start(
        Driver(state.clone()),
        Store::open(path.clone()).unwrap(),
        plan(),
        vec![source.clone()],
        |fresh| {
            let mut current = source.clone();
            if fresh {
                current.sequence += 1;
            }
            Ok(vec![current])
        },
    )
    .unwrap();
    session.check(Ok(vec![source])).unwrap();
    assert!(state.lock().unwrap().active);
    session.stop().unwrap();
    assert_eq!(*state.lock().unwrap(), before);
    assert!(session.check(Ok(vec![])).is_err());
    drop(session);
    let source = observed();
    let session = Session::start(
        Driver(state.clone()),
        Store::open(path).unwrap(),
        plan(),
        vec![source.clone()],
        |fresh| {
            let mut current = source.clone();
            if fresh {
                current.sequence += 1;
            }
            Ok(vec![current])
        },
    )
    .unwrap();
    drop(session);
    assert_eq!(*state.lock().unwrap(), before);
}
#[test]
fn reconnect_path_changes_capture_loss_and_owner_loss_restore_without_reapply() {
    for reason in 0..5 {
        let temp = crate::test_directory();
        let state = Arc::new(Mutex::new(state()));
        let source = observed();
        let mut session = Session::start(
            Driver(state.clone()),
            Store::open(temp.path().join("v.json")).unwrap(),
            plan(),
            vec![source.clone()],
            |fresh| {
                let mut current = source.clone();
                if fresh {
                    current.sequence += 1;
                }
                Ok(vec![current])
            },
        )
        .unwrap();
        let mut changed = source;
        match reason {
            0 => changed.generation += 1,
            1 => changed.interface = "new-port".into(),
            2 => changed.session = uuid::Uuid::new_v4().to_string(),
            3 => changed.sequence = 0,
            _ => {}
        }
        assert!(session
            .check(if reason == 4 {
                Err("selected executable moved or combined owner exited".into())
            } else {
                Ok(vec![changed])
            })
            .is_err());
        assert!(!state.lock().unwrap().active);
        assert!(session.record.clean);
    }
}
#[test]
fn source_bounds_and_external_policy_conflicts_never_broaden_cleanup() {
    let source = observed();
    assert!(validate_sources(&[], &[], false).is_err());
    assert!(validate_sources(&[source.clone()], &[], false).is_err());
    assert!(validate_sources(
        &[source.clone(), source.clone()],
        &[source.clone(), source.clone()],
        false
    )
    .is_err());
    let mut other = source.clone();
    other.id = uuid::Uuid::new_v4().to_string();
    assert!(validate_sources(&[source.clone()], &[other], false).is_err());
    let temp = crate::test_directory();
    let state = Arc::new(Mutex::new(state()));
    let mut session = Session::start(
        Driver(state.clone()),
        Store::open(temp.path().join("v.json")).unwrap(),
        plan(),
        vec![source.clone()],
        |fresh| {
            let mut s = source.clone();
            if fresh {
                s.sequence += 1;
            }
            Ok(vec![s])
        },
    )
    .unwrap();
    state.lock().unwrap().hidden.push(r"HID\EXTERNAL\1".into());
    let external = state.lock().unwrap().clone();
    assert!(session.check(Err("source removed".into())).is_err());
    drop(session);
    assert_eq!(*state.lock().unwrap(), external);
}

fn sql_read(path:&std::path::Path)->Result<Vec<u8>,String>{osg_persistence::test_support::read_evidence(path,"recovery-visibility")}
fn sql_write(path:&std::path::Path,data:impl AsRef<[u8]>)->Result<(),String>{osg_persistence::test_support::write_evidence(path,"recovery-visibility",Some(data.as_ref()))}
