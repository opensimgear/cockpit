#[test]
fn game_metadata_read_is_passive_and_rejects_invalid_or_revoked_clients() {
    assert_eq!(profile_guard_operation("game_metadata_get"),None);
    assert!(!learning_interrupt(&json!({"command":"game_metadata_get"})));
    let manager=GatewayManager::mcp_test_manager();
    let client=crate::mcp_tools::native_client("wikidata-read");manager.mcp_authorize_client(&client,true);
    let before=manager.games.status();
    let request=json!({"command":"game_metadata_get","clientId":client,"gameId":"missing"});
    let reply=manager.mcp_request(request.clone());assert_eq!(reply["ok"],false);
    assert!(reply.to_string().contains("Choose a discovered game for metadata"));
    assert_eq!(manager.games.status(),before);
    manager.mcp_authorize_client(&client,false);assert_eq!(manager.mcp_request(request)["ok"],false);
    assert_eq!(manager.request(json!({"command":"game_metadata_get","clientId":"bad","gameId":"missing"}))["ok"],false);
    manager.shutdown();
}
