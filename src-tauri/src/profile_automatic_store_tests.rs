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
    assert!(!path.parent().unwrap().exists());
    store.save(&settings()).unwrap();
    assert_eq!(store.load().unwrap(), Some(settings()));
    let before = std::fs::read(&path).unwrap();
    assert!(store
        .save_with(&Settings::default(), |_, _| Err(
            "injected replacement error".into()
        ))
        .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
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
    assert!(store.load().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    std::fs::write(&path, vec![b' '; MAX_BYTES + 1]).unwrap();
    assert!(store.load().is_err());
    std::fs::write(&path, b"{").unwrap();
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
