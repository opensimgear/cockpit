use super::*;
use std::{collections::BTreeMap, fs};

fn settings() -> Settings {
    Settings {
        revision: 3,
        discovery_initialized: true,
        scan_roots: vec!["C:\\Games".into()],
        games: BTreeMap::from([(
            "dcs".into(),
            GameRecord {
                name: "DCS".into(),
                installations: BTreeMap::from([(
                    "dcs-install".into(),
                    Installation {
                        root: "C:\\Games\\DCS".into(),
                        executable: Some("C:\\Games\\DCS\\bin\\DCS.exe".into()),
                        executables: vec!["C:\\Games\\DCS\\bin\\DCS.exe".into()],
                        sources: vec![LibrarySource {
                            client: "disk".into(),
                            app_id: "local-dcs".into(),
                            install_path: "C:\\Games\\DCS".into(),
                            launcher: None,
                        }],
                    },
                )]),
                selected_installation: Some("dcs-install".into()),
                integration: None,
                default_background: None,
            },
        )]),
        ..Settings::default()
    }
}

#[test]
fn fresh_store_ignores_retired_profile_library_and_round_trips_unified_games() {
    let directory = crate::test_directory();
    let old = directory.path().join("games.json");
    let legacy =
        br#"{"profiles":[{"id":"old","devices":["unsafe"]}],"defaultProfiles":{"dcs":"old"}}"#;
    fs::write(&old, legacy).unwrap();
    let store = Store::new(directory.path().join("game-installations-v1.json"));
    assert_eq!(store.load().unwrap(), None);
    let value = settings();
    store.save(&value).unwrap();
    assert_eq!(
        Store::new(directory.path().join("game-installations-v1.json"))
            .load()
            .unwrap(),
        Some(value.clone())
    );
    assert_eq!(fs::read(old).unwrap(), legacy);
    let json: serde_json::Value =
        serde_json::from_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(json["games"]["dcs"]["selectedInstallation"], "dcs-install");
}

#[test]
fn stale_game_settings_cannot_overwrite_a_committed_edit() {
    let directory = crate::test_directory();
    let path = directory.path().join("new/settings.json");
    let first = Store::new(path.clone());
    let original = settings();
    first.save(&original).unwrap();
    let stale = Store::new(path.clone());
    stale.load().unwrap();
    let mut edited = original.clone();
    edited.games.get_mut("dcs").unwrap().name = "DCS Updated".into();
    first.save(&edited).unwrap();
    assert!(stale.save(&original).unwrap_err().contains("conflict"));
    assert_eq!(Store::new(path).load().unwrap(), Some(edited));
}

#[test]
fn strict_envelope_rejects_unknown_duplicate_and_oversized_metadata_without_rewriting() {
    let directory = crate::test_directory();
    let path = directory.path().join("game-installations-v1.json");
    let store = Store::new(path.clone());
    for bytes in [
        r#"{"revision":0,"games":{},"profiles":[]}"#,
        r#"{"revision":0,"games":{},"owner":"forged"}"#,
        r#"{"revision":0,"games":{},"revision":1}"#,
        r#"{"revision":0,"games":{"dcs":{},"dcs":{}}}"#,
        r#"{"revision":0,"games":{}}"#,
        r#"{"revision":0,"games":{},"formatVersion":2}"#,
        "malformed",
    ] {
        fs::write(&path, bytes).unwrap();
        crate::persistence::test_support::record(&path, "games", "library", bytes.as_bytes());
        assert!(store.load().is_err(), "{bytes}");
        assert_eq!(fs::read(&path).unwrap(), bytes.as_bytes());
    }
    crate::persistence::test_support::record(&path, "games", "library", &vec![b' '; MAX_BYTES + 1]);
    assert!(store.load().unwrap_err().contains("Oversized"));
}

#[test]
fn unified_registry_validates_generic_ids_installation_sources_and_distinct_roots() {
    let mut value = settings();
    value.games.insert(
        "game-unknown-123".into(),
        GameRecord {
            name: "Unknown title".into(),
            installations: BTreeMap::new(),
            selected_installation: None,
            integration: None,
            default_background: None,
        },
    );
    value.validate().unwrap();
    let too_long = format!("g{}", "a".repeat(96));
    for invalid in [
        "-game",
        "Upper",
        "game_name",
        "game/name",
        too_long.as_str(),
    ] {
        let mut invalid_settings = settings();
        let game = invalid_settings.games.remove("dcs").unwrap();
        invalid_settings.games.insert(invalid.into(), game);
        assert!(invalid_settings.validate().is_err(), "{invalid}");
    }
    let mut invalid = settings();
    invalid
        .games
        .get_mut("dcs")
        .unwrap()
        .installations
        .get_mut("dcs-install")
        .unwrap()
        .sources[0]
        .install_path = "relative".into();
    assert!(invalid.validate().is_err());
    let mut duplicate_root = settings();
    let game = duplicate_root.games["dcs"].clone();
    duplicate_root.games.insert("game-copy".into(), game);
    assert!(duplicate_root.validate().is_err());
}

#[test]
fn tag_and_background_records_require_registered_game_owners() {
    let mut value = settings();
    value.game_tags.insert("dcs".into(), vec!["racing".into(), "simulation".into()]);
    value.backgrounds.insert(
        "dcs".into(),
        crate::game_artwork::Background {
            image: format!("artwork:{}.jpg", "a".repeat(64)),
            position_x: 20.0,
            position_y: 75.0,
            crop: None,
        },
    );
    value.validate().unwrap();
    value
        .game_tags
        .insert("missing-game".into(), vec!["racing".into()]);
    assert!(value.validate().is_err());
    value.game_tags.clear();
    value.backgrounds.insert(
        "missing-game".into(),
        crate::game_artwork::Background {
            image: format!("artwork:{}.jpg", "a".repeat(64)),
            position_x: 50.0,
            position_y: 50.0,
            crop: None,
        },
    );
    assert!(value.validate().is_err());
}

#[test]
fn symbolic_target_and_parent_are_never_followed() {
    let directory = crate::test_directory();
    let real = directory.path().join("real");
    fs::create_dir(&real).unwrap();
    let target = real.join("settings.json");
    fs::write(&target, serde_json::to_vec(&settings()).unwrap()).unwrap();
    crate::persistence::test_support::record(
        &target,
        "games",
        "library",
        &fs::read(&target).unwrap(),
    );
    Store::new(target.clone()).load().unwrap();
    let retained = fs::read(&target).unwrap();
    #[cfg(unix)]
    let link = |source: &Path, destination: &Path, _directory: bool| {
        std::os::unix::fs::symlink(source, destination)
    };
    #[cfg(unix)]
    {
        let linked = directory.path().join("linked.json");
        link(&target, &linked, false).unwrap();
        assert!(Store::new(linked).load().unwrap().is_none());
    }
    let parent = directory.path().join("alias");
    #[cfg(unix)]
    link(&real, &parent, true).unwrap();
    #[cfg(windows)]
    {
        for path in [&parent, &real] {
            assert!(!path
                .to_string_lossy()
                .chars()
                .any(|c| "\"&|<>%!\r\n".contains(c)));
        }
        let command = format!("mklink /J \"{}\" \"{}\"", parent.display(), real.display());
        use std::os::windows::process::CommandExt;
        let output = std::process::Command::new("cmd.exe")
            .raw_arg(format!("/D /C {command}"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(Store::new(parent.join("settings.json"))
        .load()
        .unwrap_err()
        .contains(if cfg!(windows) { "reparse" } else { "indirect" }));
    assert!(Store::new(parent.join("new.json"))
        .save(&settings())
        .is_err());
    assert_eq!(fs::read(target).unwrap(), retained);
}

#[test]
fn native_canonical_metadata_paths_keep_their_actual_platform_prefix() {
    let directory = crate::test_directory();
    let absolute = directory.path().canonicalize().unwrap();
    let store = Store::new(absolute.join("installations.json"));
    assert!(store.load().unwrap().is_none());
    store.save(&settings()).unwrap();
    assert_eq!(store.load().unwrap(), Some(settings()));
}
