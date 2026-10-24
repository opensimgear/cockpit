use super::*;
use std::fs;

fn settings() -> Settings {
    Settings {
        schema_version: 1,
        manual_installations: BTreeMap::from([(
            "dcs".into(),
            "C:\\Games\\DCS\\bin\\DCS.exe".into(),
        )]),
        ..Settings::default()
    }
}
#[test]
fn fresh_store_does_not_read_seed_or_modify_an_old_profile_library() {
    let directory = crate::test_directory();
    let old = directory.path().join("games.json");
    let legacy=br#"{"schemaVersion":1,"profiles":[{"id":"old","devices":["unsafe"]}],"defaultProfiles":{"dcs":"old"},"manualInstallations":{"dcs":"C:\\Old\\DCS.exe"}}"#;
    fs::write(&old, legacy).unwrap();
    let store = Store::new(directory.path().join("game-installations-v1.json"));
    assert_eq!(store.load().unwrap(), None);
    let fresh = Settings::default();
    assert!(fresh.manual_installations.is_empty());
    store.save(&fresh).unwrap();
    assert_eq!(
        Store::new(directory.path().join("game-installations-v1.json"))
            .load()
            .unwrap(),
        Some(fresh)
    );
    assert_eq!(fs::read(old).unwrap(), legacy);
    let value: serde_json::Value =
        serde_json::from_slice(&serde_json::to_vec(&store.load().unwrap().unwrap()).unwrap())
            .unwrap();
    assert_eq!(
        value,
        serde_json::json!({"schemaVersion":1,"manualInstallations":{}})
    );
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
    edited
        .manual_installations
        .insert("x-plane-12".into(), "C:\\X-Plane\\X-Plane.exe".into());
    first.save(&edited).unwrap();
    assert!(stale.save(&original).unwrap_err().contains("conflict"));
    assert_eq!(Store::new(path.clone()).load().unwrap(), Some(edited));
    assert!(!path.exists());
}
#[test]
fn strict_envelope_rejects_profiles_tokens_duplicate_keys_and_oversized_files() {
    let directory = crate::test_directory();
    let path = directory.path().join("game-installations-v1.json");
    let store = Store::new(path.clone());
    for bytes in [
        r#"{"schemaVersion":1,"manualInstallations":{},"profiles":[]}"#,
        r#"{"schemaVersion":1,"manualInstallations":{},"defaultProfiles":{}}"#,
        r#"{"schemaVersion":1,"manualInstallations":{},"owner":"forged"}"#,
        r#"{"schemaVersion":1,"manualInstallations":{"dcs":"C:\\one.exe","dcs":"C:\\two.exe"}}"#,
        r#"{"schemaVersion":1,"schemaVersion":1,"manualInstallations":{}}"#,
        r#"{"schemaVersion":1,"manualInstallations":{"unknown":"C:\\one.exe"}}"#,
        r#"{"schemaVersion":2,"manualInstallations":{}}"#,
        r#"{"schemaVersion":1}"#,
        "malformed",
    ] {
        fs::write(&path, bytes).unwrap();
        crate::persistence::test_support::record(&path, "games", "installations", bytes.as_bytes());
        assert!(store.load().is_err(), "{bytes}");
        assert_eq!(fs::read(&path).unwrap(), bytes.as_bytes());
    }
    crate::persistence::test_support::record(
        &path,
        "games",
        "installations",
        &vec![b' '; MAX_BYTES + 1],
    );
    assert!(store.load().unwrap_err().contains("Oversized"));
}
#[test]
fn paths_are_bounded_absolute_and_cannot_smuggle_runtime_or_extra_games() {
    for invalid in ["", "relative.exe", "C:relative.exe", "C:\\bad\n.exe"] {
        let mut value = settings();
        value
            .manual_installations
            .insert("dcs".into(), invalid.into());
        assert!(value.validate().is_err(), "{invalid}");
    }
    let mut value = settings();
    value.manual_installations.insert(
        "dcs".into(),
        format!("C:\\{}", "x".repeat(MAX_EXECUTABLE_BYTES)),
    );
    assert!(value.validate().is_err());
    value.manual_installations.insert(
        "dcs".into(),
        format!("C:\\{}", "x".repeat(MAX_EXECUTABLE_BYTES - 3)),
    );
    value.validate().unwrap();
    value
        .manual_installations
        .insert("fifth".into(), "C:\\unknown.exe".into());
    assert!(value.validate().is_err());
    value = settings();
    value.schema_version = 0;
    assert!(value.validate().is_err());
    assert!(Store::new(PathBuf::from("relative/settings.json"))
        .load()
        .is_err());
    let directory = crate::test_directory();
    assert!(Store::new(directory.path().to_owned()).load().is_err());
    assert!(Store::new(PathBuf::from(format!(
        "{}{}sub{}..{}settings.json",
        directory.path().display(),
        std::path::MAIN_SEPARATOR,
        std::path::MAIN_SEPARATOR,
        std::path::MAIN_SEPARATOR
    )))
    .load()
    .is_err());
    let parent = directory.path().join("file");
    fs::write(&parent, b"file").unwrap();
    assert!(Store::new(parent.join("settings.json"))
        .save(&settings())
        .is_err());
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
        "installations",
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
        // A junction exercises reparse rejection without requiring the
        // symlink privilege or developer mode on Windows CI.
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

#[test]
fn dynamic_game_registry_round_trips_and_preserves_legacy_overrides() {
    let directory = crate::test_directory();
    let store = Store::new(directory.path().join("installations.json"));
    let mut value = settings();
    let id = "custom-11111111-1111-4111-8111-111111111111";
    value.custom_games.insert(
        id.into(),
        CustomGame {
            name: "A custom game".into(),
            executable: Some("C:\\Games\\Custom.exe".into()),
            library: Some(LibrarySource {
                client: "steam".into(),
                app_id: "42".into(),
                install_path: "C:\\Games".into(),
                launcher: Some("C:\\Steam\\steam.exe".into()),
            }),
            default_background: None,
        },
    );
    value.backgrounds.insert(
        "dcs".into(),
        crate::game_artwork::Background {
            image: format!("artwork:{}.jpg", "a".repeat(64)),
            position_x: 20.0,
            position_y: 75.0,
            crop: None,
        },
    );
    store.save(&value).unwrap();
    assert_eq!(store.load().unwrap(), Some(value));
    assert!(valid_game_id(id));
    assert!(valid_game_id("dcs"));
    assert!(!valid_game_id("custom-anything"));
}

#[test]
fn registry_rejects_duplicate_records_missing_background_owners_and_excess_games() {
    let directory = crate::test_directory();
    let path = directory.path().join("installations.json");
    let store = Store::new(path.clone());
    let id = "custom-11111111-1111-4111-8111-111111111111";
    let record = r#"{"name":"Game","executable":null}"#;
    fs::write(&path,format!(r#"{{"schemaVersion":1,"manualInstallations":{{}},"customGames":{{"{id}":{record},"{id}":{record}}}}}"#)).unwrap();
    crate::persistence::test_support::record(&path,"games","installations",&std::fs::read(&path).unwrap());
    assert!(store.load().is_err());
    let mut value = Settings::default();
    value.backgrounds.insert(
        id.into(),
        crate::game_artwork::Background {
            image: format!("artwork:{}.jpg", "a".repeat(64)),
            position_x: 50.0,
            position_y: 50.0,
            crop: None,
        },
    );
    assert!(value.validate().is_err());
    value.backgrounds.clear();
    for index in 0..250 {
        value.custom_games.insert(
            format!("custom-{}", uuid::Uuid::from_u128(index)),
            CustomGame {
                name: format!("Game {index}"),
                executable: None,
                library: None,
                default_background: None,
            },
        );
    }
    assert!(value.validate().unwrap_err().contains("count bound"));
}
