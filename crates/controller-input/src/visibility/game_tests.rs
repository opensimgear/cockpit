use super::*;
use std::time::{Duration, Instant};
fn setup() -> (
    Service,
    tempfile::TempDir,
    Arc<Mutex<State>>,
    Arc<Mutex<Registry>>,
    String,
) {
    let (registry, id) = inventory();
    let temp = tempfile::tempdir().unwrap();
    let shared = Arc::new(Mutex::new(state()));
    let cloned = shared.clone();
    let mut service = Service::new(Some(temp.path().into()), registry.clone());
    service.platform = super::super::super::capabilities::Platform::Windows;
    service.qualified = true;
    service.factory = Arc::new(move || Ok(Box::new(Fixture(cloned.clone()))));
    service
        .request(&json!({"command":"controller_visibility_enable","enabled":true}))
        .unwrap();
    service.request(&json!({"command":"controller_visibility_save","sourceId":id,"intent":{"mode":"hide-all"}})).unwrap();
    (service, temp, shared, registry, id)
}
fn captured(s: &Service, id: &str) -> Value {
    s.game_request(
        "capture",
        &json!({"mode":"hidhide-session","sourceIds":[id]}),
    )
    .unwrap()
}
fn wait(
    s: &Service,
    request: &Value,
    registry: &Arc<Mutex<Registry>>,
    id: &str,
    active: bool,
) -> Value {
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        registry
            .lock()
            .unwrap()
            .report(id, 1, &BTreeMap::from([(0, 0)]))
            .unwrap();
        let status = s.game_request("poll", request).unwrap();
        if if active {
            status["phase"] == "active"
        } else {
            status["active"] == false
        } {
            return status;
        }
        assert!(Instant::now() < until, "{status}");
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn captured_policies_are_immutable_and_owned_stop_preserves_saved_settings() {
    let (s, _temp, driver, registry, id) = setup();
    let intent = captured(&s, &id);
    s.request(
        &json!({"command":"controller_visibility_save","sourceId":id,"intent":{"mode":"visible"}}),
    )
    .unwrap();
    let settings = s.settings.lock().unwrap().value.clone();
    let prepared = s.game_request("preflight", &intent).unwrap();
    let request = json!({"owner":uuid::Uuid::new_v4().to_string(),"prepared":prepared});
    let before = driver.lock().unwrap().clone();
    s.game_request("start", &request).unwrap();
    wait(&s, &request, &registry, &id, true);
    assert!(driver.lock().unwrap().active);
    assert_eq!(s.settings.lock().unwrap().value, settings);
    assert!(s.game_request("preflight", &intent).is_err());
    assert!(s.game_request("start", &request).is_err());
    assert!(s
        .request(&json!({"command":"controller_visibility_enable","enabled":false}))
        .is_err());
    let other = json!({"owner":uuid::Uuid::new_v4().to_string()});
    assert!(s
        .game_request("stop", &other)
        .unwrap_err()
        .contains("Another"));
    assert!(s.game_request("poll", &other).is_err());
    s.game_request("stop", &request).unwrap();
    wait(&s, &request, &registry, &id, false);
    assert_eq!(
        s.game_request("stop", &request).unwrap()["phase"],
        "restored"
    );
    assert_eq!(*driver.lock().unwrap(), before);
    assert_eq!(s.settings.lock().unwrap().value, settings);
    assert!(!s.active());
}
#[test]
fn unqualified_or_mismatched_intent_cannot_change_policy() {
    let (mut s, temp, driver, _registry, id) = setup();
    let intent = captured(&s, &id);
    let before = driver.lock().unwrap().clone();
    s.qualified = false;
    assert!(s
        .game_request("preflight", &intent)
        .unwrap_err()
        .contains("not qualified"));
    let request = json!({"owner":uuid::Uuid::new_v4().to_string(),"prepared":{}});
    assert!(s.game_request("start", &request).is_err());
    s.qualified = true;
    s.request(&json!({"command":"controller_visibility_enable","enabled":false}))
        .unwrap();
    assert!(s.game_request("preflight", &intent).is_err());
    s.request(&json!({"command":"controller_visibility_enable","enabled":true}))
        .unwrap();
    for invalid in [
        json!({}),
        json!({"mode":"hidhide-session","sourceIds":[id,id]}),
        json!({"mode":"hidhide-session","sourceIds":["bad"]}),
    ] {
        assert!(s.game_request("capture", &invalid).is_err());
    }
    let mut invalid = intent.clone();
    invalid["mode"] = json!("controlled-launch");
    assert!(s
        .game_request("preflight", &invalid)
        .unwrap_err()
        .contains("separate owned launcher"));
    invalid = intent.clone();
    invalid["policies"] = json!({});
    assert!(s.game_request("preflight", &invalid).is_err());
    invalid = intent.clone();
    invalid["policies"][&id] = json!({"mode":"block-original-input"});
    assert!(s.game_request("preflight", &invalid).is_err());
    invalid = intent.clone();
    invalid["policies"][&id] = json!({"mode":"hide-from-process","executable":"relative.exe"});
    assert!(s.game_request("preflight", &invalid).is_err());
    assert!(s
        .game_request("poll", &json!({"owner":"../escape"}))
        .is_err());
    assert!(s.game_request("unknown", &request).is_err());
    assert_eq!(*driver.lock().unwrap(), before);
    assert!(!temp.path().read_dir().unwrap().any(|p| p
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with("game-visibility")));
}
#[test]
fn policy_change_after_preflight_and_external_change_during_run_are_preserved() {
    let (s, _temp, driver, registry, id) = setup();
    let intent = captured(&s, &id);
    let prepared = s.game_request("preflight", &intent).unwrap();
    let request = json!({"owner":uuid::Uuid::new_v4().to_string(),"prepared":prepared});
    driver.lock().unwrap().hidden.push(r"HID\EXTERNAL\2".into());
    let external = driver.lock().unwrap().clone();
    assert!(s.game_request("start", &request).is_err());
    assert_eq!(*driver.lock().unwrap(), external);
    driver.lock().unwrap().hidden.clear();
    let prepared = s.game_request("preflight", &intent).unwrap();
    let request = json!({"owner":uuid::Uuid::new_v4().to_string(),"prepared":prepared});
    s.game_request("start", &request).unwrap();
    wait(&s, &request, &registry, &id, true);
    driver.lock().unwrap().hidden.push(r"HID\EXTERNAL\2".into());
    let external = driver.lock().unwrap().clone();
    s.game_request("stop", &request).unwrap();
    wait(&s, &request, &registry, &id, false);
    assert!(s
        .game_request("stop", &request)
        .unwrap_err()
        .contains("attention"));
    assert_eq!(*driver.lock().unwrap(), external);
    s.game_request("abandon", &request).unwrap();
    assert!(
        s.active(),
        "An abandoned dirty journal still blocks a new activation"
    );
}
#[test]
fn ephemeral_suppression_is_scoped_and_never_restored_by_reapplying_it() {
    for (platform, backend, mode) in [
        (
            super::super::super::capabilities::Platform::Linux,
            "linux-evdev",
            "linux-grab",
        ),
        (
            super::super::super::capabilities::Platform::Macos,
            "macos-iohid",
            "macos-seize",
        ),
    ] {
        let (registry, id) = inventory_for(backend);
        let temp = tempfile::tempdir().unwrap();
        let mut s = Service::new(Some(temp.path().into()), registry);
        s.platform = platform;
        s.qualified = true;
        s.request(&json!({"command":"controller_visibility_enable","enabled":true}))
            .unwrap();
        s.request(&json!({"command":"controller_visibility_save","sourceId":id,"intent":{"mode":"block-original-input"}})).unwrap();
        let intent = s
            .game_request("capture", &json!({"mode":mode,"sourceIds":[id]}))
            .unwrap();
        let prepared = s.game_request("preflight", &intent).unwrap();
        let request = json!({"owner":uuid::Uuid::new_v4().to_string(),"prepared":prepared});
        s.game_request("start", &request).unwrap();
        assert!(matches!(
            s.suppression.take(),
            Some(super::super::super::suppression::Command::Apply { .. })
        ));
        s.suppression.update("active", "");
        assert_eq!(s.game_request("poll", &request).unwrap()["phase"], "active");
        s.game_request("stop", &request).unwrap();
        assert!(matches!(
            s.suppression.take(),
            Some(super::super::super::suppression::Command::Stop)
        ));
        s.suppression.update("stopped", "");
        assert_eq!(
            s.game_request("stop", &request).unwrap()["phase"],
            "restored"
        );
        assert_eq!(
            s.game_request("stop", &request).unwrap()["phase"],
            "restored"
        );
        assert!(s.suppression.take().is_none());
    }
}
#[test]
fn restart_reports_owned_hiding_and_only_explicit_recovery_restores_it() {
    let (s, temp, driver, registry, id) = setup();
    let intent = captured(&s, &id);
    let prepared = s.game_request("preflight", &intent).unwrap();
    let owner = uuid::Uuid::new_v4().to_string();
    let path = temp.path().join(format!("game-visibility-{owner}.json"));
    let before = driver.lock().unwrap().clone();
    let mut store = journal::Store::open(path.clone()).unwrap();
    let plan = serde_json::from_value(prepared["policy"].clone()).unwrap();
    journal::apply(&mut Fixture(driver.clone()), &mut store, plan, || Ok(())).unwrap();
    drop(store);
    // This represents a previous process's durable applied state; discovery
    // reports it but never restores or reapplies anything automatically.
    assert!(s.active());
    assert!(s.status("")["visibility"]["recovery"]["error"]
        .as_str()
        .unwrap()
        .contains("Games"));
    assert!(driver.lock().unwrap().active);
    assert!(s.game_request("preflight", &intent).is_err());
    s.settings.lock().unwrap().value.enabled = false;
    let request = json!({"owner":owner});
    s.game_request("stop", &request).unwrap();
    wait(&s, &request, &registry, &id, false);
    s.game_request("stop", &request).unwrap();
    assert_eq!(*driver.lock().unwrap(), before);
    assert!(journal::inspect(&path).unwrap().unwrap().clean);
    assert!(!s.active());
}

#[path="game_handoff_tests.rs"]
mod handoff_tests;
