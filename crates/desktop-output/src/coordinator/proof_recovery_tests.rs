use super::*;
use super::test_support::*;
use std::{sync::mpsc, time::Duration};

#[test]
fn impossible_native_counts_on_acquisition_and_release_do_not_narrow_recovery() {
    for release in [false, true] {
        let directory = journal_dir();
        let path = directory.path().join("forged-counts.json");
        let mut fixture = Fixture::new();
        fixture.coordinator.journal = Some(path.clone());
        fixture.coordinator.activate(run(1), Scope::DesktopWide).unwrap();
        fixture.coordinator.arm(token(1), "action".into()).unwrap();
        if release { fixture.coordinator.submit(down(1, 1), 0).unwrap(); }
        *fixture.fault.lock().unwrap() = Forgery::ZeroCounts;
        let error = if release {
            fixture.coordinator.arm(token(1), "action".into()).unwrap_err()
        } else { fixture.coordinator.submit(down(1, 1), 0).unwrap_err() };
        assert!(error.contains("native counts"), "{error}");
        let saved = Coordinator::inspect_journal(&path).unwrap();
        assert!(saved.uncertain && !saved.cleanup_complete);
        assert!(saved.possible_held.contains(&key(1)));
        if !release { assert!(saved.in_flight.is_some()); }
    }
}

#[test]
fn forged_missing_hold_after_actual_down_retains_prepared_recovery_evidence() {
    let directory = journal_dir();
    let path = directory.path().join("forged-hold.json");
    let mut fixture = Fixture::new();
    fixture.coordinator.journal = Some(path.clone());
    fixture.coordinator.activate(run(1), Scope::DesktopWide).unwrap();
    fixture.coordinator.arm(token(1), "action".into()).unwrap();
    fixture.updates.lock().unwrap().clear();
    *fixture.fault.lock().unwrap() = Forgery::MissingHeld;
    let value = down(1, 1);
    assert!(fixture.coordinator.submit(value.clone(), 0).unwrap_err().contains("owner projection"));
    assert_eq!(*fixture.source.events.lock().unwrap(), [NativeEvent::Key { usage: 4, down: true, repeat: false }]);
    let snapshot = fixture.coordinator.snapshot();
    assert!(snapshot.uncertain && !snapshot.cleanup_complete);
    assert!(snapshot.possible_held.contains(&key(1)));
    assert!(matches!(&snapshot.in_flight.as_ref().unwrap().command, Command::Deliver { delivery } if delivery == &value));
    assert!(fixture.updates.lock().unwrap().is_empty());
    let retained = Coordinator::inspect_journal(&path).unwrap();
    assert!(retained.uncertain && retained.possible_held.contains(&key(1)) && retained.in_flight.is_some());
    let event_count = fixture.source.events.lock().unwrap().len();
    assert!(fixture.coordinator.submit(down(1, 2), 0).unwrap_err().contains("uncertain"));
    assert_eq!(fixture.source.events.lock().unwrap().len(), event_count);
    drop(fixture.coordinator);
    let mut recovery = Fixture::new();
    recovery.coordinator.journal = Some(path.clone());
    recovery.coordinator.snapshot = retained.clone();
    let mut controls = retained.held;
    controls.extend(retained.possible_held);
    recovery.coordinator.recover(controls).unwrap();
    assert_eq!(*recovery.source.events.lock().unwrap(), [NativeEvent::Key { usage: 4, down: false, repeat: false }]);
    let recovered = Coordinator::inspect_journal(&path).unwrap();
    assert!(!recovered.uncertain && recovered.possible_held.is_empty() && recovered.in_flight.is_none());
}

#[test]
fn cleanup_cannot_be_forged_into_a_successful_scope_rejection() {
    for intent in [Intent::Cleanup, Intent::Held { controls: vec![] }] {
        let mut fixture = Fixture::two_runs();
        fixture.coordinator.submit(down(1, 1), 0).unwrap();
        *fixture.fault.lock().unwrap() = Forgery::RejectCleanup;
        assert!(fixture.coordinator.submit(delivery(1, 2, intent), 0).unwrap_err().contains("scope rejection"));
        let snapshot = fixture.coordinator.snapshot();
        assert!(snapshot.uncertain && !snapshot.cleanup_complete);
        assert!(snapshot.possible_held.contains(&key(1)));
        assert_eq!(*fixture.source.events.lock().unwrap(), [NativeEvent::Key { usage: 4, down: true, repeat: false }]);
        assert!(fixture.updates.lock().unwrap().is_empty());
    }
}

#[test]
fn admission_field_is_mandatory_even_when_control_commands_use_null() {
    let request = Request { sequence: Counter(1), admission: None, deadline_us: None, command: Command::Stop };
    let mut value = serde_json::to_value(request).unwrap();
    assert!(value.get("admission").unwrap().is_null());
    assert!(serde_json::from_value::<Request>(value.clone()).is_ok());
    value.as_object_mut().unwrap().remove("admission");
    assert!(serde_json::from_value::<Request>(value).is_err());
}

#[test]
fn observer_failure_on_healthy_rejection_faults_and_retains_recovery_superset() {
    let mut fixture = Fixture::two_runs();
    fixture.coordinator.submit(down(1, 1), 0).unwrap();
    fixture.source.lose(fixture.registration(1));
    fixture.coordinator.set_scope_observer(Arc::new(|_| panic!("injected observer failure")));
    *fixture.fault.lock().unwrap() = Forgery::Rejected;
    assert!(fixture.coordinator.renew(run(2)).unwrap_err().contains("observer panicked"));
    let snapshot = fixture.coordinator.snapshot();
    assert!(snapshot.uncertain && !snapshot.cleanup_complete);
    assert_eq!(snapshot.phase, "faulted");
    assert!(snapshot.possible_held.contains(&key(1)));
    assert!(matches!(fixture.source.events.lock().unwrap().last(), Some(NativeEvent::Key { usage: 4, down: false, .. })));
    let count = fixture.source.events.lock().unwrap().len();
    assert!(fixture.coordinator.renew(run(2)).unwrap_err().contains("uncertain"));
    assert_eq!(fixture.source.events.lock().unwrap().len(), count);
}

#[test]
fn scope_revocation_and_native_release_publish_before_blocked_final_journal_commit() {
    let directory = journal_dir();
    let path = directory.path().join("blocked-scope-commit.json");
    let mut fixture = Fixture::two_runs();
    fixture.coordinator.journal = Some(path.clone());
    fixture.coordinator.submit(down(1, 1), 0).unwrap();
    let database = osg_persistence::Database::for_namespace(&path).unwrap();
    let lock = osg_persistence::test_support::hold_writer(&database).unwrap();
    let (published, observing) = mpsc::channel();
    fixture.coordinator.set_scope_observer(Arc::new(move |update| { published.send(update).unwrap(); }));
    fixture.source.lose(fixture.registration(1));
    let events = fixture.source.events.clone();
    let (finished, completion) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let result = fixture.coordinator.renew(run(2));
        finished.send(result.clone()).unwrap();
        (fixture.coordinator, result)
    });
    let update = observing.recv_timeout(Duration::from_secs(1));
    let before_unlock = events.lock().unwrap().clone();
    let pending = completion.recv_timeout(Duration::from_millis(50));
    drop(lock);
    let (coordinator, result) = thread.join().unwrap();
    let update = update.expect("scope callback precedes final write");
    assert_eq!(update.run, run(1));
    assert_eq!(update.receipt.admission.epoch, Counter(2));
    assert!(matches!(before_unlock.last(), Some(NativeEvent::Key { usage: 4, down: false, .. })));
    assert!(matches!(pending, Err(mpsc::RecvTimeoutError::Timeout)), "{pending:?}");
    result.unwrap();
    assert!(coordinator.snapshot().held.is_empty());
    assert!(Coordinator::inspect_journal(&path).unwrap().held.is_empty());
}
