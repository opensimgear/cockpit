use super::*;

#[test]
fn preference_revision_fences_the_same_game_without_conflicting_with_another_game() {
    let manager = GatewayManager::mcp_test_manager();
    let directory = crate::test_directory();
    let request = |mut value: Value| {
        value["clientId"] = json!("game-preference-revision-test");
        manager.mcp_request(value)
    };
    let fixture = |name: &str| {
        let installation = directory.path().join(name);
        std::fs::create_dir(&installation).unwrap();
        let executable = installation.join("game.exe");
        std::fs::write(&executable, b"fixture executable").unwrap();
        let game_id = manager.games.register_test_game(name, executable.to_str().unwrap());
        let created = request(json!({"command":"profile_create","name":name}));
        assert_eq!(created["ok"], true, "{created}");
        let mut document = created["state"]["profiles"]["document"].clone();
        document["eligibility"]["gameIds"] = json!([game_id]);
        let saved = request(json!({"command":"profile_editor_save","document":document,
            "assignments":[],"expectedRevision":document["revision"]}));
        assert_eq!(saved["ok"], true, "{saved}");
        (game_id, document["id"].clone())
    };
    let (game_id, profile_id) = fixture("First Game");
    let (other_game, other_profile) = fixture("Second Game");
    let selector = json!({"scope":"game-preference","gameId":game_id});
    let original = manager.local_settings_snapshot(&selector).unwrap();
    let set_preference = |game: &str, profile: &Value| {
        let snapshot = manager.local_settings_snapshot(&json!({"scope":"game-preference","gameId":game})).unwrap();
        let saved = request(json!({"command":"profile_local_settings_save",
            "scope":"game-preference","gameId":game,"expectedSettingsRevision":snapshot["revision"],
            "expectedActiveOwner":null,"settings":{"profileId":profile}}));
        assert_eq!(saved["ok"], true, "{saved}");
    };
    let remove = |expected: &Value| request(json!({
        "command":"profile_local_settings_save",
        "scope":"game-preference","gameId":game_id,"expectedSettingsRevision":expected,
        "expectedActiveOwner":null,"settings":{"remove":true}
    }));

    set_preference(&other_game, &other_profile);
    let unrelated = remove(&original["revision"]);
    assert_eq!(unrelated["ok"], true, "{unrelated}");
    set_preference(&game_id, &profile_id);
    let stale = remove(&original["revision"]);
    assert_eq!(stale["ok"], false, "{stale}");
    assert!(stale["error"].as_str().unwrap().contains("revision conflict"));

    let current = manager.local_settings_snapshot(&selector).unwrap();
    let accepted = remove(&current["revision"]);
    assert_eq!(accepted["ok"], true, "{accepted}");
    let other = manager.local_settings_snapshot(&json!({"scope":"game-preference","gameId":other_game})).unwrap();
    assert_eq!(other["settings"]["profileId"], other_profile);
    manager.shutdown();
}

#[test]
fn registered_game_local_settings_are_readable_and_unknown_ids_are_rejected() {
    let manager = GatewayManager::mcp_test_manager();
    let directory = crate::test_directory();
    let executable = directory.path().join("Registered Game.exe");
    std::fs::write(&executable, b"fixture executable").unwrap();
    let game_id = manager
        .games
        .register_test_game("Registered Game", executable.to_str().unwrap());
    let request = |mut value: Value| {
        value["clientId"] = json!("game-settings-test-client");
        manager.mcp_request(value)
    };

    let read = request(json!({
        "command":"profile_local_settings_get",
        "scope":"game-rule",
        "gameId":game_id
    }));
    assert_eq!(read["ok"], true, "{read}");
    assert!(read["state"]["localSettings"]["settings"]["rule"].is_null());

    let saved = request(json!({
        "command":"profile_local_settings_save",
        "scope":"game-rule",
        "gameId":game_id,
        "expectedSettingsRevision":read["state"]["localSettings"]["revision"],
        "expectedActiveOwner":null,
        "settings":{"profileId":osg_profiles::DEFAULT_PROFILE_ID,"priority":0,"enabled":true}
    }));
    if cfg!(windows) {
        assert_eq!(saved["ok"], true, "{saved}");
        assert_eq!(
            saved["state"]["localSettings"]["settings"]["rule"]["identity"]["gameId"],
            game_id
        );
    } else {
        assert_eq!(saved["ok"], false, "{saved}");
        assert_eq!(saved["error"], "The selected installation has no native process monitoring support.");
        let current = request(json!({"command":"profile_local_settings_get","scope":"game-rule","gameId":game_id}));
        assert_eq!(current, read);
    }

    let invalid = request(json!({
        "command":"profile_local_settings_get",
        "scope":"game-preference",
        "gameId":"Bad Game"
    }));
    assert_eq!(invalid["ok"], false, "{invalid}");
    assert!(game(&json!({"gameId":game_id})).is_ok());
    assert!(game(&json!({"gameId":"Bad Game"})).is_err());
    manager.shutdown();
}
