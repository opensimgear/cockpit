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
