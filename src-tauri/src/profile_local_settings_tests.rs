use super::*;

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
    assert_eq!(saved["ok"], true, "{saved}");
    assert_eq!(
        saved["state"]["localSettings"]["settings"]["rule"]["identity"]["gameId"],
        game_id
    );

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
