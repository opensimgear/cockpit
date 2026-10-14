use super::super::{journal::Backend, policy::tests::state, runtime::Adapter};
use super::*;
use crate::registry::{Control, Definition, Identity};
use std::collections::BTreeMap;
fn inventory() -> (Arc<Mutex<Registry>>, String) {
    let mut registry = Registry::default();
    let id = registry
        .connect(Definition {
            identity: Identity {
                backend: "windows-raw-input".into(),
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
fn default_status_and_saved_intent_are_inert_without_optional_dependencies() {
    let (registry, id) = inventory();
    let temp = tempfile::tempdir().unwrap();
    let mut service = Service::new(Some(temp.path().into()), registry.clone());
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
