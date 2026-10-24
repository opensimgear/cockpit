use super::*;

fn rpc(endpoint: &str, token: &str, method: &str, params: Value) -> Value {
    tests::request(endpoint, Some(token), json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .unwrap().into_json().unwrap()
}

#[test]
fn http_resources_tool_results_and_capacity_remain_bounded_and_authenticated() {
    let server = Server::open(Arc::new(GatewayManager::mcp_test_manager()), None);
    let created = server.create_credential("Transport writer".into(), "write".into(), 0).unwrap();
    let token = created["token"].as_str().unwrap();
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    server.configure(SettingsUpdate { expected_settings_revision:1, enabled:true, port }).unwrap();
    let endpoint = format!("http://127.0.0.1:{port}/mcp");
    let info = rpc(&endpoint, token, "initialize", json!({"protocolVersion":VERSIONS[0],"capabilities":{},"clientInfo":{"name":"native-transport-test","version":"1"}}));
    assert_eq!(info["result"]["serverInfo"]["name"], "opensimgear-cockpit");
    assert!(rpc(&endpoint, token, "tools/list", json!({"cursor":"unexpected"}))["error"].is_object());
    assert!(rpc(&endpoint, token, "resources/list", json!({"cursor":"unexpected"}))["error"].is_object());
    let resources = rpc(&endpoint, token, "resources/list", json!({}));
    let listed = resources["result"]["resources"].as_array().unwrap();
    assert!(!listed.is_empty());
    for resource in listed {
        let result = rpc(&endpoint, token, "resources/read", json!({"uri":resource["uri"]}));
        let text = result["result"]["contents"][0]["text"].as_str().unwrap_or_else(|| panic!("Resource read failed: {result}"));
        assert_eq!(serde_json::from_str::<Value>(text).unwrap()["ok"], true);
    }
    assert!(rpc(&endpoint, token, "resources/read", json!({"uri":"cockpit://missing"}))["error"].is_object());
    let status = rpc(&endpoint, token, "tools/call", json!({"name":"mcp_settings_get","arguments":{}}));
    assert_eq!(status["result"]["structuredContent"]["ok"], true);
    assert_eq!(status["result"]["structuredContent"]["data"]["settingsRevision"], 2);
    let invalid = rpc(&endpoint, token, "tools/call", json!({"name":"mcp_settings_update","arguments":{}}));
    assert_eq!(invalid["result"]["isError"], true);
    let operation = uuid::Uuid::new_v4().to_string();
    let params = json!({"name":"mcp_credential_create","arguments":{"name":"Through HTTP","scope":"read","expectedSettingsRevision":2,"operationId":operation}});
    let result = rpc(&endpoint, token, "tools/call", params);
    assert_eq!(result["result"]["structuredContent"]["ok"], true);
    assert_eq!(server.status()["credentials"].as_array().unwrap().len(), 2);
    let permits = server.calls.clone().try_acquire_many_owned(12).unwrap();
    let busy = rpc(&endpoint, token, "tools/call", json!({"name":"mcp_settings_get","arguments":{}}));
    assert_eq!(busy["result"]["structuredContent"]["error"]["code"], "busy");
    assert!(rpc(&endpoint, token, "resources/read", json!({"uri":"cockpit://status"}))["error"].is_object());
    // Stop/recovery calls retain their reserved admission capacity.
    let urgent = rpc(&endpoint, token, "tools/call", json!({"name":"profile_stop","arguments":{}}));
    assert_ne!(urgent["result"]["structuredContent"]["error"]["code"], "busy");
    drop(permits);
    server.shutdown();
}

#[test]
fn transport_limits_preserve_operation_identity_without_returning_oversized_data() {
    assert_eq!(transport::bounded(json!({"ok":true})), json!({"ok":true}));
    let large = json!({"ok":true,"data":"a".repeat(RESPONSE_LIMIT / 2)});
    assert_eq!(transport::bounded(large)["error"]["code"], "response_limit");
    let id = json!(uuid::Uuid::new_v4().to_string());
    let error = transport::operation_error("outcome_pending", "Reconcile", Some(id.clone()));
    assert_eq!(error["operationId"], id);
    assert!(transport::operation_error("native_failure", "Inspect", Some(json!(1))).get("operationId").is_none());
}
