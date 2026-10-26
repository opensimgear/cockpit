use super::*;
use serde_json::json;

#[test]
fn defaults_seed_only_new_settings_and_roundtrip_empty_or_edited_taxonomy() {
    let directory = crate::test_directory();
    let path = directory.path().join("tag-library.json");
    let mut settings = Settings::default();
    assert_eq!(settings.tags.len(), 47);
    assert_eq!(settings.tags["beat-em-up"], "Beat 'em Up");
    assert_eq!(settings.tags["4x"], "4X");
    assert!(settings.game_tags.is_empty());
    validate_definitions(&settings.tags).unwrap();
    for tags in [BTreeMap::new(), BTreeMap::from([("my-tag".into(), "My tag".into())])] {
        settings.tags = tags;
        let decoded: Settings = serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
        assert_eq!(decoded, settings);
        decoded.validate().unwrap();
        crate::game_installations::Store::new(path.clone()).save(&settings).unwrap();
        let persisted = crate::game_installations::Store::new(path.clone()).load().unwrap().unwrap();
        assert_eq!(persisted.tags, settings.tags);
        assert!(persisted.game_tags.is_empty());
    }
    let mut legacy = serde_json::to_value(&settings).unwrap();
    legacy["categories"] = json!({});
    assert!(serde_json::from_value::<Settings>(legacy).is_err());
}

#[test]
fn taxonomy_names_identities_and_bounds_are_validated() {
    let uuid = uuid::Uuid::new_v4().to_string();
    validate_definitions(&BTreeMap::from([(uuid, "Unique".into()), ("4x".into(), "4X".into())])).unwrap();
    for (id, name) in [("Invalid", "Name"), ("-tag", "Name"), ("good", ""), ("good", " Name"),
        ("good", "Name "), ("good", "Line\nbreak")] {
        assert!(validate_definitions(&BTreeMap::from([(id.into(), name.into())])).is_err());
    }
    assert!(validate_definitions(&BTreeMap::from([("one".into(), "Name".into()), ("two".into(), "nAmE".into())])).is_err());
    assert!(validate_definitions(&BTreeMap::from([("one".into(), "a".repeat(49))])).is_err());
    let tags: BTreeMap<_, _> = (0..128).map(|i| (format!("tag-{i}"), format!("Tag {i}"))).collect();
    validate_definitions(&tags).unwrap();
    let mut excessive = tags; excessive.insert("extra".into(), "Extra".into());
    assert!(validate_definitions(&excessive).is_err());
}

#[test]
fn assignment_sets_reject_unknown_duplicate_invalid_and_excessive_ids() {
    let tags = defaults();
    validate_assignment(&[], &tags).unwrap();
    validate_assignment(&["racing".into(), "simulation".into()], &tags).unwrap();
    for ids in [vec!["missing".into()], vec!["racing".into(), "racing".into()], vec!["Invalid".into()]] {
        assert!(validate_assignment(&ids, &tags).is_err());
    }
    let all: Vec<_> = tags.keys().cloned().collect();
    validate_assignment(&all[..32], &tags).unwrap();
    assert!(validate_assignment(&all[..33], &tags).is_err());
}
