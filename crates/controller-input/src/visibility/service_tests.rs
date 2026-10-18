use super::super::{journal::Backend, policy::tests::state, runtime::Adapter};
use super::*;
use crate::registry::{Control, Definition, Identity};
use std::collections::BTreeMap;
#[path = "game_tests.rs"]
mod game_tests;
fn inventory() -> (Arc<Mutex<Registry>>, String) {
    inventory_for("windows-raw-input")
}
fn inventory_for(backend: &str) -> (Arc<Mutex<Registry>>, String) {
    let mut registry = Registry::default();
    let id = registry
        .connect(Definition {
            identity: Identity {
                backend: backend.into(),
                vendor_id: 0x231d,
                product_id: 0x200,
                serial: None,
                collection: "1".into(),
                fingerprint: String::new(),
                path: "exact-source".into(),
            },
            name: "Controller".into(),
            controls: vec![Control::new(9, 1, 1, 0, 0)],
            diagnostics: vec![],
        })
        .unwrap();
    registry.report(&id, 1, &BTreeMap::from([(0, 0)])).unwrap();
    (Arc::new(Mutex::new(registry)), id)
}
struct Fixture(Arc<Mutex<State>>);
impl Backend for Fixture {
    fn read(&mut self) -> Result<State, String> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn change(&mut self, before: &State, after: &State) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        assert_eq!(&*state, before);
        *state = after.clone();
        Ok(())
    }
}
impl Adapter for Fixture {
    fn executable(&mut self, _: &str) -> Result<String, String> {
        Ok(r"\Device\Volume\Cockpit.exe".into())
    }
    fn instance(&mut self, _: &str) -> Result<String, String> {
        Ok(r"HID\EXACT\1".into())
    }
}
#[test]
fn native_development_readiness_keeps_explicit_visibility_settings_inert() {
    let (registry, id) = inventory();
    let temp = tempfile::tempdir().unwrap();
    let service = Service::new(Some(temp.path().into()), registry);
    let status = service.request(&json!({"command":"controller_visibility_status","sourceId":id})).unwrap();
    assert_eq!(status["visibility"]["qualified"], super::super::capabilities::DEVELOPMENT_NATIVE_PROFILES);
    assert_eq!(status["visibility"]["enabled"], false);
    assert_eq!(status["visibility"]["selectedCount"], 0);
    assert_eq!(status["visibility"]["global"]["inspected"], false);
    assert_eq!(status["visibility"]["selfAccess"]["status"], "not-requested");
    assert!(!service.active());
    assert!(temp.path().read_dir().unwrap().next().is_none());
    if super::super::capabilities::DEVELOPMENT_NATIVE_PROFILES {
        assert!(status["visibility"]["reason"].as_str().unwrap().starts_with("Development acceptance mode:"));
    }
}
#[test]
fn startup_self_access_guard_preserves_pending_standalone_game_and_invalid_journals() {
    use super::super::policy::{tests::{capture, selected}, Plan, Policy};
    let (registry, _) = inventory();
    let temp = tempfile::tempdir().unwrap();
    let mut service = Service::new(Some(temp.path().into()), registry);
    service.startup_self_access(); // cfg(test) ordinary startup must remain inert.
    assert_eq!(service.self_access_status().status, "not-requested");
    assert!(service.self_access_guard().is_ok());
    let plan = Plan::build(state(), selected(Policy::HideAll), capture()).unwrap();
    let path = service.path.as_ref().unwrap();
    let mut store = journal::Store::open(path.clone()).unwrap();
    let mut record = store.begin(plan.clone()).unwrap();
    assert!(service.self_access_guard().unwrap_err().contains("pending visibility recovery"));
    record.clean = true;
    store.write(&record).unwrap();
    drop(store);
    assert!(service.self_access_guard().is_ok());
    let game = temp.path().join(format!("game-visibility-{}.json", uuid::Uuid::new_v4()));
    let mut store = journal::Store::open(game.clone()).unwrap();
    store.begin(plan).unwrap();
    assert!(service.self_access_guard().unwrap_err().contains("owner"));
    drop(store);
    std::fs::write(&game, b"invalid").unwrap();
    assert!(service.self_access_guard().is_err());
    std::fs::remove_file(&game).unwrap();
    std::fs::write(path, b"invalid").unwrap();
    assert!(service.self_access_guard().is_err());
}
#[test]
fn default_status_and_saved_intent_are_inert_without_optional_dependencies() {
    let (registry, id) = inventory();
    let temp = tempfile::tempdir().unwrap();
    let mut service = Service::new(Some(temp.path().into()), registry.clone());
    service.qualified = false; // Exercise the default gate independently of the dev opt-in.
    service.platform = super::super::capabilities::Platform::Windows;
    service.factory = Arc::new(|| Err("HidHide missing or restart required".into()));
    let status = service
        .request(&json!({"command":"controller_visibility_status","sourceId":id}))
        .unwrap();
    assert_eq!(status["visibility"]["enabled"], false);
    assert_eq!(status["visibility"]["qualified"], false);
    assert!(temp.path().read_dir().unwrap().next().is_none());
    let inspected = service
        .request(&json!({"command":"controller_visibility_inspect"}))
        .unwrap();
    assert!(inspected["visibility"]["global"]["error"]
        .as_str()
        .unwrap()
        .contains("missing"));
    service.request(&json!({"command":"controller_visibility_save","sourceId":id,"intent":{"mode":"hide-all"}})).unwrap();
    service
        .request(&json!({"command":"controller_visibility_enable","enabled":true}))
        .unwrap();
    assert!(service
        .request(&json!({"command":"controller_visibility_apply","sourceIds":[id]}))
        .unwrap_err()
        .contains("disabled"));
    assert!(service
        .request(&json!({"command":"controller_visibility_restore"}))
        .is_err());
    assert!(!service.active());
    assert!(service
        .request(&json!({"command":"controller_visibility_enable","enabled":"yes"}))
        .is_err());
    assert!(service.request(&json!({"command":"controller_visibility_save","sourceId":id,"intent":{"mode":"unsupported"}})).is_err());
    assert!(service.request(&json!({"command":"controller_visibility_save","sourceId":"unknown","intent":{"mode":"visible"}})).is_err());
    assert!(service
        .request(&json!({"command":"controller_visibility_unknown"}))
        .is_err());
    assert!(service.request(&json!({})).is_err());
    assert!(registry.lock().unwrap().visibility_sources(&[]).is_err());
    assert!(registry
        .lock()
        .unwrap()
        .visibility_sources(&["missing".into()])
        .is_err());
}
#[test]
fn explicit_worker_admission_guards_edits_and_restore_remains_available() {
    let (registry, id) = inventory();
    let temp = tempfile::tempdir().unwrap();
    let state = Arc::new(Mutex::new(state()));
    let shared = state.clone();
    let mut service = Service::new(Some(temp.path().into()), registry.clone());
    service.qualified = true;
    service.platform = super::super::capabilities::Platform::Windows;
    service.factory = Arc::new(move || Ok(Box::new(Fixture(shared.clone()))));
    assert!(service
        .request(&json!({"command":"controller_visibility_apply","sourceIds":[id]}))
        .is_err());
    service
        .request(&json!({"command":"controller_visibility_enable","enabled":true}))
        .unwrap();
    assert!(service
        .request(&json!({"command":"controller_visibility_apply","sourceIds":[id]}))
        .is_err());
    service.request(&json!({"command":"controller_visibility_save","sourceId":id,"intent":{"mode":"hide-all"}})).unwrap();
    assert!(service
        .request(&json!({"command":"controller_visibility_apply","sourceIds":"bad"}))
        .is_err());
    let inspected = service
        .request(&json!({"command":"controller_visibility_inspect", "sourceId":id}))
        .unwrap();
    assert_eq!(inspected["visibility"]["effective"]["hidden"], false);
    let started = service
        .request(&json!({"command":"controller_visibility_apply","sourceIds":[id]}))
        .unwrap();
    assert_eq!(started["visibility"]["job"]["active"], true);
    for _ in 0..200 {
        registry
            .lock()
            .unwrap()
            .report(&id, 1, &BTreeMap::from([(0, 0)]))
            .unwrap();
        if service
            .worker
            .lock()
            .unwrap()
            .snapshot()
            .is_some_and(|s| s.phase == "active")
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        service.worker.lock().unwrap().snapshot().unwrap().phase,
        "active"
    );
    assert!(state.lock().unwrap().active);
    for command in [
        "controller_visibility_inspect",
        "controller_visibility_save",
        "controller_visibility_enable",
    ] {
        assert!(service.request(&json!({"command":command,"sourceId":id,"enabled":false,"intent":{"mode":"visible"}})).is_err());
    }
    let status=service.request(&json!({"command":"controller_visibility_restore","jobId":started["visibility"]["job"]["jobId"]})).unwrap();
    assert!(status["visibility"]["recovery"]["required"].is_boolean());
    for _ in 0..200 {
        if !service.active() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!service.active());
    assert!(!state.lock().unwrap().active);
    assert!(service
        .request(&json!({"command":"controller_visibility_restore"}))
        .is_err());
    registry.lock().unwrap().disconnect(&id);
    assert!(service.status(&id)["visibility"]["effective"].is_null());
    let no_path = Service::new(None, registry);
    assert!(no_path.status("")["visibility"]["recovery"]["required"] == false);
}
#[test]
fn corrupt_recovery_and_preferences_are_reported_without_startup_replay() {
    let (registry, _) = inventory();
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("controller-visibility.json"), b"invalid").unwrap();
    std::fs::write(
        temp.path().join("controller-visibility-session.json"),
        b"invalid",
    )
    .unwrap();
    let service = Service::new(Some(temp.path().into()), registry);
    let status = service.status("");
    assert!(status["visibility"]["settingsError"].is_string());
    assert_eq!(status["visibility"]["recovery"]["required"], true);
    assert!(!service.active());
}

#[test]
fn unix_capabilities_save_without_driver_access_and_never_broaden() {
    use super::super::capabilities::Platform;
    for (platform, backend) in [
        (Platform::Linux, "linux-evdev"),
        (Platform::Macos, "macos-iohid"),
    ] {
        let (registry, id) = inventory_for(backend);
        let dir = tempfile::tempdir().unwrap();
        let control = super::super::suppression::Control::default();
        let mut service =
            Service::new(Some(dir.path().into()), registry).with_suppression(control.clone());
        service.platform = platform;
        service.qualified = false;
        service.factory = Arc::new(|| panic!("Unix visibility must not inspect HidHide"));
        assert!(service
            .request(&json!({"command":"controller_visibility_inspect"}))
            .is_ok());
        assert!(service.request(&json!({"command":"controller_visibility_save","sourceId":id,"intent":{"mode":"hide-all"}})).is_err());
        service.request(&json!({"command":"controller_visibility_save","sourceId":id,"intent":{"mode":"block-original-input"}})).unwrap();
        service
            .request(&json!({"command":"controller_visibility_enable","enabled":true}))
            .unwrap();
        assert!(service
            .request(&json!({"command":"controller_visibility_apply","sourceIds":[id]}))
            .unwrap_err()
            .contains("disabled"));
        assert!(control.take().is_none());
        service.qualified = true;
        let result = service
            .request(&json!({"command":"controller_visibility_apply","sourceIds":[id]}))
            .unwrap();
        assert_eq!(result["visibility"]["job"]["active"], true);
        assert!(matches!(
            control.take(),
            Some(super::super::suppression::Command::Apply { .. })
        ));
        service
            .request(&json!({"command":"controller_visibility_restore"}))
            .unwrap();
        assert!(matches!(
            control.take(),
            Some(super::super::suppression::Command::Stop)
        ));
        control.update("stopped", "");
        service.request(&json!({"command":"controller_visibility_save","sourceId":id,"intent":{"mode":"visible"}})).unwrap();
        assert!(service
            .request(&json!({"command":"controller_visibility_apply","sourceIds":[id]}))
            .unwrap_err()
            .contains("no fallback"));
        assert!(service
            .request(&json!({"command":"controller_visibility_launch","pid":1}))
            .is_err());
    }
}

#[test]
fn isolated_launch_is_independent_gated_and_owned_by_the_service() {
    use super::super::launch;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::{Duration, Instant},
    };
    struct Child(Arc<AtomicUsize>);
    impl launch::Child for Child {
        fn poll(&mut self) -> Result<launch::Poll, String> {
            Ok(launch::Poll::Running)
        }
        fn stop(&mut self) -> Result<(), String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }
    let (registry, id) = inventory_for("linux-evdev");
    let dir = tempfile::tempdir().unwrap();
    let mut service = Service::new(Some(dir.path().into()), registry.clone());
    let request = json!({"command":"controller_visibility_launch","sourceIds":[id],"launch":{"launchKind":"direct-child","executable":"/opt/app/game","directory":"/opt/app","arguments":[]}});
    service.platform = super::super::capabilities::Platform::Macos;
    assert!(service
        .request(&request)
        .unwrap_err()
        .contains("Linux-only"));
    service.platform = super::super::capabilities::Platform::Linux;
    assert_eq!(
        service.status("")["visibility"]["launch"]["qualified"],
        false
    );
    assert!(service.request(&request).unwrap_err().contains("disabled"));
    assert!(dir.path().read_dir().unwrap().next().is_none());
    let mut invalid = request.clone();
    invalid["pid"] = json!(12);
    assert!(service
        .request(&invalid)
        .unwrap_err()
        .contains("Already-running"));
    let stops = Arc::new(AtomicUsize::new(0));
    let stopped = stops.clone();
    *service.launch.lock().unwrap() =
        launch::Worker::fixture(Arc::new(move |_, _| Ok(Box::new(Child(stopped.clone())))));
    assert!(service
        .request(&request)
        .unwrap_err()
        .contains("Explicitly enable"));
    service
        .request(&json!({"command":"controller_visibility_enable","enabled":true}))
        .unwrap();
    let mut invalid = request.clone();
    invalid["sourceIds"] = json!([]);
    assert!(service.request(&invalid).is_err());
    let status = service.request(&request).unwrap();
    assert_eq!(status["visibility"]["launch"]["job"]["active"], true);
    assert!(!service.suppression.active());
    assert!(service.request(&request).is_err());
    assert!(service
        .request(&json!({"command":"controller_visibility_enable","enabled":false}))
        .is_err());
    assert!(service
        .request(&json!({"command":"controller_visibility_restore","jobId":"stale"}))
        .is_err());
    let until = Instant::now() + Duration::from_secs(3);
    while service.status("")["visibility"]["launch"]["job"]["phase"] != "running" {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    service.request(&json!({"command":"controller_visibility_restore","jobId":status["visibility"]["launch"]["job"]["jobId"]})).unwrap();
    while service.active() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(stops.load(Ordering::SeqCst), 1);
    service.request(&request).unwrap();
    registry.lock().unwrap().disconnect(&id);
    while service.active() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!service.suppression.active());
}
