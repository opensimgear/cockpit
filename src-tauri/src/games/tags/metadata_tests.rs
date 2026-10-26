use super::*;

#[test]
fn tag_sets_replace_clear_and_compare_without_order() {
    let (runtime, _directory) = fixture();
    assert_eq!(runtime.status()["tags"]["racing"], "Racing");
    assert_eq!(runtime.status()["catalogue"][0]["tagIds"], json!([]));
    let save = |ids: Value, expected: Value| runtime.metadata_request(&json!({"command":"game_tags_save",
        "gameId":GAME,"tagIds":ids,"expectedTagIds":expected}));
    save(json!(["simulation", "racing"]), json!([])).unwrap();
    assert_eq!(saved(&runtime).game_tags[GAME], vec!["racing", "simulation"]);
    save(json!(["flight"]), json!(["simulation", "racing"])).unwrap();
    let before = saved(&runtime);
    assert!(save(json!([]), json!([])).is_err());
    assert!(save(json!(["missing"]), json!(["flight"])).is_err());
    assert!(save(json!(["racing", "racing"]), json!(["flight"])).is_err());
    assert!(save(json!([]), json!(["flight", "flight"])).is_err());
    assert!(save(json!("flight"), json!(["flight"])).is_err());
    assert!(save(json!([]), json!(null)).is_err());
    assert_eq!(saved(&runtime), before);
    for invalid in [json!({"command":"game_tag_definitions_save","tags":{},"expectedTags":null}),
        json!({"command":"game_tag_definitions_save","tags":[],"expectedTags":before.tags}),
        json!({"command":"game_tag_definitions_save","tags":{"one":"Same","two":"same"},"expectedTags":before.tags})] {
        assert!(runtime.metadata_request(&invalid).is_err());
    }
    assert_eq!(saved(&runtime), before);
    save(json!([]), json!(["flight"])).unwrap();
    assert!(!saved(&runtime).game_tags.contains_key(GAME));
    assert_eq!(runtime.status()["catalogue"][0]["tagIds"], json!([]));
    assert!(runtime.status().get("categories").is_none());
    assert!(runtime.status()["catalogue"][0].get("categoryId").is_none());
    runtime.shutdown();
}

#[test]
fn taxonomy_edits_preserve_assignments_and_delete_only_removed_tag_ids() {
    let (runtime, _directory) = fixture();
    runtime.metadata_request(&json!({"command":"game_tags_save","gameId":GAME,
        "tagIds":["flight","simulation"],"expectedTagIds":[]})).unwrap();
    let expected = saved(&runtime).tags;
    let mut tags = expected.clone();
    tags.insert("flight".into(), "Aviation".into());
    tags.remove("simulation");
    runtime.metadata_request(&json!({"command":"game_tag_definitions_save","tags":tags,"expectedTags":expected})).unwrap();
    assert_eq!(saved(&runtime).game_tags[GAME], vec!["flight"]);
    let before = saved(&runtime);
    assert!(runtime.metadata_request(&json!({"command":"game_tag_definitions_save","tags":{},"expectedTags":expected})).is_err());
    assert_eq!(saved(&runtime), before);
    runtime.metadata_request(&json!({"command":"game_tag_definitions_save","tags":{},"expectedTags":tags})).unwrap();
    assert!(saved(&runtime).tags.is_empty());
    assert!(saved(&runtime).game_tags.is_empty());
    assert!(runtime.status()["tags"].as_object().unwrap().is_empty());
    runtime.shutdown();
}

#[test]
fn tag_edits_are_atomic_against_concurrent_preparation_and_reject_unknown_fields() {
    let (runtime, _directory) = fixture();
    let request = json!({"command":"game_tags_save","gameId":GAME,"tagIds":["racing"],"expectedTagIds":[]});
    let prepared = runtime.prepare_metadata(&request).unwrap();
    runtime.metadata_request(&request).unwrap();
    assert!(runtime.commit_metadata(prepared).is_err());
    let before = saved(&runtime);
    for request in [json!({"command":"game_tags_save","gameId":"missing","tagIds":[],"expectedTagIds":[]}),
        json!({"command":"game_tags_save","gameId":GAME,"tagIds":[],"expectedTagIds":["racing"],"extra":true}),
        json!({"command":"game_tag_definitions_save","tags":{},"expectedTags":before.tags,"extra":true}),
        json!({"command":"game_categories_save","categories":{}})] {
        assert!(runtime.metadata_request(&request).is_err());
    }
    assert_eq!(saved(&runtime), before);
    runtime.shutdown();
}
