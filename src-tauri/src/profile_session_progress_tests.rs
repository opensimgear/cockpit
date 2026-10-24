//! Exercise notifications through the real native host/coordinator. The peer
//! fixture owns only in-memory companion state; no OS devices are acquired.
use super::*;
use companion::tests::NativeFixture;
use std::sync::Mutex;

#[test]
fn native_progress_follows_actual_companion_apply_order_and_exact_epoch() {
    let fixture = NativeFixture::new();
    let events = Arc::new(Mutex::new(vec![]));
    let observed = events.clone();
    let host = fixture.host().with_progress(Arc::new(move |epoch, step, message| {
        observed.lock().unwrap().push((epoch, step.to_owned(), message.to_owned()));
    }));
    let mut coordinator = crate::profile_session::Coordinator::new(host, None);
    coordinator.apply(fixture.candidate(false)).unwrap();
    let events = events.lock().unwrap().clone();
    assert!(events.iter().all(|(epoch, _, message)| *epoch == 1 && !message.is_empty()));
    let position = |step: &str| events.iter().position(|(_, actual, _)| actual == step)
        .unwrap_or_else(|| panic!("Missing {step}: {events:?}"));
    assert!(position("preparing-resources") < position("companion-assignment"));
    assert!(position("companion-assignment") < position("companion-baseline"));
    assert!(position("companion-baseline") < position("configuring-mappings"));
    assert!(position("configuring-mappings") < position("starting-routes"));
    for absent in ["creating-virtual-device", "hiding-peripherals", "publishing-hardware-config",
        "preparing-desktop-bindings", "activating-desktop-bindings", "connecting-game-telemetry"] {
        assert!(!events.iter().any(|(_, step, _)| step == absent), "No such resource was requested: {absent}");
    }
    assert_eq!(coordinator.status().state, crate::profile_session::State::Running);
    coordinator.stop().unwrap();
}

#[test]
fn native_progress_does_not_announce_future_work_after_cancellation() {
    let fixture = NativeFixture::new();
    let events = Arc::new(Mutex::new(vec![]));
    let observed = events.clone();
    let host = fixture.host();
    let cancellation = host.admission_epoch.clone();
    let host = host.with_progress(Arc::new(move |epoch, step, _| {
        observed.lock().unwrap().push((epoch, step.to_owned()));
        if step == "companion-baseline" {
            cancellation.fetch_add(1, Ordering::AcqRel);
        }
    }));
    let mut coordinator = crate::profile_session::Coordinator::new(host, None);
    let error = coordinator.apply(fixture.candidate(false)).unwrap_err();
    assert!(error.contains("cancelled"), "{error}");
    let events = events.lock().unwrap();
    assert!(events.iter().any(|(_, step)| step == "companion-baseline"));
    assert!(events.iter().any(|(_, step)| step == "neutralizing-outputs"));
    assert!(!events.iter().any(|(_, step)| matches!(step.as_str(), "configuring-mappings" | "starting-routes")));
    assert!(events.iter().all(|(epoch, _)| *epoch == 1), "Cleanup belongs to the cancelled admission");
    assert!(coordinator.status().owner.is_none());
}

#[test]
fn native_cleanup_progress_follows_live_cleanup_and_uses_the_cleanup_epoch() {
    let fixture = NativeFixture::new();
    let events = Arc::new(Mutex::new(vec![]));
    let observed = events.clone();
    let host = fixture.host();
    let companions = host.companions.clone();
    let before = companions.profile_metadata().unwrap().assignment_snapshot(&["instance".into()]).unwrap();
    let restoration_started = Arc::new(Mutex::new(None));
    let captured = restoration_started.clone();
    let progress_companions = companions.clone();
    let host = host.with_progress(Arc::new(move |epoch, step, message| {
        observed.lock().unwrap().push((epoch, step.to_owned(), message.to_owned()));
        if step == "cleanup-restoring-companion-panels" {
            *captured.lock().unwrap() = Some(progress_companions.profile_metadata().unwrap()
                .assignment_snapshot(&["instance".into()]).unwrap());
        }
    }));
    let mut coordinator = crate::profile_session::Coordinator::new(host, None);
    coordinator.apply(fixture.candidate(false)).unwrap();
    let active = companions.profile_metadata().unwrap().assignment_snapshot(&["instance".into()]).unwrap();
    assert_ne!(active, before);
    events.lock().unwrap().clear();
    coordinator.stop_with_progress(42).unwrap();
    let events = events.lock().unwrap().clone();
    assert!(!events.is_empty());
    assert!(events.iter().all(|(epoch, _, message)| *epoch == 42 && !message.is_empty()), "{events:?}");
    let position = |step: &str| events.iter().position(|(_, actual, _)| actual == step)
        .unwrap_or_else(|| panic!("Missing {step}: {events:?}"));
    assert!(position("cleanup-stopping-mappings") < position("cleanup-releasing-companion-bindings"));
    assert!(position("cleanup-releasing-companion-bindings") < position("cleanup-validating-receipts"));
    assert!(position("cleanup-validating-receipts") < position("cleanup-restoring-companion-panels"));
    assert_eq!(*restoration_started.lock().unwrap(), Some(active), "The restoration status precedes its actual effect");
    assert_eq!(companions.profile_metadata().unwrap().assignment_snapshot(&["instance".into()]).unwrap(), before);
    for absent in ["cleanup-releasing-desktop-bindings", "cleanup-removing-virtual-device", "cleanup-unhiding-peripherals",
        "cleanup-restoring-peripheral-config", "cleanup-restoring-peripheral-outputs", "cleanup-stopping-scripts",
        "cleanup-disconnecting-game"] {
        assert!(!events.iter().any(|(_, step, _)| step == absent), "No such output was owned: {absent}");
    }
    assert_eq!(coordinator.status().state, crate::profile_session::State::Stopped);
    assert!(coordinator.status().owner.is_none());
}

#[test]
fn native_cleanup_progress_reports_cold_journal_recovery_without_a_prepared_run() {
    let fixture = NativeFixture::new();
    let directory = crate::test_directory();
    let journal = directory.path().join("cleanup-progress-session.json");
    let host = fixture.host();
    let companions = host.companions.clone();
    let before = companions.profile_metadata().unwrap().assignment_snapshot(&["instance".into()]).unwrap();
    let mut coordinator = crate::profile_session::Coordinator::new(host, Some(journal.clone()));
    coordinator.apply(fixture.candidate(false)).unwrap();
    drop(coordinator);
    let events = Arc::new(Mutex::new(vec![]));
    let observed = events.clone();
    let host = fixture.host().with_progress(Arc::new(move |epoch, step, message| {
        observed.lock().unwrap().push((epoch, step.to_owned(), message.to_owned()));
    }));
    assert!(host.prepared.is_empty(), "Recovery must not reconstruct executable profile state");
    let mut restarted = crate::profile_session::Coordinator::new(host, Some(journal));
    assert_eq!(restarted.status().state, crate::profile_session::State::RecoveryNeeded);
    restarted.recover_with_progress(43).unwrap();
    let events = events.lock().unwrap().clone();
    assert!(!events.is_empty());
    assert!(events.iter().all(|(epoch, _, message)| *epoch == 43 && !message.is_empty()), "{events:?}");
    let position = |step: &str| events.iter().position(|(_, actual, _)| actual == step)
        .unwrap_or_else(|| panic!("Missing {step}: {events:?}"));
    assert!(position("cleanup-validating-recorded-outputs") < position("cleanup-validating-receipts"));
    assert!(position("cleanup-validating-receipts") < position("cleanup-restoring-companion-panels"));
    assert!(!events.iter().any(|(_, step, _)| matches!(step.as_str(),
        "cleanup-stopping-mappings" | "cleanup-releasing-desktop-bindings" | "cleanup-removing-virtual-device")));
    assert_eq!(companions.profile_metadata().unwrap().assignment_snapshot(&["instance".into()]).unwrap(), before);
    assert_eq!(restarted.status().state, crate::profile_session::State::Stopped);
    assert!(restarted.status().owner.is_none());
}

#[test]
fn native_cleanup_progress_does_not_announce_restoration_after_receipt_validation_fails() {
    let fixture = NativeFixture::new();
    let directory = crate::test_directory();
    let journal = directory.path().join("invalid-cleanup-progress-session.json");
    let mut coordinator = crate::profile_session::Coordinator::new(fixture.host(), Some(journal.clone()));
    coordinator.apply(fixture.candidate(false)).unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&osg_persistence::test_support::read_evidence(&journal,"recovery-profile").unwrap()).unwrap();
    let mut run: OwnedRun = serde_json::from_value(saved["active"].clone()).unwrap();
    drop(coordinator);
    run.receipts.push(Receipt {kind:"unknown-output".into(),resource_id:"unknown".into(),state:json!({})});
    let events = Arc::new(Mutex::new(vec![]));
    let observed = events.clone();
    let mut host = fixture.host().with_progress(Arc::new(move |epoch, step, _| {
        observed.lock().unwrap().push((epoch, step.to_owned()));
    }));
    host.set_cleanup_progress(Some(44));
    let before = host.companions.profile_metadata().unwrap().assignment_snapshot(&["instance".into()]).unwrap();
    assert!(host.restore(&run, true).unwrap_err().contains("Unknown native recovery receipts"));
    assert_eq!(*events.lock().unwrap(), vec![(44,"cleanup-validating-receipts".into())]);
    assert_eq!(host.companions.profile_metadata().unwrap().assignment_snapshot(&["instance".into()]).unwrap(), before);
    run.receipts.pop();
    host.restore(&run, true).unwrap();
    host.set_cleanup_progress(None);
    let count = events.lock().unwrap().len();
    host.report_progress(&run.owner, "stale-cleanup", "Stale cleanup");
    assert_eq!(events.lock().unwrap().len(), count, "Clearing the scoped epoch prevents stale cold-recovery notifications");
}
