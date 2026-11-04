use super::*;

fn rpc(endpoint: &str, token: &str, method: &str, params: Value) -> Value {
    tests::request(endpoint, Some(token), json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .unwrap().into_json().unwrap()
}

pub(super) fn listen_observations(endpoint: &str, token: &str, uris: &[&str]) -> impl Iterator<Item = Value> {
    use std::io::{BufRead, BufReader};
    let stream = ureq::post(endpoint).timeout(Duration::from_secs(5))
        .set("Accept", "application/json, text/event-stream")
        .set("MCP-Protocol-Version", VERSIONS[1])
        .set("Mcp-Method", "subscriptions/listen")
        .set("Authorization", &format!("Bearer {token}"))
        .send_json(json!({"jsonrpc":"2.0","id":42,"method":"subscriptions/listen","params":{
            "notifications":{"resourceSubscriptions":uris},
            "_meta":{"io.modelcontextprotocol/protocolVersion":VERSIONS[1],
                "io.modelcontextprotocol/clientInfo":{"name":"observation-test","version":"1"},
                "io.modelcontextprotocol/clientCapabilities":{}}
        }})).unwrap();
    assert!(stream.header("Content-Type").unwrap().starts_with("text/event-stream"));
    BufReader::new(stream.into_reader()).lines().filter_map(|line| {
        let line = line.unwrap();
        line.strip_prefix("data:").map(|data| serde_json::from_str(data).unwrap())
    })
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
    assert_ne!(info["result"]["capabilities"]["resources"]["subscribe"], true);
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
fn modern_observation_stream_delivers_baseline_and_closes_on_revocation() {
    let server = Server::open(Arc::new(GatewayManager::mcp_test_manager()), None);
    let created = server.create_credential("Observation reader".into(), "read".into(), 0).unwrap();
    let token = created["token"].as_str().unwrap();
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    server.configure(SettingsUpdate { expected_settings_revision:1, enabled:true, port }).unwrap();
    let endpoint = format!("http://127.0.0.1:{port}/mcp");
    let uri = "cockpit://observations/profiles.library";
    assert_eq!(rpc(&endpoint, token, "resources/templates/list", json!({}))
        ["result"]["resourceTemplates"][0]["uriTemplate"], "cockpit://observations/{topic}");
    let read = rpc(&endpoint, token, "resources/read", json!({"uri":uri}));
    let body: Value = serde_json::from_str(read["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["frame"]["kind"], "snapshot");

    let missing = "cockpit://observations/can.inputs.00112233445566778899aabb.00112233445566778899aabbccddeeff";
    assert!(rpc(&endpoint, token, "resources/read", json!({"uri":missing}))["error"].is_object());
    let mut unavailable = listen_observations(&endpoint, token, &[uri, missing]);
    assert_eq!(unavailable.next().unwrap()["method"], "notifications/subscriptions/acknowledged");
    assert!(unavailable.next().unwrap()["error"].is_object());
    drop(unavailable);

    let permits = server.subscriptions.try_acquire_many(8).unwrap();
    let mut busy = listen_observations(&endpoint, token, &[uri]);
    assert_eq!(busy.next().unwrap()["method"], "notifications/subscriptions/acknowledged");
    assert!(busy.next().unwrap()["error"].is_object());
    assert!(rpc(&endpoint, token, "tools/list", json!({}))["result"]["tools"].is_array());
    drop(busy);
    drop(permits);

    let mut stream = listen_observations(&endpoint, token, &[uri,uri,"cockpit://unknown"]);
    let ack = stream.next().unwrap();
    assert_eq!(ack["method"], "notifications/subscriptions/acknowledged");
    let snapshot = stream.next().unwrap();
    assert_eq!(snapshot["method"], "notifications/resources/updated");
    assert_eq!(snapshot["params"]["uri"], uri);
    assert_eq!(snapshot["params"]["_meta"]["opensimgear.com/observation"]["kind"], "snapshot");
    let created = server.manager.request(json!({"command":"profile_create",
        "clientId":"mcp-observation-fixture","name":"Observed stream change"}));
    assert_eq!(created["ok"], true, "{created}");
    let change = stream.next().unwrap();
    assert_eq!(change["method"], "notifications/resources/updated");
    assert!(change["params"]["_meta"]["opensimgear.com/observation"].to_string().contains("Observed stream change"));
    let id = server.status()["credentials"][0]["id"].as_str().unwrap().to_owned();
    server.revoke_credential(&id, 2).unwrap();
    assert_eq!(stream.next().unwrap()["id"], 42);
    server.shutdown();
}

#[test]
fn live_observation_rechecks_external_credential_revocation_before_delivery() {
    let root = crate::test_directory();
    let path = root.path().join("mcp-v1.json");
    let server = Server::open(Arc::new(GatewayManager::mcp_test_manager()), Some(path.clone()));
    let created = server.create_credential("External revocation reader".into(), "read".into(), 0).unwrap();
    let token = created["token"].as_str().unwrap();
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    server.configure(SettingsUpdate { expected_settings_revision: 1, enabled: true, port }).unwrap();
    let endpoint = format!("http://127.0.0.1:{port}/mcp");
    let uri = "cockpit://observations/profiles.library";
    let mut stream = listen_observations(&endpoint, token, &[uri]);
    assert_eq!(stream.next().unwrap()["method"], "notifications/subscriptions/acknowledged");
    assert_eq!(stream.next().unwrap()["method"], "notifications/resources/updated");
    let mut external = Store::open(Some(path));
    let mut updated = external.settings.clone();
    updated.credentials.clear();
    external.save(updated).unwrap();
    let result = server.manager.request(json!({"command":"profile_create",
        "clientId":"mcp-external-revocation-fixture","name":"Post-revocation update"}));
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(stream.next().unwrap()["id"], 42);
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

#[test]
fn debug_controller_streaming_requires_write_credentials() {
    let server=Server::open(Arc::new(GatewayManager::mcp_test_manager()),None);
    let reader=server.create_credential("Debug reader".into(),"read".into(),0).unwrap();
    let writer=server.create_credential("Debug writer".into(),"write".into(),1).unwrap();
    let reservation=std::net::TcpListener::bind("127.0.0.1:0").unwrap();let port=reservation.local_addr().unwrap().port();drop(reservation);
    server.configure(SettingsUpdate{expected_settings_revision:2,enabled:true,port}).unwrap();
    let generation=server.state.lock().unwrap().listener.as_ref().unwrap().generation;
    let reader=server.authenticate(reader["token"].as_str().unwrap(),generation).unwrap();
    let writer=server.authenticate(writer["token"].as_str().unwrap(),generation).unwrap();
    let input=crate::mcp_tools::is_mutation("profile_debug_controller_input");
    let status=crate::mcp_tools::is_mutation("profile_debug_controller_status");
    assert!(input);assert!(!status);
    assert!(!server.authorized(&reader,input));assert!(server.authorized(&reader,status));
    assert!(server.authorized(&writer,input));
    server.shutdown();
}
