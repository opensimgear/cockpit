use super::*;
use osg_profiles::activation::GameIdentity;
fn settings() -> Settings {
    Settings {
        mode: SelectionMode::Automatic,
        rules: vec![Rule {
            identity: GameIdentity {
                game_id: "dcs".into(),
                executable: "C:\\Games\\DCS\\bin\\DCS.exe".into(),
            },
            profile_id: DEFAULT_PROFILE_ID.into(),
            priority: 10,
            enabled: true,
        }],
        ..Settings::default()
    }
}
#[test]
fn missing_state_is_passive_and_rules_round_trip_without_process_authority() {
    let directory = crate::test_directory();
    let path = directory.path().join("new").join("automatic.json");
    let store = Store::new(Some(path.clone()));
    assert!(store.load().unwrap().is_none());
    assert!(!path.exists());
    store.save(&settings()).unwrap();
    assert_eq!(store.load().unwrap(), Some(settings()));
    let stale = Store::new(Some(path.clone()));
    stale.load().unwrap();
    store.save(&Settings::default()).unwrap();
    assert!(stale.save(&settings()).unwrap_err().contains("conflict"));
    assert_eq!(
        Store::new(Some(path.clone())).load().unwrap(),
        Some(Settings::default())
    );
    Store::new(None).save(&settings()).unwrap();
    assert!(Store::new(None).load().unwrap().is_none());
}
#[test]
fn corrupt_unknown_oversized_and_nonregular_settings_are_preserved() {
    let directory = crate::test_directory();
    let path = directory.path().join("automatic.json");
    let store = Store::new(Some(path.clone()));
    let mut forged = serde_json::to_value(settings()).unwrap();
    forged["rules"][0]["pid"] = serde_json::json!(42);
    let bytes = serde_json::to_vec(&forged).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    crate::persistence::test_support::record(
        &path,
        "profiles",
        "selection",
        &std::fs::read(&path).unwrap(),
    );
    assert!(store.load().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    std::fs::write(&path, vec![b' '; MAX_BYTES + 1]).unwrap();
    crate::persistence::test_support::record(
        &path,
        "profiles",
        "selection",
        &std::fs::read(&path).unwrap(),
    );
    assert!(store.load().is_err());
    std::fs::write(&path, b"{").unwrap();
    crate::persistence::test_support::record(
        &path,
        "profiles",
        "selection",
        &std::fs::read(&path).unwrap(),
    );
    assert!(store.load().is_err());
    for mutate in [
        |s: &mut Settings| s.schema_version = 2,
        |s: &mut Settings| s.manual_profile_id = "bad".into(),
        |s: &mut Settings| s.rules.push(s.rules[0].clone()),
    ] {
        let mut bad = settings();
        mutate(&mut bad);
        assert!(store.save(&bad).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{");
    }
    assert!(Store::new(Some(directory.path().to_owned()))
        .load()
        .is_err());
    assert!(Store::new(Some(PathBuf::from("relative.json")))
        .load()
        .is_err());
    assert!(Store::new(Some(
        directory.path().join("..bad").join("..").join("x.json")
    ))
    .save(&settings())
    .is_err());
    let parent = directory.path().join("file");
    std::fs::write(&parent, b"keep").unwrap();
    assert!(Store::new(Some(parent.join("automatic.json")))
        .save(&settings())
        .is_err());
    assert_eq!(std::fs::read(parent).unwrap(), b"keep");
}

#[test]
fn remembered_context_choices_round_trip_and_older_settings_load_without_them() {
    let directory = crate::test_directory();
    let path = directory.path().join("automatic.json");
    let store = Store::new(Some(path.clone()));
    let mut saved = settings();
    saved
        .preferred_profiles
        .insert("dcs".into(), DEFAULT_PROFILE_ID.into());
    store.save(&saved).unwrap();
    assert_eq!(store.load().unwrap(), Some(saved));
    let mut legacy = serde_json::to_value(settings()).unwrap();
    legacy.as_object_mut().unwrap().remove("preferredProfiles");
    let legacy_root = crate::test_directory();
    let legacy_path = legacy_root.path().join("automatic.json");
    std::fs::write(&legacy_path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert!(Store::new(Some(legacy_path)).load().unwrap().is_none());
    for (game, profile) in [("Bad Game", DEFAULT_PROFILE_ID), ("dcs", "forged")] {
        let mut invalid = settings();
        invalid
            .preferred_profiles
            .insert(game.into(), profile.into());
        assert!(store.save(&invalid).is_err());
    }
}

#[test]
fn generic_game_rules_and_choices_survive_without_a_current_machine_registration() {
    let directory = crate::test_directory();
    let store = Store::new(Some(directory.path().join("automatic.json")));
    let mut saved = settings();
    saved.rules = (1..=osg_profiles::activation::MAX_RULES)
        .map(|index| {
            let mut rule = settings().rules.remove(0);
            rule.identity.game_id = format!("game-{}", uuid::Uuid::from_u128(index as u128));
            rule.identity.executable = format!("C:\\Games\\Library {index}\\Game.exe");
            saved
                .preferred_profiles
                .insert(rule.identity.game_id.clone(), rule.profile_id.clone());
            rule
        })
        .collect();
    store.save(&saved).unwrap();
    assert_eq!(store.load().unwrap(), Some(saved.clone()));
    saved.preferred_profiles.insert(
        format!("game-{}", uuid::Uuid::new_v4()),
        DEFAULT_PROFILE_ID.into(),
    );
    assert!(
        store.save(&saved).is_err(),
        "remembered choices remain bounded"
    );
}
