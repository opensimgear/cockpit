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

fn metadata(labels: &[&str], tags: &BTreeMap<String, String>) -> Vec<String> {
    from_metadata(tags, &labels.iter().map(|label| (*label).into()).collect::<Vec<_>>())
}

#[test]
fn platform_labels_match_exact_taxonomy_names_starter_ids_and_equivalent_aliases() {
    let tags = defaults();
    assert_eq!(metadata(&[" FIRST-PERSON shooter ", "FPS", "Role-playing", "RPG",
        "Real-time Strategy", "Third Person Shooter", "Action RPG", "Simulation",
        "Massively Multiplayer", "Multiplayer", "Single-player", "Steam Achievements",
        "Unknown genre", "Turn Based", "Racing adventure"], &tags),
        ["action-rpg", "fps", "rpg", "rts", "simulation", "third-person-shooter"]);
    assert_eq!(metadata(&["Massively Multiplayer Online Role-playing Game", "MMORPG"], &tags), ["mmorpg"]);
    assert_eq!(metadata(&["Beat 'em Up", "4X", "Turn-based Strategy", "Virtual Reality"], &tags),
        ["4x", "beat-em-up", "turn-based-strategy", "vr"]);
}

#[test]
fn platform_mapping_respects_custom_renamed_and_deleted_taxonomy() {
    let mut tags = defaults();
    tags.insert("fps".into(), "Precision".into());
    tags.insert("custom".into(), "Cozy exploration".into());
    tags.remove("rpg");
    assert_eq!(metadata(&["First Person Shooter", "Precision", "cozy exploration", "RPG", "Role-playing"], &tags),
        ["custom", "fps"]);
    assert!(metadata(&["Racing", "FPS"], &BTreeMap::new()).is_empty());
    // Similar spelling does not make a platform label a partial match.
    assert!(metadata(&["Cozy", "Cozy exploration adventure", "custom"], &tags).is_empty());
    tags.insert("other".into(), "Cozy-exploration".into());
    assert!(metadata(&["Cozy exploration"], &tags).is_empty());
}

#[test]
fn metadata_labels_are_bounded_control_free_deduplicated_and_sorted() {
    let tags: BTreeMap<_, _> = (0..40).map(|i| (format!("tag-{i:02}"), format!("Genre {i:02}"))).collect();
    let mut labels: Vec<_> = tags.values().cloned().collect();
    labels.reverse();
    labels.extend(["Genre 00".into(), "Genre\n01".into(), "Genre\0 02".into(), " ".into(), "!".repeat(129)]);
    let result = from_metadata(&tags, &labels);
    assert_eq!(result, tags.keys().take(32).cloned().collect::<Vec<_>>());
    validate_assignment(&result, &tags).unwrap();
    assert!(metadata(&["Genre\n01", "Genre\0 02", "Genre\t03"], &tags).is_empty());
    let oversized = BTreeMap::from([("oversized".into(), "a".repeat(129))]);
    assert!(from_metadata(&oversized, &["a".repeat(129)]).is_empty());
}

#[test]
fn clearing_and_deleting_definitions_preserve_initialized_empty_assignment() {
    let mut settings = Settings::default();
    let game: crate::game_installations::GameRecord = serde_json::from_value(json!({
        "name":"Example", "installations":{}, "selectedInstallation":null,
        "integration":null, "defaultBackground":null
    })).unwrap();
    settings.games.insert("example".into(), game);
    save_assignment(&mut settings, &json!({"gameId":"example", "tagIds":[], "expectedTagIds":[]})).unwrap();
    assert_eq!(settings.game_tags.get("example"), Some(&vec![]));
    save_assignment(&mut settings, &json!({"gameId":"example", "tagIds":["racing"], "expectedTagIds":[]})).unwrap();
    let expected = settings.tags.clone();
    save_definitions(&mut settings, &json!({"tags":{}, "expectedTags":expected})).unwrap();
    assert_eq!(settings.game_tags.get("example"), Some(&vec![]));
    let decoded: Settings = serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
    assert_eq!(decoded.game_tags.get("example"), Some(&vec![]));
    decoded.validate().unwrap();
}
