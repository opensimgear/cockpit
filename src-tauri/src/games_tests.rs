use super::*;
use crate::game_discovery::ProcessIdentity;
use std::time::{Duration, Instant};

const GATEWAY: &str = "stm32:00112233445566778899aabb";
const MODULE: &str = "00112233445566778899aabbccddeeff";

#[test]
fn legacy_game_settings_keep_profiles_defaults_and_installations_when_xplane_is_added() {
    let mut old = Saved::fresh().value();
    old["profiles"]
        .as_array_mut()
        .unwrap()
        .retain(|p| p["gameId"] == "dcs" || p["gameId"] == "assetto-corsa-rally");
    old["defaultProfiles"]
        .as_object_mut()
        .unwrap()
        .retain(|k, _| !k.starts_with("x-plane-"));
    old["manualInstallations"] = json!({"dcs":"/custom/DCS.exe"});
    let migrated = Saved::parse(&old).unwrap().value();
    assert_eq!(migrated["profiles"].as_array().unwrap().len(), 4);
    assert_eq!(
        migrated["defaultProfiles"]["dcs"],
        old["defaultProfiles"]["dcs"]
    );
    assert_eq!(migrated["manualInstallations"], old["manualInstallations"]);
    for prior in old["profiles"].as_array().unwrap() {
        assert!(migrated["profiles"].as_array().unwrap().contains(prior));
    }
    assert_eq!(Saved::parse(&migrated).unwrap().value(), migrated);
}
#[test]
fn full_legacy_profile_store_and_new_default_id_collisions_migrate_without_data_loss() {
    let mut old = Saved::fresh().value();
    old["profiles"]
        .as_array_mut()
        .unwrap()
        .retain(|p| p["gameId"] == "dcs" || p["gameId"] == "assetto-corsa-rally");
    old["defaultProfiles"]
        .as_object_mut()
        .unwrap()
        .retain(|k, _| !k.starts_with("x-plane-"));
    for i in 0..30 {
        let mut p = default_profile("dcs");
        p["id"] = json!(if i == 0 {
            "x-plane-11-default".into()
        } else {
            format!("old-{i}")
        });
        old["profiles"].as_array_mut().unwrap().push(p);
    }
    let migrated = Saved::parse(&old).unwrap().value();
    assert_eq!(migrated["profiles"].as_array().unwrap().len(), 34);
    for profile in old["profiles"].as_array().unwrap() {
        assert!(migrated["profiles"].as_array().unwrap().contains(profile));
    }
    assert_ne!(
        migrated["defaultProfiles"]["x-plane-11"],
        "x-plane-11-default"
    );
    assert_eq!(Saved::parse(&migrated).unwrap().value(), migrated);
}

#[test]
fn xplane_profiles_are_version_scoped_and_discard_transient_process_identity() {
    let mut value = default_profile("x-plane-11");
    let mut c = integration();
    c["document"]["provider"] = json!("x-plane");
    c["connection"] = crate::xplane::defaults(11);
    c["connection"]["processIdentity"] = json!({"pid":42,"path":"/fixture/X-Plane.exe","epoch":12});
    value["integration"] = c;
    let saved = profile(&value).unwrap();
    assert!(saved["integration"]["connection"]["processIdentity"].is_null());
    value["gameId"] = json!("x-plane-12");
    assert!(profile(&value).is_err());
    value["integration"]["connection"]["version"] = json!(12);
    assert!(profile(&value).is_ok());
}

struct FakePlatform {
    alive: AtomicBool,
    found: AtomicBool,
    fail: AtomicBool,
    launched: AtomicBool,
    launch_supported: AtomicBool,
}
impl FakePlatform {
    fn new() -> Self {
        Self {
            alive: AtomicBool::new(true),
            found: AtomicBool::new(false),
            fail: AtomicBool::new(false),
            launched: AtomicBool::new(false),
            launch_supported: AtomicBool::new(true),
        }
    }
    fn identity() -> ProcessIdentity {
        ProcessIdentity {
            pid: 42,
            path: "/games/DCS.exe".into(),
            epoch: 9,
        }
    }
}
impl Platform for FakePlatform {
    fn catalogue(&self, manual: &BTreeMap<String, String>) -> Vec<Value> {
        GAMES.iter().map(|g|json!({"id":g,"name":g,"installed":true,"launchSupported":self.launch_supported.load(Ordering::Acquire),"executable":manual.get(*g).map(String::as_str).unwrap_or("/games/DCS.exe"),"provider":if *g=="dcs"{"dcs-bios"}else{*g}})).collect()
    }
    fn validate(&self, _game: &str, path: &str) -> Result<(), String> {
        if path == "invalid" {
            Err("Invalid executable".into())
        } else {
            Ok(())
        }
    }
    fn launch(&self, _game: &Value) -> Result<ProcessIdentity, String> {
        self.launched.store(true, Ordering::Release);
        if self.fail.load(Ordering::Acquire) {
            Err("Game launch failed".into())
        } else {
            Ok(Self::identity())
        }
    }
    fn find(&self, _game: &Value) -> Result<Option<ProcessIdentity>, String> {
        Ok(self.found.load(Ordering::Acquire).then(Self::identity))
    }
    fn alive(&self, _identity: &ProcessIdentity) -> Result<bool, String> {
        Ok(self.alive.load(Ordering::Acquire))
    }
}
struct Mock {
    config: Value,
    enabled: bool,
    boot: u64,
    integration: Value,
    dcs: Value,
    calls: Vec<String>,
    offline: bool,
    fail: Option<String>,
    fail_after_write: Option<String>,
    fail_read_after_apply: bool,
    after_read: usize,
    profile_match: bool,
    layout_after_write: usize,
    layout_reads: usize,
    preflight_valid: bool,
    edit_on_claim: bool,
}
fn config(name: &str) -> Value {
    json!({"name":name,"expanderMask":0,"buttons":[[0,0,0]],"axes":[],"hats":[],"encoders":[],"selectors":[]})
}
fn mock() -> (Panel, Arc<Mutex<Mock>>) {
    let shared = Arc::new(Mutex::new(Mock {
        config: config("Baseline"),
        enabled: true,
        boot: 1,
        integration: json!({"running":false,"config":null}),
        dcs: json!({"running":false,"config":null}),
        calls: vec![],
        offline: false,
        fail: None,
        fail_after_write: None,
        fail_read_after_apply: false,
        after_read: 0,
        profile_match: true,
        layout_after_write: 0,
        layout_reads: 0,
        preflight_valid: true,
        edit_on_claim: false,
    }));
    let copy = shared.clone();
    let panel: Panel = Arc::new(move |r| {
        assert_eq!(r["clientId"], CLIENT);
        let mut m = copy.lock().unwrap();
        let command = r["command"].as_str().unwrap();
        m.calls.push(command.into());
        if m.fail.as_deref() == Some(command) {
            return json!({"ok":false,"error":format!("{command} failed")});
        }
        if m.offline && r["gatewayId"].is_string() {
            return json!({"ok":false,"error":"Peripheral offline"});
        }
        if command == "config_read" && m.after_read > 0 {
            m.after_read -= 1;
            return json!({"ok":false,"error":"Readback unavailable"});
        }
        match command {
            "claim" if m.edit_on_claim => m.config = config("External edit during claim"),
            "integration_preflight" => {
                m.integration["validation"] = json!({"valid":m.preflight_valid,"preview":true,"diagnostics":[{"message":"Catalogue or device capability unavailable"}]})
            }
            "integration_save" => m.integration["config"] = r["integrationConfig"].clone(),
            "integration_start" => m.integration["running"] = json!(true),
            "integration_stop" => m.integration["running"] = json!(false),
            "integration_clear" => m.integration["config"] = Value::Null,
            "dcs_start" => m.dcs["running"] = json!(true),
            "dcs_stop" => m.dcs["running"] = json!(false),
            "config_apply" => {
                m.config = r["config"].clone();
                if m.enabled {
                    m.profile_match = false;
                }
                if m.fail_read_after_apply {
                    m.offline = true;
                }
            }
            "enable" => {
                m.enabled = true;
                m.profile_match = true;
                m.layout_reads = m.layout_after_write;
            }
            "disable" => {
                m.enabled = false;
                m.profile_match = false;
                m.layout_reads = m.layout_after_write;
            }
            "update-profile" => {
                m.profile_match = true;
                m.layout_reads = m.layout_after_write;
            }
            _ => {}
        }
        if m.fail_after_write.as_deref() == Some(command) {
            m.fail_after_write = None;
            return json!({"ok":false,"error":"Acknowledgement lost"});
        }
        let pending = m.layout_reads > 0;
        if command == "config_read" && pending {
            m.layout_reads -= 1;
        }
        json!({"ok":true,"state":{"connected":true,"layoutPending":pending,"modules":[{"id":MODULE,"boot":m.boot,"online":true,"enabled":m.enabled,"profileMatch":m.profile_match}],
            "moduleConfigs":{MODULE:{"generation":1,"savedGeneration":1,"owner":0,"dirty":false,"pendingApply":false,"valid":true,"view":"active","config":m.config}},
            "integration":m.integration,"dcsBios":m.dcs}})
    });
    (panel, shared)
}
fn path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.research/games-tests")
        .join(uuid::Uuid::new_v4().to_string())
        .join("games.json")
}
fn setup() -> (GamesRuntime, Arc<Mutex<Mock>>, Arc<FakePlatform>, PathBuf) {
    let (panel, m) = mock();
    let platform = Arc::new(FakePlatform::new());
    let path = path();
    (
        GamesRuntime::new(panel, path.clone(), platform.clone()),
        m,
        platform,
        path,
    )
}
fn peripheral_profile(game: &str) -> Value {
    json!({"schemaVersion":1,"id":"custom","gameId":game,"name":"Custom","devices":[{"gatewayId":GATEWAY,"moduleId":MODULE,"controller":"disabled","config":config("Game mapping")}],"integration":null})
}
fn integration() -> Value {
    json!({"document":{"schemaVersion":1,"id":"rally-revs","name":"Rally revs","provider":"assetto-corsa-rally","context":{},"roles":[{"id":"lights","label":"Lights"}],"inputs":[],"outputs":[{"id":"rev","role":"lights","endpoint":"led.rgb","actionId":"led.rgb.set_color","channel":"rev_ratio","format":{"kind":"rgb","bands":[{"minimum":0.7,"color":"00FF00"}]} }]},"connection":{"source":"synthetic","freshnessMs":1500},"assignments":[{"role":"lights","gatewayId":GATEWAY,"moduleId":MODULE}]})
}
fn save(runtime: &GamesRuntime, p: Value) {
    runtime
        .request(&json!({"command":"game_profile_save","gameProfile":p}))
        .unwrap();
}
fn launch(runtime: &GamesRuntime, game: &str) {
    runtime.request(&json!({"command":"game_launch","gameId":game,"profileId":"custom","acceptBindingChanges":true})).unwrap();
}
fn wait(runtime: &GamesRuntime, phase: &str) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while runtime.status()["session"]["phase"] != phase && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        runtime.status()["session"]["phase"],
        phase,
        "{}",
        runtime.status()
    );
}
fn clean(path: &Path) {
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn profile_validation_rejects_unknown_fields_invalid_bounds_and_route_aliases() {
    let valid = peripheral_profile("dcs");
    assert_eq!(profile(&valid).unwrap(), valid);
    for (pointer, value) in [
        ("/schemaVersion", json!(4)),
        ("/id", json!("bad id")),
        ("/gameId", json!("unknown")),
        ("/name", json!("\n")),
        ("/devices/0/controller", json!("off")),
        ("/devices/0/gatewayId", json!("unknown:gateway")),
        ("/devices/0/moduleId", json!("0".repeat(32))),
        ("/devices/0/config/axes", json!([[0, 48, 4095, 0, 0, 0, 0]])),
    ] {
        let mut value_doc = valid.clone();
        *value_doc.pointer_mut(pointer).unwrap() = value;
        assert!(profile(&value_doc).is_err(), "{pointer}");
    }
    let mut invalid = valid.clone();
    invalid["private"] = json!(true);
    assert!(profile(&invalid).is_err());
    invalid = valid.clone();
    invalid["devices"][0]["config"]["private"] = json!(true);
    assert!(profile(&invalid).is_err());
    invalid = valid.clone();
    let duplicate = invalid["devices"][0].clone();
    invalid["devices"].as_array_mut().unwrap().push(duplicate);
    assert!(profile(&invalid).is_err());
    invalid = valid.clone();
    invalid["devices"] = json!(vec![valid["devices"][0].clone(); 33]);
    assert!(profile(&invalid).is_err());
    invalid = valid.clone();
    invalid["name"] = json!("X".repeat(81));
    assert!(profile(&invalid).is_err());
    invalid = valid;
    invalid["integration"] = integration();
    assert!(profile(&invalid).is_err());
    invalid["gameId"] = json!("assetto-corsa-rally");
    assert!(profile(&invalid).is_ok());
    invalid["integration"]["assignments"][0]["gatewayId"] = json!("pico:OSG-PICO-0011223344556677");
    assert!(profile(&invalid).is_ok());
}
#[test]
fn saves_roundtrip_defaults_delete_manual_selection_and_bad_requests() {
    let (runtime, _, _, path) = setup();
    assert_eq!(runtime.status()["profiles"].as_array().unwrap().len(), 4);
    save(&runtime, peripheral_profile("dcs"));
    runtime
        .request(&json!({"command":"game_profile_default","gameId":"dcs","profileId":"custom"}))
        .unwrap();
    assert!(runtime
        .request(&json!({"command":"game_profile_delete","profileId":"custom"}))
        .is_err());
    let (panel, _) = mock();
    let loaded = GamesRuntime::new(panel, path.clone(), Arc::new(FakePlatform::new()));
    assert_eq!(loaded.status()["defaultProfiles"]["dcs"], "custom");
    runtime
        .request(
            &json!({"command":"game_profile_default","gameId":"dcs","profileId":"dcs-default"}),
        )
        .unwrap();
    runtime
        .request(&json!({"command":"game_profile_delete","profileId":"custom"}))
        .unwrap();
    runtime
        .request(
            &json!({"command":"game_installation","gameId":"dcs","executable":"/custom/DCS.exe"}),
        )
        .unwrap();
    assert_eq!(
        runtime.status()["catalogue"][0]["executable"],
        "/custom/DCS.exe"
    );
    runtime
        .request(&json!({"command":"game_installation","gameId":"dcs","executable":null}))
        .unwrap();
    assert!(runtime
        .request(&json!({"command":"game_installation","gameId":"dcs","executable":"invalid"}))
        .is_err());
    runtime.request(&json!({"command":"game_scan"})).unwrap();
    runtime
        .request(&json!({"command":"game_get_state"}))
        .unwrap();
    for request in [
        json!({}),
        json!([]),
        json!({"command":"unknown"}),
        json!({"command":"game_profile_delete","profileId":"unknown"}),
        json!({"command":"game_profile_default","gameId":"dcs","profileId":"assetto-corsa-rally-default"}),
    ] {
        assert!(runtime.request(&request).is_err());
    }
    save(&runtime, peripheral_profile("dcs"));
    assert!(runtime.request(&json!({"command":"game_profile_save","gameProfile":peripheral_profile("assetto-corsa-rally")})).is_err());
    runtime.shutdown();
    assert!(runtime.request(&json!({"command":"game_scan"})).is_err());
    clean(&path);
}
#[test]
fn binding_changes_require_acceptance_before_any_write_or_launch() {
    let (runtime, m, platform, path) = setup();
    save(&runtime, peripheral_profile("dcs"));
    runtime
        .request(&json!({"command":"game_launch","gameId":"dcs","profileId":"custom"}))
        .unwrap();
    wait(&runtime, "failed");
    assert!(runtime.status()["session"]["bindingWarning"].is_string());
    assert!(!platform.launched.load(Ordering::Acquire));
    assert!(!m
        .lock()
        .unwrap()
        .calls
        .iter()
        .any(|c| c == "config_apply" || c == "disable"));
    assert!(!runtime.active());
    clean(&path);
}
#[test]
fn launch_applies_setup_and_process_exit_restores_without_saving_flash() {
    let (runtime, m, platform, path) = setup();
    save(&runtime, peripheral_profile("dcs"));
    launch(&runtime, "dcs");
    wait(&runtime, "running");
    assert_eq!(runtime.status()["session"]["pid"], 42);
    assert!(runtime.active());
    assert!(path.with_extension("recovery.json").exists());
    {
        let s = m.lock().unwrap();
        assert_eq!(s.config, config("Game mapping"));
        assert!(!s.enabled);
        assert!(!s.calls.iter().any(|c| c == "config_apply_save"));
    }
    assert!(runtime
        .guard_request(&json!({"command":"config_apply","gatewayId":GATEWAY,"moduleId":MODULE}))
        .is_err());
    for command in [
        "status",
        "config_read",
        "integration_get_state",
        "integration_override",
        "game_get_state",
    ] {
        assert!(runtime.guard_request(&json!({"command":command})).is_ok());
    }
    assert!(runtime.guard_request(&json!({"command":"connect"})).is_ok());
    assert!(runtime
        .request(&json!({"command":"game_launch","gameId":"dcs"}))
        .is_err());
    assert!(runtime.request(&json!({"command":"game_scan"})).is_err());
    platform.alive.store(false, Ordering::Release);
    wait(&runtime, "idle");
    {
        let s = m.lock().unwrap();
        assert_eq!(s.config, config("Baseline"));
        assert!(s.enabled);
        assert_eq!(
            s.calls
                .iter()
                .filter(|c| c.as_str() == "config_apply")
                .count(),
            2
        );
    }
    assert!(!path.with_extension("recovery.json").exists());
    assert!(!runtime.active());
    clean(&path);
}
#[test]
fn activation_adopts_existing_process_and_manual_deactivation_and_shutdown_restore() {
    let (runtime, m, platform, path) = setup();
    platform.found.store(true, Ordering::Release);
    save(&runtime, peripheral_profile("dcs"));
    launch(&runtime, "dcs");
    wait(&runtime, "running");
    assert!(!platform.launched.load(Ordering::Acquire));
    runtime
        .request(&json!({"command":"game_deactivate"}))
        .unwrap();
    wait(&runtime, "idle");
    runtime.request(&json!({"command":"game_activate","gameId":"dcs","profileId":"custom","acceptBindingChanges":true})).unwrap();
    wait(&runtime, "running");
    runtime.shutdown();
    assert_eq!(runtime.status()["session"]["phase"], "idle");
    assert_eq!(m.lock().unwrap().config, config("Baseline"));
    clean(&path);
}
#[test]
fn launch_failure_and_lost_write_reply_restore_once() {
    let (runtime, m, platform, path) = setup();
    platform.fail.store(true, Ordering::Release);
    m.lock().unwrap().fail_after_write = Some("config_apply".into());
    save(&runtime, peripheral_profile("dcs"));
    launch(&runtime, "dcs");
    wait(&runtime, "failed");
    assert!(runtime.status()["session"]["error"]
        .as_str()
        .unwrap()
        .contains("launch failed"));
    assert_eq!(m.lock().unwrap().config, config("Baseline"));
    assert_eq!(
        m.lock()
            .unwrap()
            .calls
            .iter()
            .filter(|c| c.as_str() == "config_apply")
            .count(),
        2
    );
    assert!(!runtime.active());
    clean(&path);
}
#[test]
fn offline_failure_keeps_journal_and_explicit_recovery_reconciles_current_state() {
    let (runtime, m, _, path) = setup();
    m.lock().unwrap().fail_read_after_apply = true;
    save(&runtime, peripheral_profile("dcs"));
    launch(&runtime, "dcs");
    wait(&runtime, "recovery-needed");
    assert!(runtime.active());
    assert!(runtime.guard_request(&json!({"command":"connect"})).is_ok());
    {
        let mut m = m.lock().unwrap();
        m.offline = false;
        m.fail_read_after_apply = false;
    }
    runtime.request(&json!({"command":"game_recover"})).unwrap();
    wait(&runtime, "idle");
    assert_eq!(m.lock().unwrap().config, config("Baseline"));
    clean(&path);
}
#[test]
fn recovery_preserves_external_edits_and_new_module_boots() {
    let (runtime, m, _, path) = setup();
    save(&runtime, peripheral_profile("dcs"));
    launch(&runtime, "dcs");
    wait(&runtime, "running");
    m.lock().unwrap().config = config("External edit");
    runtime
        .request(&json!({"command":"game_deactivate"}))
        .unwrap();
    wait(&runtime, "recovery-needed");
    assert_eq!(m.lock().unwrap().config, config("External edit"));
    runtime.request(&json!({"command":"game_recover"})).unwrap();
    wait(&runtime, "recovery-needed");
    {
        let mut m = m.lock().unwrap();
        m.config = config("Game mapping");
        m.boot = 2;
    }
    runtime.request(&json!({"command":"game_recover"})).unwrap();
    wait(&runtime, "recovery-needed");
    assert_eq!(m.lock().unwrap().config, config("Game mapping"));
    {
        let mut m = m.lock().unwrap();
        m.boot = 1;
    }
    runtime.request(&json!({"command":"game_recover"})).unwrap();
    wait(&runtime, "idle");
    clean(&path);
}
#[test]
fn restart_never_replays_journal_and_recovery_restores_captured_integration() {
    let (runtime, m, _, path) = setup();
    let mut p = peripheral_profile("assetto-corsa-rally");
    p["integration"] = integration();
    {
        let mut m = m.lock().unwrap();
        m.dcs = json!({"config":{"schemaVersion":2},"running":true});
    }
    save(&runtime, p);
    launch(&runtime, "assetto-corsa-rally");
    wait(&runtime, "running");
    assert!(m.lock().unwrap().integration["running"] == true);
    let journal = fs::read(path.with_extension("recovery.json")).unwrap();
    runtime.shutdown();
    fs::write(path.with_extension("recovery.json"), journal).unwrap();
    {
        let mut m = m.lock().unwrap();
        m.config = config("Game mapping");
        m.enabled = false;
        m.integration = json!({"config":integration(),"running":false});
        m.calls.clear();
    }
    let (panel, new_m) = mock();
    {
        let old = m.lock().unwrap();
        let mut new = new_m.lock().unwrap();
        new.config = old.config.clone();
        new.enabled = old.enabled;
        new.integration = old.integration.clone();
        new.dcs = old.dcs.clone();
    }
    let restarted = GamesRuntime::new(panel, path.clone(), Arc::new(FakePlatform::new()));
    assert_eq!(restarted.status()["session"]["phase"], "recovery-needed");
    assert!(new_m.lock().unwrap().calls.is_empty());
    restarted
        .request(&json!({"command":"game_recover"}))
        .unwrap();
    wait(&restarted, "idle");
    assert!(new_m.lock().unwrap().integration["config"].is_null());
    assert_eq!(new_m.lock().unwrap().dcs["running"], true);
    clean(&path);
}
#[test]
fn rally_live_integration_uses_actual_game_pid_after_launch() {
    let (runtime, m, platform, path) = setup();
    let mut p = peripheral_profile("assetto-corsa-rally");
    let mut c = integration();
    c["connection"]["source"] = json!("shared-memory");
    c["connection"].as_object_mut().unwrap().remove("processId");
    p["integration"] = c;
    save(&runtime, p);
    assert!(runtime.status()["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "custom")
        .unwrap()["integration"]["connection"]["processId"]
        .is_null());
    launch(&runtime, "assetto-corsa-rally");
    wait(&runtime, "running");
    assert!(platform.launched.load(Ordering::Acquire));
    assert_eq!(
        m.lock().unwrap().integration["config"]["connection"]["processId"],
        42
    );
    runtime
        .request(&json!({"command":"game_deactivate"}))
        .unwrap();
    wait(&runtime, "idle");
    clean(&path);
}
#[test]
fn xplane_launch_uses_a_fresh_process_identity_and_exit_restores_the_session() {
    for version in [11, 12] {
        let (runtime, m, platform, path) = setup();
        let game = format!("x-plane-{version}");
        let mut p = peripheral_profile(&game);
        let mut c = integration();
        c["document"]["provider"] = json!("x-plane");
        c["document"]["outputs"][0]["channel"] = json!("airspeed_kts");
        c["connection"] = crate::xplane::defaults(version);
        p["integration"] = c;
        save(&runtime, p);
        assert!(!platform.launched.load(Ordering::Acquire));
        assert!(!m
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c == "integration_start"));
        launch(&runtime, &game);
        wait(&runtime, "running");
        assert_eq!(
            m.lock().unwrap().integration["config"]["connection"]["processIdentity"],
            FakePlatform::identity().value()
        );
        platform.alive.store(false, Ordering::Release);
        wait(&runtime, "idle");
        assert_eq!(m.lock().unwrap().config, config("Baseline"));
        assert!(!path.with_extension("recovery.json").exists());
        clean(&path);
    }
}
#[test]
fn changed_mapping_with_enabled_controller_publishes_and_restores_hid_profiles() {
    let (runtime, m, _, path) = setup();
    let mut p = peripheral_profile("dcs");
    p["devices"][0]["controller"] = json!("keep");
    m.lock().unwrap().layout_after_write = 3;
    save(&runtime, p);
    launch(&runtime, "dcs");
    wait(&runtime, "running");
    {
        let m = m.lock().unwrap();
        assert!(m.enabled && m.profile_match);
        assert_eq!(
            m.calls
                .iter()
                .filter(|c| c.as_str() == "update-profile")
                .count(),
            1
        );
    }
    runtime
        .request(&json!({"command":"game_deactivate"}))
        .unwrap();
    wait(&runtime, "idle");
    {
        let m = m.lock().unwrap();
        assert!(m.enabled && m.profile_match);
        assert_eq!(m.config, config("Baseline"));
        assert_eq!(
            m.calls
                .iter()
                .filter(|c| c.as_str() == "update-profile")
                .count(),
            2
        );
    }
    clean(&path);
}
#[test]
fn readback_waits_for_layout_and_missing_acknowledgements_without_replaying_controller_changes() {
    let (runtime, m, _, path) = setup();
    {
        let mut m = m.lock().unwrap();
        m.layout_after_write = 3;
        m.fail_after_write = Some("disable".into());
    }
    save(&runtime, peripheral_profile("dcs"));
    launch(&runtime, "dcs");
    wait(&runtime, "running");
    runtime
        .request(&json!({"command":"game_deactivate"}))
        .unwrap();
    wait(&runtime, "idle");
    assert_eq!(
        m.lock()
            .unwrap()
            .calls
            .iter()
            .filter(|c| c.as_str() == "disable")
            .count(),
        1
    );
    assert_eq!(
        m.lock()
            .unwrap()
            .calls
            .iter()
            .filter(|c| c.as_str() == "enable")
            .count(),
        1
    );
    clean(&path);
}
#[test]
fn preexisting_hid_mismatch_rejected_and_synthetic_activation_works_without_native_launching() {
    let (runtime, m, platform, path) = setup();
    m.lock().unwrap().profile_match = false;
    save(&runtime, peripheral_profile("dcs"));
    launch(&runtime, "dcs");
    wait(&runtime, "failed");
    assert!(!platform.launched.load(Ordering::Acquire));
    m.lock().unwrap().profile_match = true;
    platform.launch_supported.store(false, Ordering::Release);
    runtime.request(&json!({"command":"game_scan"})).unwrap();
    assert!(runtime
        .request(&json!({"command":"game_launch","gameId":"dcs"}))
        .is_err());
    runtime.request(&json!({"command":"game_activate","gameId":"dcs","profileId":"custom","acceptBindingChanges":true})).unwrap();
    wait(&runtime, "running");
    assert!(runtime.status()["session"]["pid"].is_null());
    runtime
        .request(&json!({"command":"game_deactivate"}))
        .unwrap();
    wait(&runtime, "idle");
    clean(&path);
}
#[test]
fn runtime_config_preflight_rejects_overlaps_expansion_and_duplicate_axis_channels() {
    let mut value = config("Name");
    value["buttons"] = json!([[24, 0, 0]]);
    assert!(validate_config(&value).is_err());
    value["expanderMask"] = json!(1);
    assert!(validate_config(&value).is_ok());
    value["buttons"] = json!([[1, 1, 1]]);
    assert!(validate_config(&value).is_err());
    value["buttons"] = json!([[1, 0, 0]]);
    value["encoders"] = json!([[1, 2]]);
    assert!(validate_config(&value).is_err());
    value["encoders"] = json!([]);
    value["axes"] = json!([[0, 48, 0, 4095, 0, 0, 0], [0, 49, 0, 4095, 0, 0, 0]]);
    assert!(validate_config(&value).is_err());
    value["axes"][1][0] = json!(1);
    value["axes"][1][1] = json!(48);
    assert!(validate_config(&value).is_err());
    value["axes"][1][1] = json!(49);
    assert!(validate_config(&value).is_ok());
    let mut p = peripheral_profile("dcs");
    let mut row = p["devices"][0].clone();
    row["gatewayId"] = json!("pico:OSG-PICO-0011223344556677");
    p["devices"].as_array_mut().unwrap().push(row);
    assert!(profile(&p).is_ok());
}
#[test]
fn bad_integration_preflight_fails_before_stopping_integrations_or_mutating_devices() {
    let (runtime, m, platform, path) = setup();
    m.lock().unwrap().preflight_valid = false;
    let mut p = peripheral_profile("assetto-corsa-rally");
    p["integration"] = integration();
    save(&runtime, p);
    launch(&runtime, "assetto-corsa-rally");
    wait(&runtime, "failed");
    assert_eq!(
        runtime.status()["session"]["error"],
        "Catalogue or device capability unavailable"
    );
    assert!(!m
        .lock()
        .unwrap()
        .calls
        .iter()
        .any(|c| c == "integration_stop" || c == "config_apply"));
    assert!(!platform.launched.load(Ordering::Acquire));
    assert!(!path.with_extension("recovery.json").exists());
    clean(&path);
}
#[test]
fn output_only_devices_and_implicit_integration_targets_need_no_controller_mutation() {
    let (runtime, m, _, path) = setup();
    {
        let mut m = m.lock().unwrap();
        m.enabled = false;
        m.profile_match = false;
        m.config = json!({"name":"Lights","expanderMask":0,"buttons":[],"axes":[],"hats":[],"encoders":[],"selectors":[]});
    }
    let mut p = peripheral_profile("assetto-corsa-rally");
    p["devices"] = json!([]);
    p["integration"] = integration();
    save(&runtime, p);
    runtime
        .request(
            &json!({"command":"game_launch","gameId":"assetto-corsa-rally","profileId":"custom"}),
        )
        .unwrap();
    wait(&runtime, "running");
    assert!(m.lock().unwrap().integration["running"] == true);
    assert!(!m
        .lock()
        .unwrap()
        .calls
        .iter()
        .any(|c| c == "enable" || c == "disable" || c == "config_apply"));
    runtime
        .request(&json!({"command":"game_deactivate"}))
        .unwrap();
    wait(&runtime, "idle");
    assert!(!m.lock().unwrap().enabled);
    clean(&path);
}
#[test]
fn previous_saved_running_integration_is_restored_and_external_integration_edits_preserved() {
    let (runtime, m, _, path) = setup();
    let mut baseline = integration();
    baseline["document"]["id"] = json!("previous");
    baseline["document"]["name"] = json!("Previous integration");
    m.lock().unwrap().integration = json!({"config":baseline,"running":true});
    let mut p = peripheral_profile("assetto-corsa-rally");
    p["integration"] = integration();
    save(&runtime, p);
    launch(&runtime, "assetto-corsa-rally");
    wait(&runtime, "running");
    runtime
        .request(&json!({"command":"game_deactivate"}))
        .unwrap();
    wait(&runtime, "idle");
    assert_eq!(m.lock().unwrap().integration["config"], baseline);
    assert_eq!(m.lock().unwrap().integration["running"], true);
    launch(&runtime, "assetto-corsa-rally");
    wait(&runtime, "running");
    let mut outside = integration();
    outside["document"]["id"] = json!("external");
    m.lock().unwrap().integration["config"] = outside.clone();
    runtime
        .request(&json!({"command":"game_deactivate"}))
        .unwrap();
    wait(&runtime, "recovery-needed");
    assert_eq!(m.lock().unwrap().integration["config"], outside);
    m.lock().unwrap().integration["config"] = integration();
    runtime.request(&json!({"command":"game_recover"})).unwrap();
    wait(&runtime, "idle");
    assert_eq!(m.lock().unwrap().integration["config"], baseline);
    clean(&path);
}
#[test]
fn abandoning_recovery_requires_confirmation_archives_exact_bytes_and_keeps_current_setup() {
    let (runtime, m, _, path) = setup();
    save(&runtime, peripheral_profile("dcs"));
    launch(&runtime, "dcs");
    wait(&runtime, "running");
    assert!(runtime
        .request(&json!({"command":"game_abandon","preserveCurrentSetup":true}))
        .is_err());
    m.lock().unwrap().config = config("External edit");
    runtime
        .request(&json!({"command":"game_deactivate"}))
        .unwrap();
    wait(&runtime, "recovery-needed");
    let journal = fs::read(path.with_extension("recovery.json")).unwrap();
    let calls = m.lock().unwrap().calls.len();
    assert!(runtime.request(&json!({"command":"game_abandon"})).is_err());
    runtime
        .request(&json!({"command":"game_abandon","preserveCurrentSetup":true}))
        .unwrap();
    assert_eq!(runtime.status()["session"]["phase"], "idle");
    assert!(!runtime.active());
    assert_eq!(m.lock().unwrap().calls.len(), calls);
    assert_eq!(m.lock().unwrap().config, config("External edit"));
    let archive = runtime.status()["session"]["recoveryArchive"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(fs::read(archive).unwrap(), journal);
    assert!(!path.with_extension("recovery.json").exists());
    assert!(runtime
        .request(&json!({"command":"game_abandon","preserveCurrentSetup":true}))
        .is_err());
    clean(&path);
}
#[test]
fn restarted_device_already_back_to_baseline_is_recovered_without_hardware_writes() {
    let (runtime, m, _, path) = setup();
    save(&runtime, peripheral_profile("dcs"));
    launch(&runtime, "dcs");
    wait(&runtime, "running");
    {
        let mut m = m.lock().unwrap();
        m.boot = 2;
        m.config = config("Baseline");
        m.enabled = true;
        m.profile_match = true;
    }
    let writes_before = m
        .lock()
        .unwrap()
        .calls
        .iter()
        .filter(|c| {
            c.as_str() == "config_apply" || c.as_str() == "enable" || c.as_str() == "disable"
        })
        .count();
    runtime
        .request(&json!({"command":"game_deactivate"}))
        .unwrap();
    wait(&runtime, "idle");
    let writes_after = m
        .lock()
        .unwrap()
        .calls
        .iter()
        .filter(|c| {
            c.as_str() == "config_apply" || c.as_str() == "enable" || c.as_str() == "disable"
        })
        .count();
    assert_eq!(writes_before, writes_after);
    clean(&path);
}
#[test]
fn invalid_settings_and_journals_are_visible_and_cannot_overwrite_recovery() {
    let path = path();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"invalid").unwrap();
    let (panel, _) = mock();
    let runtime = GamesRuntime::new(panel, path.clone(), Arc::new(FakePlatform::new()));
    assert!(runtime.status()["error"].is_string());
    fs::write(path.with_extension("recovery.json"), b"invalid").unwrap();
    let (panel, _) = mock();
    let blocked = GamesRuntime::new(panel, path.clone(), Arc::new(FakePlatform::new()));
    assert!(blocked.active());
    assert!(blocked.request(&json!({"command":"game_recover"})).is_err());
    assert!(blocked
        .request(&json!({"command":"game_profile_save","gameProfile":peripheral_profile("dcs")}))
        .is_err());
    assert_eq!(
        fs::read(path.with_extension("recovery.json")).unwrap(),
        b"invalid"
    );
    blocked
        .request(&json!({"command":"game_abandon","preserveCurrentSetup":true}))
        .unwrap();
    assert!(!blocked.active());
    let archive = blocked.status()["session"]["recoveryArchive"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(fs::read(archive).unwrap(), b"invalid");
    clean(&path);
}
#[test]
fn external_edit_between_snapshot_and_claim_is_preserved_without_config_apply() {
    let (runtime, m, _, path) = setup();
    m.lock().unwrap().edit_on_claim = true;
    save(&runtime, peripheral_profile("dcs"));
    launch(&runtime, "dcs");
    wait(&runtime, "recovery-needed");
    assert_eq!(
        m.lock().unwrap().config,
        config("External edit during claim")
    );
    assert!(!m.lock().unwrap().calls.iter().any(|c| c == "config_apply"));
    runtime
        .request(&json!({"command":"game_abandon","preserveCurrentSetup":true}))
        .unwrap();
    clean(&path);
}
#[test]
fn setup_and_integration_errors_restore_acknowledged_changes() {
    let (runtime, m, _, path) = setup();
    save(&runtime, peripheral_profile("dcs"));
    m.lock().unwrap().fail = Some("claim".into());
    launch(&runtime, "dcs");
    wait(&runtime, "failed");
    assert_eq!(m.lock().unwrap().config, config("Baseline"));
    m.lock().unwrap().fail = Some("integration_start".into());
    let mut p = peripheral_profile("assetto-corsa-rally");
    p["integration"] = integration();
    runtime
        .request(&json!({"command":"game_profile_delete","profileId":"custom"}))
        .unwrap();
    save(&runtime, p);
    launch(&runtime, "assetto-corsa-rally");
    wait(&runtime, "failed");
    assert_eq!(m.lock().unwrap().config, config("Baseline"));
    assert!(m.lock().unwrap().integration["config"].is_null());
    clean(&path);
}
#[test]
fn saved_and_journal_parsers_reject_bad_defaults_duplicates_and_untrusted_shapes() {
    let fresh = Saved::fresh().value();
    assert!(Saved::parse(&fresh).is_ok());
    for value in [
        json!({}),
        json!({"schemaVersion":2}),
        json!({"schemaVersion":1,"profiles":[],"defaultProfiles":{},"manualInstallations":{}}),
    ] {
        assert!(Saved::parse(&value).is_err());
    }
    let mut duplicate = fresh.clone();
    duplicate["profiles"]
        .as_array_mut()
        .unwrap()
        .push(default_profile("dcs"));
    assert!(Saved::parse(&duplicate).is_err());
    let mut wrong = fresh.clone();
    wrong["defaultProfiles"]["dcs"] = json!("assetto-corsa-rally-default");
    assert!(Saved::parse(&wrong).is_err());
    wrong = fresh;
    wrong["manualInstallations"] = json!({"unknown":"path"});
    assert!(Saved::parse(&wrong).is_err());
    assert!(validate_journal(json!({"schemaVersion":1,"gameId":"dcs","profileId":"custom","devices":[],"integrationTaken":null})).is_err());
}

#[test]
fn overview_detects_external_games_without_activating_a_profile() {
    let (runtime, hardware, platform, path) = setup();
    assert!(runtime.status()["detectedGames"]
        .as_array()
        .unwrap()
        .is_empty());
    platform.found.store(true, Ordering::Release);
    let status = runtime.status();
    assert_eq!(status["session"]["phase"], "idle");
    assert_eq!(status["detectedGames"].as_array().unwrap().len(), 4);
    assert_eq!(status["detectedGames"][0]["pid"], 42);
    assert!(status["detectionErrors"].as_array().unwrap().is_empty());
    assert!(!platform.launched.load(Ordering::Acquire));
    assert!(hardware.lock().unwrap().calls.is_empty());
    platform.found.store(false, Ordering::Release);
    assert!(runtime.status()["detectedGames"]
        .as_array()
        .unwrap()
        .is_empty());
    clean(&path);
}

include!("games_mapping_tests.rs");

#[path = "game_companion_tests.rs"]
mod companions;
