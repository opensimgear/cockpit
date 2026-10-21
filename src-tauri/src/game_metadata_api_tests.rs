use super::*;
use crate::{
    game_discovery::{Platform, ProcessIdentity},
    game_library::{Candidate, Scan},
};
use std::collections::BTreeMap;

struct MetadataOnly;
impl Platform for MetadataOnly {
    fn catalogue(&self, manual: &BTreeMap<String, String>) -> Vec<Value> {
        osg_profiles::activation::SUPPORTED_GAME_IDS
            .iter()
            .map(|id| {
                json!({
                    "id":id,"name":id,"provider":if *id=="dcs" {"dcs-bios"} else {*id},
                    "installed":manual.contains_key(*id),"executable":manual.get(*id),
                    "processMonitoringSupported":false,"launchSupported":false,
                })
            })
            .collect()
    }
    fn validate(&self, _: &str, path: &str) -> Result<(), String> {
        crate::game_discovery::validate_custom_executable(path).map(|_| ())
    }
    fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> {
        panic!("metadata cannot launch")
    }
    fn find(&self, _: &Value) -> Result<Option<ProcessIdentity>, String> {
        panic!("metadata cannot scan processes")
    }
    fn alive(&self, _: &ProcessIdentity) -> Result<bool, String> {
        panic!("metadata cannot acquire process authority")
    }
}
fn fixture() -> (GamesRuntime, tempfile::TempDir) {
    let directory = crate::test_directory();
    let runtime = GamesRuntime::new(
        Arc::new(|request| panic!("metadata cannot dispatch output: {request}")),
        Paths::in_directory(directory.path()),
        Arc::new(MetadataOnly),
    );
    (runtime, directory)
}
fn executable(directory: &Path, name: &str) -> String {
    let path = directory.join(name);
    fs::write(&path, b"fixture").unwrap();
    path.to_str().unwrap().into()
}
fn artwork(directory: &Path) -> PathBuf {
    let path = directory.join("header.png");
    image::RgbImage::from_pixel(80, 60, image::Rgb([35, 70, 120]))
        .save(&path)
        .unwrap();
    path
}
fn candidate(directory: &Path, app: &str, executable_candidates: Vec<String>) -> Candidate {
    Candidate {
        id: format!("steam:{app}"),
        client: "steam".into(),
        name: "Imported game".into(),
        install_path: directory.to_str().unwrap().into(),
        executable_candidates,
        game_id: None,
        kind: "unknown".into(),
        app_id: app.into(),
        launcher: None,
        artwork: None,
    }
}
fn scan(candidates: Vec<Candidate>) -> impl FnOnce() -> Scan {
    move || Scan {
        candidates,
        diagnostics: vec!["Fixture discovery only".into()],
    }
}
fn saved(runtime: &GamesRuntime) -> crate::game_installations::Settings {
    runtime.inner.settings.lock().unwrap().saved.clone()
}
fn added(runtime: &GamesRuntime) -> String {
    runtime
        .inner
        .settings
        .lock()
        .unwrap()
        .added_game_id
        .clone()
        .unwrap()
}

#[test]
fn staged_metadata_rejects_stale_cross_runtime_and_busy_commits_without_losing_newer_settings() {
    let (runtime, directory) = fixture();
    let first = executable(directory.path(), "Staged.exe");
    runtime
        .metadata_request(
            &json!({"command":"game_custom_save","name":"Original","executable":first}),
        )
        .unwrap();
    let id = added(&runtime);
    let source = artwork(directory.path());
    let stale = runtime
        .prepare_metadata(&json!({"command":"game_background_save","gameId":"dcs","path":source}))
        .unwrap();
    let asset = directory.path().join("game-artwork").join(
        stale.next.backgrounds["dcs"]
            .image
            .trim_start_matches("artwork:"),
    );
    runtime.metadata_request(&json!({"command":"game_custom_save","gameId":id,"name":"Newer name","executable":first})).unwrap();
    assert!(asset.is_file(),"An outstanding preparation protects its managed artwork from a different commit's collection");
    let newer = saved(&runtime);
    assert!(runtime
        .commit_metadata(stale)
        .unwrap_err()
        .contains("changed while"));
    assert_eq!(saved(&runtime), newer);
    let proposal = runtime
        .prepare_metadata(
            &json!({"command":"game_custom_save","gameId":id,"name":"Busy","executable":first}),
        )
        .unwrap();
    let admission = runtime.admission.lock().unwrap();
    assert!(runtime
        .commit_metadata(proposal)
        .unwrap_err()
        .contains("busy"));
    drop(admission);
    assert_eq!(saved(&runtime), newer);
    let proposal=runtime.prepare_metadata(&json!({"command":"game_custom_save","gameId":id,"name":"Other runtime","executable":first})).unwrap();
    let (other, _) = fixture();
    assert!(other
        .commit_metadata(proposal)
        .unwrap_err()
        .contains("another native runtime"));
    assert_eq!(saved(&runtime), newer);
    other.shutdown();
    runtime.finish_metadata();
    assert!(!asset.exists());
    runtime.shutdown();
}

#[test]
fn manual_edits_and_all_game_backgrounds_are_atomic_and_durable() {
    let (runtime, directory) = fixture();
    let first = executable(directory.path(), "First.exe");
    runtime
        .metadata_request(
            &json!({"command":"game_custom_save","name":"  First game  ","executable":first}),
        )
        .unwrap();
    let id = added(&runtime);
    assert_eq!(saved(&runtime).custom_games[&id].name, "First game");
    let source = artwork(directory.path());
    for game in ["dcs", id.as_str()] {
        runtime.metadata_request(&json!({"command":"game_background_save","gameId":game,"path":source,"positionX":20.0})).unwrap();
        runtime
            .metadata_request(
                &json!({"command":"game_background_save","gameId":game,"positionY":80.0}),
            )
            .unwrap();
        let background = saved(&runtime).backgrounds[game].clone();
        assert_eq!((background.position_x, background.position_y), (20.0, 80.0));
    }
    let retained = saved(&runtime);
    for request in [
        json!({"command":"game_background_save","gameId":"dcs","positionX":-1}),
        json!({"command":"game_background_save","gameId":"dcs","path":42}),
        json!({"command":"game_background_save","gameId":"unknown","path":null}),
        json!({"command":"game_custom_save","gameId":id,"name":"Renamed","executable":first,"processSession":"forged"}),
        json!({"command":"game_custom_save","name":"Duplicate","executable":first}),
        json!({"command":"game_custom_save","name":"Missing executable"}),
        json!({"command":"game_custom_save","name":"Bad executable","executable":42}),
        json!({"command":"game_custom_save","gameId":"custom-forged","name":"Forged","executable":null}),
        json!({"command":"game_custom_remove","gameId":"unknown"}),
    ] {
        assert!(runtime.metadata_request(&request).is_err(), "{request}");
        assert_eq!(saved(&runtime), retained);
    }
    let relocated = executable(directory.path(), "Moved.exe");
    runtime.metadata_request(&json!({"command":"game_custom_save","gameId":id,"name":"Renamed","executable":relocated})).unwrap();
    assert_eq!(added(&runtime), id);
    assert_eq!(saved(&runtime).custom_games[&id].name, "Renamed");
    fs::remove_file(source).unwrap();
    assert_eq!(
        runtime.inner.installations.load().unwrap(),
        Some(saved(&runtime))
    );
    runtime
        .metadata_request(&json!({"command":"game_background_save","gameId":"dcs","path":null}))
        .unwrap();
    assert!(!saved(&runtime).backgrounds.contains_key("dcs"));
    runtime
        .metadata_request(&json!({"command":"game_custom_remove","gameId":id}))
        .unwrap();
    assert!(!saved(&runtime).custom_games.contains_key(&id));
    assert!(!saved(&runtime).backgrounds.contains_key(&id));
    runtime.shutdown();
}

#[test]
fn reviewed_import_resolves_ambiguity_and_rescans_keep_identity_and_override() {
    let (runtime, directory) = fixture();
    let first = executable(directory.path(), "First.exe");
    let second = executable(directory.path(), "Second.exe");
    let mut imported = candidate(directory.path(), "42", vec![first.clone(), second]);
    imported.artwork = Some(artwork(directory.path()));
    runtime
        .metadata_request_with_scan(
            &json!({"command":"game_library_scan"}),
            scan(vec![imported.clone()]),
        )
        .unwrap();
    assert!(
        saved(&runtime).custom_games.is_empty(),
        "discovery alone cannot import"
    );
    assert_eq!(
        runtime
            .inner
            .settings
            .lock()
            .unwrap()
            .library_candidates
            .len(),
        1
    );
    runtime
        .metadata_request_with_scan(
            &json!({"command":"game_library_import","candidateId":"steam:42"}),
            scan(vec![imported.clone()]),
        )
        .unwrap();
    let id = added(&runtime);
    assert!(saved(&runtime).custom_games[&id].executable.is_none());
    assert!(saved(&runtime).default_backgrounds.contains_key(&id));
    runtime
        .metadata_request(&json!({"command":"game_background_save","gameId":id,"positionX":25.0}))
        .unwrap();
    let background = saved(&runtime).backgrounds[&id].clone();
    runtime
        .metadata_request_with_scan(
            &json!({"command":"game_library_import","candidateId":"steam:42","executable":first}),
            scan(vec![imported.clone()]),
        )
        .unwrap();
    assert_eq!(added(&runtime), id);
    assert!(saved(&runtime).custom_games[&id].executable.is_some());
    imported.executable_candidates.clear();
    runtime
        .metadata_request_with_scan(
            &json!({"command":"game_library_import","candidateId":"steam:42"}),
            scan(vec![imported]),
        )
        .unwrap();
    assert_eq!(saved(&runtime).custom_games.len(), 1);
    assert!(saved(&runtime).custom_games[&id].executable.is_some());
    assert_eq!(saved(&runtime).backgrounds[&id], background);
    let retained = saved(&runtime);
    assert!(runtime
        .metadata_request_with_scan(
            &json!({"command":"game_library_import","candidateId":"steam:42"}),
            scan(vec![])
        )
        .is_err());
    assert_eq!(saved(&runtime), retained, "stale preview must not commit");
    runtime.shutdown();
}

#[test]
fn library_import_reuses_manual_and_integrated_game_identities() {
    let (runtime, directory) = fixture();
    let first = executable(directory.path(), "Manual.exe");
    let builtin = executable(directory.path(), "DCS.exe");
    runtime
        .metadata_request(
            &json!({"command":"game_custom_save","name":"My name","executable":first}),
        )
        .unwrap();
    let id = added(&runtime);
    let manual = candidate(directory.path(), "42", vec![first]);
    runtime
        .metadata_request_with_scan(
            &json!({"command":"game_library_import","candidateId":"steam:42"}),
            scan(vec![manual]),
        )
        .unwrap();
    assert_eq!(added(&runtime), id);
    assert_eq!(saved(&runtime).custom_games[&id].name, "My name");
    let mut integrated = candidate(directory.path(), "223750", vec![builtin]);
    integrated.game_id = Some("dcs".into());
    runtime
        .metadata_request_with_scan(
            &json!({"command":"game_library_import","candidateId":"steam:223750"}),
            scan(vec![integrated]),
        )
        .unwrap();
    assert_eq!(added(&runtime), "dcs");
    assert_eq!(saved(&runtime).custom_games.len(), 1);
    assert!(saved(&runtime).manual_installations.contains_key("dcs"));
    runtime.shutdown();
}
