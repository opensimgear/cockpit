use super::*;
use crate::game_discovery::ProcessIdentity;
use std::collections::BTreeMap;
use std::sync::atomic::AtomicUsize;
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
#[test]
fn fresh_runtime_ignores_old_profile_store_and_never_seeds_authoring_state() {
    let directory = crate::test_directory();
    let old = directory.path().join("games.json");
    let legacy = br#"{"profiles":"malicious old data","manualInstallations":{"dcs":"old"}}"#;
    fs::write(&old, legacy).unwrap();
    let paths = Paths::in_directory(directory.path());
    let runtime = GamesRuntime::new(no_io(), paths, Arc::new(FakePlatform::default()));
    let status = runtime.status();
    assert!(status.get("cleanup").is_none());
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
fn retired_cleanup_journals_are_ignored_without_migration_or_hardware_calls(){
    let directory=crate::test_directory();let paths=Paths::in_directory(directory.path());
    let retired=paths.recovery.clone();fs::write(&retired,b"discarded legacy data").unwrap();
    let runtime=GamesRuntime::new(no_io(),paths,Arc::new(FakePlatform::default()));
    assert!(runtime.status().get("cleanup").is_none());assert!(runtime.guard_profile_takeover().is_ok());
    for command in ["game_recover","game_abandon"]{assert!(runtime.request(&json!({"command":command,"preserveCurrentSetup":true})).is_err());}
    assert_eq!(fs::read(&retired).unwrap(),b"discarded legacy data");
    assert!(!directory.path().join("games.cleanup.lock").exists());
}
