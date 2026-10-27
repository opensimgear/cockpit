use super::*;
use crate::game_discovery::{Platform, ProcessIdentity};
use crate::game_installations::{GameRecord, Installation, LibrarySource, Settings};
use std::collections::BTreeMap;
#[path="games/manual_registration_tests.rs"] mod manual_registration_tests;

const GAME: &str = "game-fixture-123";

struct MetadataOnly;
impl Platform for MetadataOnly {
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
        vec![]
    }
    fn validate(&self, _: &str, path: &str) -> Result<(), String> {
        crate::game_discovery::validate_executable(path).map(|_| ())
    }
    fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> {
        panic!("metadata cannot launch")
    }
    fn find(&self, _: &Value) -> Result<Option<ProcessIdentity>, String> {
        panic!("metadata cannot inspect processes")
    }
    fn alive(&self, _: &ProcessIdentity) -> Result<bool, String> {
        panic!("metadata cannot acquire process authority")
    }
}

fn seeded_settings(directory: &Path) -> Settings {
    let install_a = directory.join("install-a");
    let install_b = directory.join("install-b");
    fs::create_dir_all(&install_a).unwrap();
    fs::create_dir_all(&install_b).unwrap();
    let executable_a = install_a.join("Game.exe");
    let executable_b = install_b.join("Game.exe");
    fs::write(&executable_a, b"fixture a").unwrap();
    fs::write(&executable_b, b"fixture b").unwrap();
    let installation = |root: &Path, executable: &Path, source_id: &str| Installation {
        root: root.canonicalize().unwrap().to_string_lossy().into_owned(),
        executable: Some(
            executable
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        ),
        executables: vec![executable
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned()],
        sources: vec![LibrarySource {
            client: "disk".into(),
            app_id: source_id.into(),
            install_path: root.canonicalize().unwrap().to_string_lossy().into_owned(),
            launcher: None,
        }],
    };
    Settings {
        revision: 1,
        discovery_initialized: true,
        games: BTreeMap::from([(
            GAME.into(),
            GameRecord {
                name: "Fixture Game".into(),
                installations: BTreeMap::from([
                    (
                        "install-a".into(),
                        installation(&install_a, &executable_a, "fixture-a"),
                    ),
                    (
                        "install-b".into(),
                        installation(&install_b, &executable_b, "fixture-b"),
                    ),
                ]),
                selected_installation: Some("install-a".into()),
                integration: None,
                default_background: None,
            },
        )]),
        ..Settings::default()
    }
}

fn fixture() -> (GamesRuntime, tempfile::TempDir) {
    let directory = crate::test_directory();
    let paths = Paths::in_directory(directory.path());
    let initial = seeded_settings(directory.path());
    crate::game_installations::Store::new(paths.installations.clone())
        .save(&initial)
        .unwrap();
    let runtime = GamesRuntime::test_runtime(
        Arc::new(|request| panic!("metadata cannot dispatch output: {request}")),
        paths,
        Arc::new(MetadataOnly),
    );
    runtime.seed_test_catalogue(crate::game_discovery::merged_catalogue(
        &MetadataOnly,
        &initial,
    ));
    (runtime, directory)
}

fn saved(runtime: &GamesRuntime) -> Settings {
    runtime.inner.settings.lock().unwrap().saved.clone()
}

#[test]
fn library_artwork_browsing_previews_and_pages_without_publishing_metadata() {
    let (runtime, directory) = fixture();
    let root = directory.path().join("install-a");
    let cover = root.join("cover.png");
    image::RgbImage::new(480, 800).save(&cover).unwrap();
    image::RgbImage::new(960, 540).save(root.join("header.jpg")).unwrap();
    fs::write(root.join("folder.jpg"), b"invalid raster").unwrap();
    let before = saved(&runtime);
    let first = runtime.artwork_options(&json!({"command":"game_artwork_list","gameId":GAME,"limit":1})).unwrap();
    assert_eq!(first["total"], 2);
    assert_eq!(first["items"].as_array().unwrap().len(), 1);
    assert_eq!(first["nextOffset"], 1);
    assert_eq!(runtime.artwork_options(&json!({"command":"game_artwork_list","gameId":GAME,"limit":1})).unwrap(), first);
    assert!(first["items"][0]["image"].as_str().unwrap().starts_with("data:image/jpeg;base64,"));
    let second = runtime.artwork_options(&json!({"command":"game_artwork_list","gameId":GAME,"offset":1,"limit":1})).unwrap();
    assert!(second["nextOffset"].is_null());
    let artwork_id = first["items"][0]["id"].as_str().unwrap();
    let preview = runtime.artwork_options(&json!({"command":"game_artwork_preview","gameId":GAME,"artworkId":artwork_id})).unwrap();
    assert!(preview["image"].as_str().unwrap().starts_with("data:image/jpeg;base64,"));
    for request in [
        json!({"command":"game_artwork_list","gameId":GAME,"limit":0}),
        json!({"command":"game_artwork_list","gameId":GAME,"offset":65}),
        json!({"command":"game_artwork_list","gameId":GAME,"path":cover}),
        json!({"command":"game_artwork_list","gameId":"other-game"}),
        json!({"command":"game_artwork_preview","gameId":GAME,"artworkId":"bad"}),
        json!({"command":"game_artwork_preview","gameId":GAME,"artworkId":"0".repeat(64)}),
    ] { assert!(runtime.artwork_options(&request).is_err(), "{request}"); }
    fs::remove_file(cover).unwrap();
    assert!(runtime.artwork_options(&json!({"command":"game_artwork_preview","gameId":GAME,"artworkId":artwork_id})).is_err());
    assert_eq!(saved(&runtime), before);
    runtime.shutdown();
    assert!(runtime.artwork_options(&json!({"command":"game_artwork_list","gameId":GAME})).is_err());
}

fn high_resolution_artwork_preview() -> String {
    use base64::Engine;
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90)
        .encode_image(&image::RgbImage::new(960, 540)).unwrap();
    // Valid JPEG comment segments exercise the transport bound without
    // depending on compression ratios of a particular codec build.
    let padding = vec![b'a'; 90_000 - bytes.len()];
    for chunk in padding.chunks(60_000) {
        let length = ((chunk.len() + 2) as u16).to_be_bytes();
        bytes.splice(2..2, [vec![0xff, 0xfe, length[0], length[1]], chunk.to_vec()].concat());
    }
    format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

#[test]
fn game_metadata_edits_persist_and_installation_selection_is_independent() {
    let (runtime, _directory) = fixture();
    runtime
        .metadata_request(&json!({"command":"game_save","gameId":GAME,"name":"Renamed Game"}))
        .unwrap();
    assert_eq!(saved(&runtime).games[GAME].name, "Renamed Game");
    runtime.metadata_request(&json!({"command":"game_installation_select","gameId":GAME,"installationId":"install-b"})).unwrap();
    let persisted = saved(&runtime);
    assert_eq!(
        persisted.games[GAME].selected_installation.as_deref(),
        Some("install-b")
    );
    assert_eq!(
        runtime.inner.installations.load().unwrap(),
        Some(persisted.clone())
    );
    runtime.shutdown();
}

#[test]
fn executable_edits_are_confined_to_the_selected_installation_and_atomic() {
    let (runtime, directory) = fixture();
    let before = saved(&runtime);
    let inside = directory.path().join("install-a").join("Alternate.exe");
    fs::write(&inside, b"alternate runtime").unwrap();
    runtime.metadata_request(&json!({"command":"game_save","gameId":GAME,"installationId":"install-a","executable":inside})).unwrap();
    let updated = saved(&runtime);
    assert_eq!(
        updated.games[GAME].installations["install-a"]
            .executable
            .as_deref(),
        Some(inside.canonicalize().unwrap().to_str().unwrap())
    );
    for request in [
        json!({"command":"game_save","gameId":GAME,"installationId":"install-a","executable":"relative.exe"}),
        json!({"command":"game_save","gameId":GAME,"installationId":"install-a","executable":directory.path().join("outside.exe")}),
        json!({"command":"game_save","gameId":GAME,"installationId":"missing-install","executable":null}),
        json!({"command":"game_save","gameId":GAME,"name":""}),
        json!({"command":"game_save","gameId":"game-unknown","name":"Unknown"}),
    ] {
        assert!(runtime.metadata_request(&request).is_err(), "{request}");
        assert_eq!(saved(&runtime), updated);
    }
    assert_ne!(before, updated);
    runtime.shutdown();
}

#[test]
fn tags_and_backgrounds_follow_game_ownership_and_remove_atomically() {
    let (runtime, directory) = fixture();
    runtime.metadata_request(&json!({"command":"game_tags_save","gameId":GAME,
        "tagIds":["racing","simulation"],"expectedTagIds":[]})).unwrap();
    let image = directory.path().join("header.png");
    image::RgbImage::from_pixel(80, 60, image::Rgb([35, 70, 120]))
        .save(&image)
        .unwrap();
    runtime.metadata_request(&json!({"command":"game_background_save","gameId":GAME,"path":image,"positionX":20.0,"positionY":75.0})).unwrap();
    let before_remove = saved(&runtime);
    assert_eq!(before_remove.game_tags[GAME], vec!["racing", "simulation"]);
    assert!(before_remove.backgrounds.contains_key(GAME));
    assert!(runtime
        .metadata_request(
            &json!({"command":"game_tags_save","gameId":GAME,"tagIds":["missing"],"expectedTagIds":["racing","simulation"]})
        )
        .is_err());
    assert_eq!(saved(&runtime), before_remove);
    runtime
        .metadata_request(&json!({"command":"game_remove","gameId":GAME}))
        .unwrap();
    let removed = saved(&runtime);
    assert!(!removed.games.contains_key(GAME));
    assert!(!removed.game_tags.contains_key(GAME));
    assert!(!removed.backgrounds.contains_key(GAME));
    assert!(runtime
        .metadata_request(&json!({"command":"game_remove","gameId":GAME}))
        .is_err());
    runtime.shutdown();
}

#[test]
fn high_resolution_discovery_artwork_can_be_saved_as_a_background() {
    let (runtime, _directory) = fixture();
    let image = high_resolution_artwork_preview();
    let request = json!({
        "command":"game_background_save",
        "gameId":GAME,
        "imageData":image,
        "crop":{"x":0.0,"y":0.0,"width":1.0,"height":1.0}
    });
    assert!(request.to_string().len() > 65_536);
    assert!(request.to_string().len() <= crate::gateways::GAME_METADATA_REQUEST_LIMIT);
    runtime.metadata_request(&request).unwrap();
    let background = &saved(&runtime).backgrounds[GAME];
    let bytes = fs::read(runtime.inner.installations.root().join("game-artwork").join(
        background.image.strip_prefix("artwork:").unwrap(),
    ))
    .unwrap();
    assert!(bytes.len() <= 128 * 1024);
    assert_eq!(background.crop.unwrap().width, 1.0);
    runtime.shutdown();
}

#[test]
fn prepared_edits_reject_stale_and_busy_commits_without_losing_newer_state() {
    let (runtime, _directory) = fixture();
    let stale = runtime
        .prepare_metadata(&json!({"command":"game_save","gameId":GAME,"name":"Stale"}))
        .unwrap();
    runtime
        .metadata_request(&json!({"command":"game_save","gameId":GAME,"name":"Current"}))
        .unwrap();
    let current = saved(&runtime);
    assert!(runtime
        .commit_metadata(stale)
        .unwrap_err()
        .contains("changed during preparation"));
    assert_eq!(saved(&runtime), current);
    let prepared = runtime
        .prepare_metadata(&json!({"command":"game_save","gameId":GAME,"name":"Busy"}))
        .unwrap();
    let admission = runtime.admission.lock().unwrap();
    assert!(runtime
        .commit_metadata(prepared)
        .unwrap_err()
        .contains("busy"));
    drop(admission);
    assert_eq!(saved(&runtime), current);
    runtime.shutdown();
}

#[path="games/tags/metadata_tests.rs"] mod tag_tests;
