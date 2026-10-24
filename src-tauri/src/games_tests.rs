use super::*;
use crate::game_discovery::ProcessIdentity;
use std::collections::BTreeMap;
use std::sync::atomic::AtomicUsize;
const GATEWAY: &str = "stm32:00112233445566778899aabb";
const MODULE: &str = "00112233445566778899aabbccddeeff";
const OWNER: &str = "11111111-1111-4111-8111-111111111111";
#[derive(Default)]
struct FakePlatform {
    calls: AtomicUsize,
}
impl Platform for FakePlatform {
    fn catalogue(&self, manual: &BTreeMap<String, String>) -> Vec<Value> {
        GAMES.iter().map(|id|json!({"id":id,"name":id,"provider":if *id=="dcs"{"dcs-bios"}else{*id},"installed":true,"executable":manual.get(*id).cloned().unwrap_or_else(||format!("C:\\Games\\{id}\\game.exe")),"launchSupported":true,"processMonitoringSupported":false})).collect()
    }
    fn validate(&self, _: &str, path: &str) -> Result<(), String> {
        if path.starts_with("C:\\") || Path::new(path).is_absolute() {
            Ok(())
        } else {
            Err("Invalid executable".into())
        }
    }
    fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> {
        panic!("Retired launcher must never execute")
    }
    fn spawn_process(&self, _: &Value) -> Result<(), String> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        panic!("Metadata/cleanup must never spawn")
    }
    fn find(&self, _: &Value) -> Result<Option<ProcessIdentity>, String> {
        Ok(None)
    }
    fn alive(&self, _: &ProcessIdentity) -> Result<bool, String> {
        Ok(false)
    }
}
fn no_io() -> CleanupIo {
    Arc::new(|request| panic!("No cleanup call expected: {request}"))
}
fn wait(runtime: &GamesRuntime) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while runtime.inner.busy.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
}
fn journal() -> Value {
    json!({"schemaVersion":3,"gameId":"dcs","profileId":"old-owned-label","devices":[],"mapping":null,"companions":null,"integrationTaken":true,"integrationChanged":true,"baselineIntegrationRunning":true,"baselineDcsRunning":true,"baselineIntegration":{"untrusted":"do not parse or resume"},"desiredIntegration":{"untrusted":"do not parse or resume"},"baselineDcs":{"untrusted":"do not read old settings"}})
}
fn save_journal(paths: &Paths, value: &Value) {
    fs::create_dir_all(paths.recovery.parent().unwrap()).unwrap();
    fs::write(&paths.recovery, serde_json::to_vec(value).unwrap()).unwrap();
}
fn config(name: &str) -> Value {
    json!({"name":name,"expanderMask":0,"buttons":[[0,0,0]],"axes":[],"hats":[],"encoders":[],"selectors":[]})
}
fn physical_journal(baseline_enabled: bool, desired_enabled: bool) -> Value {
    let mut value = journal();
    value["devices"] = json!([{"gatewayId":GATEWAY,"moduleId":MODULE,"boot":1,"baselineConfig":config("Baseline"),"desiredConfig":config("Applied"),"baselineEnabled":baseline_enabled,"desiredEnabled":desired_enabled,"configAttempted":true,"controllerAttempted":baseline_enabled!=desired_enabled,"publicationAttempted":true}]);
    value
}

#[test]
fn fresh_runtime_ignores_old_profile_store_and_never_seeds_authoring_state() {
    let directory = crate::test_directory();
    let old = directory.path().join("games.json");
    let legacy = br#"{"profiles":"malicious old data","manualInstallations":{"dcs":"old"}}"#;
    fs::write(&old, legacy).unwrap();
    let paths = Paths::in_directory(directory.path());
    let runtime = GamesRuntime::new(no_io(), paths, Arc::new(FakePlatform::default()));
    let status = runtime.status();
    assert_eq!(status["cleanup"]["phase"], "idle");
    assert!(status.get("session").is_none());
    assert!(status.get("profiles").is_none());
    assert!(status.get("defaultProfiles").is_none());
    assert_eq!(status["manualInstallations"], json!({}));
    assert!(status["error"].is_null());
    assert_eq!(fs::read(old).unwrap(), legacy);
    assert!(!directory.path().join("game-installations-v1.json").exists());
}

#[test]
fn custom_metadata_is_durable_and_never_acquires_provider_or_hardware_capabilities() {
    let directory = crate::test_directory();
    let executable = directory.path().join("Custom.exe");
    fs::write(&executable, b"fixture").unwrap();
    let platform = Arc::new(FakePlatform::default());
    let runtime = GamesRuntime::new(
        no_io(),
        Paths::in_directory(directory.path()),
        platform.clone(),
    );
    runtime
        .request(
            &json!({"command":"game_custom_save","name":"Custom game","executable":executable}),
        )
        .unwrap();
    let status = runtime.status();
    let id = status["addedGameId"].as_str().unwrap().to_owned();
    assert!(crate::game_installations::is_custom_id(&id));
    let game = status["catalogue"]
        .as_array()
        .unwrap()
        .iter()
        .find(|game| game["id"] == id)
        .unwrap();
    assert_eq!(game["provider"], Value::Null);
    assert_eq!(game["integrationSupported"], false);
    assert_eq!(game["custom"], true);
    assert_eq!(game["installed"], true);
    runtime.request(&json!({"command":"game_custom_save","gameId":id,"name":"Renamed","executable":executable})).unwrap();
    runtime.shutdown();
    drop(runtime);
    let runtime = GamesRuntime::new(
        no_io(),
        Paths::in_directory(directory.path()),
        platform.clone(),
    );
    let status = runtime.status();
    assert_eq!(
        status["catalogue"]
            .as_array()
            .unwrap()
            .iter()
            .find(|game| game["id"] == id)
            .unwrap()["name"],
        "Renamed"
    );
    assert!(runtime
        .request(&json!({"command":"game_custom_save","name":"Duplicate","executable":executable}))
        .unwrap_err()
        .contains("already belongs"));
    assert!(runtime
        .request(&json!({"command":"game_installation","gameId":"dcs","executable":executable}))
        .unwrap_err()
        .contains("already belongs"));
    runtime
        .request(&json!({"command":"game_custom_remove","gameId":id}))
        .unwrap();
    assert!(!runtime.status()["catalogue"]
        .as_array()
        .unwrap()
        .iter()
        .any(|game| game["id"] == id));
    assert_eq!(platform.calls.load(Ordering::Acquire), 0);
    runtime.shutdown();
}
#[test]
fn telemetry_inspection_is_bounded_passive_and_available_without_a_profile() {
    let directory = crate::test_directory();
    let platform = Arc::new(FakePlatform::default());
    let runtime = GamesRuntime::new(
        no_io(),
        Paths::in_directory(directory.path()),
        platform.clone(),
    );
    let before = runtime.status();
    let report = runtime.inspect_telemetry(&json!({"command":"game_telemetry_inspect","gameId":"assetto-corsa-evo","search":"engine.rpm","limit":16})).unwrap();
    assert_eq!(report["schemaVersion"], 1);
    assert_eq!(report["games"].as_array().unwrap().len(), 7);
    let evo = report["games"]
        .as_array()
        .unwrap()
        .iter()
        .find(|game| game["id"] == "assetto-corsa-evo")
        .unwrap();
    assert_eq!(evo["total"], 1);
    assert_eq!(evo["channels"][0]["id"], "engine.rpm");
    assert_eq!(evo["channels"][0]["unit"], "rpm");
    assert_eq!(evo["channels"][0]["status"], "unknown");
    assert_eq!(evo["channels"][0]["value"], Value::Null);
    assert_eq!(runtime.status()["cleanup"], before["cleanup"]);
    assert_eq!(platform.calls.load(Ordering::Acquire), 0);
    for extra in [
        json!({"limit":0}),
        json!({"limit":129}),
        json!({"offset":65537}),
        json!({"gameId":"unknown"}),
        json!({"search":"a".repeat(201)}),
        json!({"activate":true}),
    ] {
        let mut request = json!({"command":"game_telemetry_inspect"});
        request
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert!(runtime.inspect_telemetry(&request).is_err(), "{request}");
    }
    runtime.shutdown();
}
#[test]
fn manual_installations_save_restart_remove_and_corruption_is_preserved() {
    let directory = crate::test_directory();
    let paths = Paths::in_directory(directory.path());
    let file = paths.installations.clone();
    let runtime = GamesRuntime::new(no_io(), paths, Arc::new(FakePlatform::default()));
    runtime.request(&json!({"command":"game_installation","gameId":"dcs","executable":"C:\\Chosen\\DCS.exe"})).unwrap();
    runtime.shutdown();
    drop(runtime);
    let runtime = GamesRuntime::new(
        no_io(),
        Paths::in_directory(directory.path()),
        Arc::new(FakePlatform::default()),
    );
    assert_eq!(
        runtime.status()["manualInstallations"]["dcs"],
        "C:\\Chosen\\DCS.exe"
    );
    assert!(runtime
        .request(&json!({"command":"game_installation","gameId":"dcs","executable":"relative"}))
        .is_err());
    assert!(runtime
        .request(&json!({"command":"game_installation","gameId":"dcs"}))
        .is_err());
    runtime
        .request(&json!({"command":"game_installation","gameId":"dcs","executable":null}))
        .unwrap();
    assert_eq!(runtime.status()["manualInstallations"], json!({}));
    runtime.shutdown();
    drop(runtime);
    fs::write(&file, b"malformed retained metadata").unwrap();
    let runtime = GamesRuntime::new(no_io(),Paths::in_directory(directory.path()),Arc::new(FakePlatform::default()));
    assert!(runtime.status()["error"].is_null(), "retired files cannot replace the database");
    assert_eq!(runtime.status()["manualInstallations"],json!({}));runtime.shutdown();drop(runtime);
    let db=osg_persistence::Database::for_namespace(&file).unwrap();
    db.commit(&[osg_persistence::Change{domain:"games",key:"installations",expected_revision:db.revision("games","installations").unwrap(),payload:Some(b"malformed active metadata")}]).unwrap();
    let runtime=GamesRuntime::new(no_io(),Paths::in_directory(directory.path()),Arc::new(FakePlatform::default()));
    assert!(runtime.status()["error"].is_string());
    assert!(runtime.request(&json!({"command":"game_installation","gameId":"dcs","executable":"C:\\Chosen\\DCS.exe"})).is_err());
    assert_eq!(fs::read(file).unwrap(),b"malformed retained metadata");
    runtime.request(&json!({"command":"game_scan"})).unwrap();
}
#[test]
fn game_get_state_does_not_wait_for_metadata_admission_and_keeps_request_validation() {
    let directory = crate::test_directory();
    let runtime = Arc::new(GamesRuntime::new(
        no_io(),
        Paths::in_directory(directory.path()),
        Arc::new(FakePlatform::default()),
    ));
    let admission = runtime.admission.lock().unwrap();
    let reading = runtime.clone();
    let (completed, responses) = std::sync::mpsc::channel();
    let reader = thread::spawn(move || {
        completed.send([
            reading.request(&json!({"command":"game_get_state"})),
            reading.request(&json!({"command":"game_get_state","profileId":"forged"})),
            reading.request(&json!({"command":"game_get_state","clientId":"x".repeat(65_536)})),
        ]).unwrap();
    });
    let response = responses.recv_timeout(Duration::from_secs(2));
    drop(admission);
    reader.join().unwrap();
    let [valid, unknown, oversized] = response.expect("Passive state must not wait for metadata admission");
    assert!(valid.is_ok());
    assert!(unknown.unwrap_err().contains("Unknown game metadata request field"));
    assert!(oversized.unwrap_err().contains("Invalid game metadata request"));
    runtime.shutdown();
    assert!(runtime.request(&json!({"command":"game_get_state"})).unwrap_err().contains("closed"));
    assert!(fs::read_dir(directory.path()).unwrap().all(|entry|matches!(entry.unwrap().file_name().to_str(),Some("cockpit.sqlite3"|"game-artwork.lock"))));
}

#[test]
fn retired_commands_and_unknown_metadata_fields_have_no_effects_or_files() {
    let directory = crate::test_directory();
    let platform = Arc::new(FakePlatform::default());
    let runtime = GamesRuntime::new(
        no_io(),
        Paths::in_directory(directory.path()),
        platform.clone(),
    );
    for command in [
        "game_profile_save",
        "game_profile_delete",
        "game_profile_default",
        "game_launch",
        "game_activate",
        "game_deactivate",
    ] {
        assert!(runtime
            .request(&json!({"command":command,"gameId":"dcs","gameProfile":{"malicious":true}}))
            .unwrap_err()
            .contains("retired"));
    }
    assert!(runtime
        .request(&json!({"command":"game_get_state","profileId":"forged"}))
        .is_err());
    assert!(runtime
        .request(&json!({"command":"game_installation","gameId":"unknown","executable":null}))
        .is_err());
    assert!(runtime.request(&json!({"command":"unknown"})).is_err());
    assert!(runtime.guard_profile_takeover().is_ok());
    assert_eq!(platform.calls.load(Ordering::Acquire), 0);
    assert!(fs::read_dir(directory.path()).unwrap().all(|entry|matches!(entry.unwrap().file_name().to_str(),Some("cockpit.sqlite3"|"game-artwork.lock"))));
}
#[test]
fn retained_volatile_binding_intents_cleanup_without_reading_or_restarting_old_graphs() {
    let directory = crate::test_directory();
    let paths = Paths::in_directory(directory.path());
    save_journal(&paths, &journal());
    let recovery = paths.recovery.clone();
    let runtime = GamesRuntime::new(no_io(), paths, Arc::new(FakePlatform::default()));
    assert_eq!(runtime.status()["cleanup"]["phase"], "recovery-needed");
    assert!(runtime.guard_profile_takeover().is_err());
    runtime.request(&json!({"command":"game_recover"})).unwrap();
    wait(&runtime);
    assert_eq!(runtime.status()["cleanup"]["phase"], "idle");
    assert!(!recovery.exists());
    assert!(runtime.guard_profile_takeover().is_ok());
    assert!(!directory.path().join("games.json").exists());
    assert!(!directory.path().join("game-installations-v1.json").exists());
}
#[test]
fn mapping_cleanup_carries_native_exact_reference_and_never_uses_baseline_recording() {
    let directory = crate::test_directory();
    let paths = Paths::in_directory(directory.path());
    let mut value = journal();
    value["mapping"] = json!({"owner":OWNER,"taken":true,"armed":true,"stopped":false,"resumed":false,"desired":{"profile":{"requirements":{"virtualDevices":true}},"visibility":{"legacy":"marker"}},"baseline":"must not parse or resume","baselineToken":"must not parse","settings":"must not read"});
    save_journal(&paths, &value);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let recorded = calls.clone();
    let reference = json!({"id":{"kind":"manual"},"owner":{"session":OWNER,"generation":4},"recordHash":format!("sha256:{}","a".repeat(64))});
    let expected = reference.clone();
    let io: CleanupIo = Arc::new(move |request| {
        recorded
            .lock()
            .unwrap()
            .push(request["command"].as_str().unwrap().to_owned());
        assert_eq!(request["owner"], OWNER);
        match request["command"].as_str().unwrap() {
            "_retired_mapping_inspect" => json!({"ok":true,"state":{"recovery":expected}}),
            "_retired_mapping_cleanup" => {
                assert_eq!(request["recovery"], expected);
                json!({"ok":true,"state":{}})
            }
            "_retired_mapping_visibility_cleanup" => json!({"ok":true,"state":{}}),
            _ => panic!("No old graph command is allowed: {request}"),
        }
    });
    let runtime = GamesRuntime::new(io, paths, Arc::new(FakePlatform::default()));
    runtime.request(&json!({"command":"game_recover"})).unwrap();
    wait(&runtime);
    assert_eq!(runtime.status()["cleanup"]["phase"], "idle");
    assert_eq!(
        *calls.lock().unwrap(),
        [
            "_retired_mapping_inspect",
            "_retired_mapping_cleanup",
            "_retired_mapping_visibility_cleanup"
        ]
    );
}
#[derive(Default)]
struct Hardware {
    config: Value,
    enabled: bool,
    boot: u64,
    profile_match: bool,
    offline: bool,
    lose_apply_reply: bool,
    offline_after_apply: bool,
    calls: Vec<String>,
}
fn hardware() -> (CleanupIo, Arc<Mutex<Hardware>>) {
    let state = Arc::new(Mutex::new(Hardware {
        config: config("Applied"),
        enabled: true,
        boot: 1,
        profile_match: false,
        ..Default::default()
    }));
    let shared = state.clone();
    let io: CleanupIo = Arc::new(move |request| {
        assert_eq!(request["gatewayId"], GATEWAY);
        assert_eq!(request["moduleId"], MODULE);
        let command = request["command"].as_str().unwrap();
        let mut state = shared.lock().unwrap();
        state.calls.push(command.into());
        if state.offline {
            return json!({"ok":false,"error":"offline"});
        }
        match command {
            "config_read" | "claim" | "release" => {}
            "config_apply" => {
                state.config = request["config"].clone();
                state.profile_match = false;
                if state.offline_after_apply {
                    state.offline = true;
                }
                if state.lose_apply_reply {
                    return json!({"ok":false,"error":"Acknowledgement lost"});
                }
            }
            "enable" => {
                state.enabled = true;
                state.profile_match = true;
            }
            "disable" => {
                state.enabled = false;
                state.profile_match = false;
            }
            "update-profile" => state.profile_match = true,
            _ => panic!("No unowned operation: {request}"),
        }
        json!({"ok":true,"state":{"connected":true,"layoutPending":false,"modules":[{"id":MODULE,"boot":state.boot,"online":true,"enabled":state.enabled,"profileMatch":state.profile_match}],"moduleConfigs":{MODULE:{"generation":1,"savedGeneration":1,"owner":0,"dirty":false,"pendingApply":false,"valid":true,"view":"active","config":state.config}}}})
    });
    (io, state)
}
#[test]
fn physical_cleanup_reconciles_lost_ack_once_and_publishes_only_owned_baseline() {
    for baseline_enabled in [true, false] {
        let directory = crate::test_directory();
        let paths = Paths::in_directory(directory.path());
        save_journal(&paths, &physical_journal(baseline_enabled, true));
        let (io, state) = hardware();
        state.lock().unwrap().lose_apply_reply = true;
        let runtime = GamesRuntime::new(io, paths, Arc::new(FakePlatform::default()));
        runtime.request(&json!({"command":"game_recover"})).unwrap();
        wait(&runtime);
        assert_eq!(
            runtime.status()["cleanup"]["phase"],
            "idle",
            "{}",
            runtime.status()
        );
        let state = state.lock().unwrap();
        assert_eq!(state.config, config("Baseline"));
        assert_eq!(state.enabled, baseline_enabled);
        assert_eq!(
            state
                .calls
                .iter()
                .filter(|call| *call == "config_apply")
                .count(),
            1
        );
        assert!(state
            .calls
            .iter()
            .all(|call| !call.starts_with("integration_") && !call.starts_with("_game_mapping_")));
    }
}
#[test]
fn offline_readback_preserves_journal_and_retry_does_not_repeat_a_confirmed_write() {
    let directory = crate::test_directory();
    let paths = Paths::in_directory(directory.path());
    save_journal(&paths, &physical_journal(true, true));
    let recovery = paths.recovery.clone();
    let (io, state) = hardware();
    state.lock().unwrap().offline_after_apply = true;
    let runtime = GamesRuntime::new(io, paths, Arc::new(FakePlatform::default()));
    runtime.request(&json!({"command":"game_recover"})).unwrap();
    wait(&runtime);
    assert_eq!(runtime.status()["cleanup"]["phase"], "recovery-needed");
    assert!(recovery.exists());
    {
        let mut state = state.lock().unwrap();
        state.offline = false;
        state.offline_after_apply = false;
    }
    runtime.request(&json!({"command":"game_recover"})).unwrap();
    wait(&runtime);
    assert_eq!(runtime.status()["cleanup"]["phase"], "idle");
    assert_eq!(
        state
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|call| *call == "config_apply")
            .count(),
        1
    );
}
#[test]
fn foreign_hardware_or_replaced_journal_is_preserved_without_cleanup_mutation() {
    let directory = crate::test_directory();
    let paths = Paths::in_directory(directory.path());
    save_journal(&paths, &physical_journal(true, true));
    let recovery = paths.recovery.clone();
    let (io, state) = hardware();
    state.lock().unwrap().config = config("External");
    let runtime = GamesRuntime::new(io, paths, Arc::new(FakePlatform::default()));
    runtime.request(&json!({"command":"game_recover"})).unwrap();
    wait(&runtime);
    assert_eq!(runtime.status()["cleanup"]["phase"], "recovery-needed");
    assert!(!state
        .lock()
        .unwrap()
        .calls
        .iter()
        .any(|call| call == "claim" || call == "config_apply"));
    let mut other = journal();
    other["profileId"] = json!("replacement");
    fs::write(&recovery, serde_json::to_vec(&other).unwrap()).unwrap();
    let before = fs::read(&recovery).unwrap();
    assert!(runtime
        .request(&json!({"command":"game_recover"}))
        .unwrap_err()
        .contains("changed"));
    assert!(runtime
        .request(&json!({"command":"game_abandon","preserveCurrentSetup":true}))
        .unwrap_err()
        .contains("changed"));
    assert_eq!(fs::read(recovery).unwrap(), before);
}
#[test]
fn malformed_cleanup_requires_explicit_acknowledgement_without_output_recovery() {
    let directory = crate::test_directory();
    let paths = Paths::in_directory(directory.path());
    let recovery = paths.recovery.clone();
    fs::write(&recovery, b"malformed retained journal").unwrap();
    let runtime = GamesRuntime::new(no_io(), paths, Arc::new(FakePlatform::default()));
    assert!(runtime.guard_profile_takeover().is_err());
    assert!(runtime.request(&json!({"command":"game_recover"})).is_err());
    assert!(runtime.request(&json!({"command":"game_abandon"})).is_err());
    runtime
        .request(&json!({"command":"game_abandon","preserveCurrentSetup":true}))
        .unwrap();
    assert!(!recovery.exists());
    assert_eq!(runtime.status()["cleanup"]["phase"], "idle");
    assert!(runtime.status()["cleanup"]["recoveryArchive"].is_string());
}
#[test]
fn acknowledgement_cannot_archive_a_replacement_or_unbounded_rejected_journal() {
    let directory = crate::test_directory();
    let paths = Paths::in_directory(directory.path());
    let recovery = paths.recovery.clone();
    fs::write(&recovery, b"malformed retained journal").unwrap();
    let runtime = GamesRuntime::new(no_io(), paths, Arc::new(FakePlatform::default()));
    fs::write(&recovery, serde_json::to_vec(&journal()).unwrap()).unwrap();
    let replacement = fs::read(&recovery).unwrap();
    assert!(runtime
        .request(&json!({"command":"game_abandon","preserveCurrentSetup":true}))
        .unwrap_err()
        .contains("changed"));
    assert_eq!(fs::read(&recovery).unwrap(), replacement);
    runtime.shutdown();
    drop(runtime);
    fs::write(&recovery, vec![b'x'; 2 * 1024 * 1024 + 1]).unwrap();
    let runtime = GamesRuntime::new(
        no_io(),
        Paths::in_directory(directory.path()),
        Arc::new(FakePlatform::default()),
    );
    assert!(runtime
        .request(&json!({"command":"game_abandon","preserveCurrentSetup":true}))
        .unwrap_err()
        .contains("bounded"));
    assert_eq!(fs::metadata(recovery).unwrap().len(), 2 * 1024 * 1024 + 1);
}

#[test]
fn competing_cleanup_instances_cannot_restore_or_acknowledge_the_owned_file() {
    let directory = crate::test_directory();
    let paths = Paths::in_directory(directory.path());
    save_journal(&paths, &physical_journal(true, true));
    let entered = Arc::new(std::sync::Barrier::new(2));
    let released = Arc::new(std::sync::Barrier::new(2));
    let first_read = Arc::new(AtomicBool::new(true));
    let (hardware_io, hardware) = hardware();
    let io: CleanupIo = {
        let entered = entered.clone();
        let released = released.clone();
        Arc::new(move |request| {
            if request["command"] == "config_read" && first_read.swap(false, Ordering::AcqRel) {
                entered.wait();
                released.wait();
            }
            hardware_io(request)
        })
    };
    let first = GamesRuntime::new(io, paths, Arc::new(FakePlatform::default()));
    first.request(&json!({"command":"game_recover"})).unwrap();
    entered.wait();
    let second = GamesRuntime::new(
        no_io(),
        Paths::in_directory(directory.path()),
        Arc::new(FakePlatform::default()),
    );
    let recover = second.request(&json!({"command":"game_recover"}));
    let abandon = second.request(&json!({"command":"game_abandon","preserveCurrentSetup":true}));
    released.wait();
    wait(&first);
    assert!(recover.unwrap_err().contains("Another Cockpit process"));
    assert!(abandon.unwrap_err().contains("Another Cockpit process"));
    assert_eq!(first.status()["cleanup"]["phase"], "idle");
    assert_eq!(
        hardware
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|command| *command == "config_apply")
            .count(),
        1
    );
}

#[test]
fn malformed_identity_phase_or_physical_evidence_never_reaches_cleanup_io() {
    let mut cases = vec![json!({}), json!({"schemaVersion":4})];
    for (pointer, value) in [
        ("/gameId", json!("unknown")),
        ("/profileId", json!("../../forged")),
        ("/integrationTaken", Value::Null),
        ("/devices/0/gatewayId", json!("unrouted")),
        ("/devices/0/moduleId", json!("f".repeat(32))),
        ("/devices/0/boot", json!(0)),
        ("/devices/0/configAttempted", Value::Null),
        ("/devices/0/baselineConfig", json!({"name":"malformed"})),
    ] {
        let mut value_to_test = physical_journal(true, true);
        *value_to_test.pointer_mut(pointer).unwrap() = value;
        cases.push(value_to_test);
    }
    let mut duplicates = physical_journal(true, true);
    let row = duplicates["devices"][0].clone();
    duplicates["devices"].as_array_mut().unwrap().push(row);
    cases.push(duplicates);
    for value in cases {
        let directory = crate::test_directory();
        let paths = Paths::in_directory(directory.path());
        save_journal(&paths, &value);
        let file = paths.recovery.clone();
        let bytes = fs::read(&file).unwrap();
        let runtime = GamesRuntime::new(no_io(), paths, Arc::new(FakePlatform::default()));
        assert!(runtime.request(&json!({"command":"game_recover"})).is_err());
        assert!(runtime.guard_profile_takeover().is_err());
        assert_eq!(fs::read(file).unwrap(), bytes);
    }
}
