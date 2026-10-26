use super::*;
use crate::{
    game_discovery::ProcessIdentity,
    game_installations::{GameRecord, Installation, LibrarySource, Settings},
};
use std::{collections::BTreeMap, fs, sync::atomic::AtomicUsize};

const GAME: &str = "dcs";
#[derive(Default)]
struct FakePlatform {
    calls: AtomicUsize,
}
impl Platform for FakePlatform {
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
        vec![]
    }
    fn validate(&self, _: &str, path: &str) -> Result<(), String> {
        if path.starts_with("C:\\") || Path::new(path).is_absolute() {
            Ok(())
        } else {
            Err("Invalid executable".into())
        }
    }
    fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> {
        panic!("Metadata must never launch")
    }
    fn spawn_process(&self, _: &Value) -> Result<(), String> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        panic!("Metadata must never spawn")
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

fn seed_registry(root: &Path) -> Settings {
    let first_root = root.join("first");
    let second_root = root.join("second");
    fs::create_dir_all(&first_root).unwrap();
    fs::create_dir_all(&second_root).unwrap();
    let first_exe = first_root.join("DCS.exe");
    let second_exe = second_root.join("DCS.exe");
    fs::write(&first_exe, b"fixture").unwrap();
    fs::write(&second_exe, b"fixture").unwrap();
    let installation = |path: &Path, exe: &Path, source: &str| Installation {
        root: path.canonicalize().unwrap().to_string_lossy().into_owned(),
        executable: Some(exe.canonicalize().unwrap().to_string_lossy().into_owned()),
        executables: vec![exe.canonicalize().unwrap().to_string_lossy().into_owned()],
        sources: vec![LibrarySource {
            client: "disk".into(),
            app_id: source.into(),
            install_path: path.canonicalize().unwrap().to_string_lossy().into_owned(),
            launcher: None,
        }],
    };
    Settings {
        revision: 1,
        discovery_initialized: true,
        games: BTreeMap::from([(
            GAME.into(),
            GameRecord {
                name: "DCS".into(),
                installations: BTreeMap::from([
                    (
                        "dcs-first".into(),
                        installation(&first_root, &first_exe, "dcs-first"),
                    ),
                    (
                        "dcs-second".into(),
                        installation(&second_root, &second_exe, "dcs-second"),
                    ),
                ]),
                selected_installation: Some("dcs-first".into()),
                integration: Some("dcs".into()),
                default_background: None,
            },
        )]),
        ..Settings::default()
    }
}
fn seeded_runtime(directory: &Path, platform: Arc<FakePlatform>) -> GamesRuntime {
    let paths = Paths::in_directory(directory);
    let settings = seed_registry(directory);
    crate::game_installations::Store::new(paths.installations.clone())
        .save(&settings)
        .unwrap();
    GamesRuntime::test_runtime(no_io(), paths, platform)
}

#[test]
fn fresh_runtime_ignores_retired_profile_store_and_has_no_seeded_games() {
    let directory = crate::test_directory();
    let old = directory.path().join("games.json");
    let legacy = br#"{"profiles":"malicious old data","preferences":{"untrusted":true}}"#;
    fs::write(&old, legacy).unwrap();
    let runtime = GamesRuntime::test_runtime(
        no_io(),
        Paths::in_directory(directory.path()),
        Arc::new(FakePlatform::default()),
    );
    let status = runtime.status();
    assert!(status.get("cleanup").is_none());
    assert!(status.get("session").is_none());
    assert!(status.get("profiles").is_none());
    assert!(status["catalogue"].as_array().unwrap().is_empty());
    assert_eq!(status["discovery"]["status"], "idle");
    assert_eq!(fs::read(old).unwrap(), legacy);
    runtime.shutdown();
}

#[test]
fn discovered_game_metadata_persists_across_restart_and_removal_cleans_related_records() {
    let directory = crate::test_directory();
    let platform = Arc::new(FakePlatform::default());
    let runtime = seeded_runtime(directory.path(), platform.clone());
    runtime
        .request(&json!({"command":"game_save","gameId":GAME,"name":"Renamed DCS"}))
        .unwrap();
    runtime.request(&json!({"command":"game_installation_select","gameId":GAME,"installationId":"dcs-second"})).unwrap();
    assert_eq!(
        runtime.status()["catalogue"]
            .as_array()
            .unwrap()
            .iter()
            .find(|game| game["id"] == GAME)
            .unwrap()["selectedInstallationId"],
        "dcs-second"
    );
    runtime.shutdown();
    drop(runtime);
    let runtime = GamesRuntime::test_runtime(
        no_io(),
        Paths::in_directory(directory.path()),
        platform.clone(),
    );
    let game = runtime.status()["catalogue"]
        .as_array()
        .unwrap()
        .iter()
        .find(|game| game["id"] == GAME)
        .unwrap()
        .clone();
    assert_eq!(game["name"], "Renamed DCS");
    assert_eq!(game["selectedInstallationId"], "dcs-second");
    runtime
        .request(&json!({"command":"game_remove","gameId":GAME}))
        .unwrap();
    assert!(runtime.status()["catalogue"]
        .as_array()
        .unwrap()
        .iter()
        .all(|game| game["id"] != GAME));
    assert!(!runtime
        .inner
        .installations
        .load()
        .unwrap()
        .unwrap()
        .games
        .contains_key(GAME));
    assert_eq!(platform.calls.load(Ordering::Acquire), 0);
    runtime.shutdown();
}

#[test]
fn telemetry_inspection_is_bounded_passive_and_available_without_a_profile() {
    let directory = crate::test_directory();
    let platform = Arc::new(FakePlatform::default());
    let runtime = seeded_runtime(directory.path(), platform.clone());
    runtime.seed_test_catalogue(vec![json!({
        "id":GAME,
        "name":"Telemetry Fixture",
        "provider":"assetto-corsa-evo",
        "integrationKey":"assetto-corsa-evo",
        "integrationCompatible":true,
        "installed":true,
        "integrationSetup":{"state":"ready"},
        "launchSupported":false,
        "processMonitoringSupported":true
    })]);
    let before = runtime.status();
    let report = runtime.inspect_telemetry(&json!({"command":"game_telemetry_inspect","gameId":GAME,"search":"engine.rpm","limit":16})).unwrap();
    assert_eq!(report["schemaVersion"], 1);
    assert_eq!(report["games"].as_array().unwrap().len(), 1);
    let game = &report["games"][0];
    assert_eq!(game["id"], GAME);
    assert_eq!(game["total"], 1);
    assert_eq!(game["channels"][0]["id"], "engine.rpm");
    assert_eq!(game["channels"][0]["unit"], "rpm");
    assert_eq!(game["channels"][0]["status"], "unknown");
    assert_eq!(game["channels"][0]["value"], Value::Null);
    assert_eq!(runtime.status()["discovery"], before["discovery"]);
    assert_eq!(platform.calls.load(Ordering::Acquire), 0);
    for extra in [
        json!({"limit":0}),
        json!({"limit":129}),
        json!({"offset":65537}),
        json!({"gameId":"Unknown"}),
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
fn game_get_state_does_not_wait_for_metadata_admission_and_keeps_request_validation() {
    let directory = crate::test_directory();
    let runtime = Arc::new(GamesRuntime::test_runtime(
        no_io(),
        Paths::in_directory(directory.path()),
        Arc::new(FakePlatform::default()),
    ));
    let admission = runtime.admission.lock().unwrap();
    let reading = runtime.clone();
    let (completed, responses) = std::sync::mpsc::channel();
    let reader = thread::spawn(move || {
        completed
            .send([
                reading.request(&json!({"command":"game_get_state"})),
                reading.request(&json!({"command":"game_get_state","profileId":"forged"})),
            ])
            .unwrap();
    });
    let response = responses.recv_timeout(Duration::from_secs(2));
    drop(admission);
    reader.join().unwrap();
    let [valid, unknown] = response.expect("Passive state must not wait for metadata admission");
    assert!(valid.is_ok());
    assert!(unknown.unwrap_err().contains("Unknown game status field"));
    runtime.shutdown();
    assert!(runtime
        .request(&json!({"command":"game_get_state"}))
        .unwrap_err()
        .contains("closed"));
}

#[test]
fn retired_commands_and_unknown_metadata_fields_have_no_effects_or_hardware_calls() {
    let directory = crate::test_directory();
    let platform = Arc::new(FakePlatform::default());
    let runtime = GamesRuntime::test_runtime(
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
        "game_installation",
    ] {
        assert!(runtime
            .request(&json!({"command":command,"gameId":GAME,"gameProfile":{"malicious":true}}))
            .is_err());
    }
    assert!(runtime
        .request(&json!({"command":"game_get_state","profileId":"forged"}))
        .is_err());
    assert!(runtime
        .request(&json!({"command":"game_save","gameId":"unknown","name":"x"}))
        .is_err());
    assert!(runtime.request(&json!({"command":"unknown"})).is_err());
    assert!(runtime.guard_profile_takeover().is_ok());
    assert_eq!(platform.calls.load(Ordering::Acquire), 0);
    runtime.shutdown();
}

#[test]
fn retired_cleanup_journals_are_ignored_without_migration_or_hardware_calls() {
    let directory = crate::test_directory();
    let paths = Paths::in_directory(directory.path());
    let retired = paths.recovery.clone();
    fs::write(&retired, b"discarded legacy data").unwrap();
    let runtime = GamesRuntime::test_runtime(no_io(), paths, Arc::new(FakePlatform::default()));
    assert!(runtime.status().get("cleanup").is_none());
    assert!(runtime.guard_profile_takeover().is_ok());
    for command in ["game_recover", "game_abandon"] {
        assert!(runtime
            .request(&json!({"command":command,"preserveCurrentSetup":true}))
            .is_err());
    }
    assert_eq!(fs::read(&retired).unwrap(), b"discarded legacy data");
    assert!(!directory.path().join("games.cleanup.lock").exists());
}
