use super::*;
use std::{sync::mpsc, time::Duration};

const ID: &str = "stm32:00112233445566778899aabb";
const CLIENT: &str = "workspace-client-0001";

struct RenewalOnly {
    clients: Mutex<Vec<String>>,
    result: Result<Vec<String>, String>,
}
impl Adapter for RenewalOnly {
    fn request(&self, _: Value) -> Value { panic!("lease renewal must not dispatch a device request") }
    fn cached_state(&self) -> Value { panic!("lease renewal must not collect cached display state") }
    fn workspace_snapshot(&self) -> (Arc<Value>, BTreeMap<String, String>) {
        panic!("lease renewal must not collect workspace snapshots")
    }
    fn renew_workspace_client(&self, client: &str) -> Result<Vec<String>, String> {
        self.clients.lock().unwrap().push(client.into());self.result.clone()
    }
    fn shutdown(&self) {}
}
fn setup(result: Result<Vec<String>, String>) -> (Arc<GatewayManager>, Arc<RenewalOnly>) {
    let manager = Arc::new(GatewayManager::mcp_test_manager());
    let adapter = Arc::new(RenewalOnly { clients: Mutex::new(vec![]), result });
    manager.sessions.lock().unwrap().insert(ID.into(), adapter.clone());
    (manager, adapter)
}
fn request(client: &str) -> Value {
    json!({"command":"workspace_client_renew","clientId":client,"gatewayId":ID})
}

#[test]
fn renewal_requires_exact_fields_valid_client_and_a_known_existing_session() {
    let (manager, adapter) = setup(Ok(vec!["module-1".into()]));
    let mut invalid = Vec::new();
    for field in ["clientId", "gatewayId"] {
        let mut value = request(CLIENT);value.as_object_mut().unwrap().remove(field);invalid.push(value);
    }
    for field in ["offset", "operationId", "moduleId", "ownershipToken"] {
        let mut value = request(CLIENT);value[field] = json!("forbidden");invalid.push(value);
    }
    for client in [json!(null), json!(7), json!("short"), json!("x".repeat(81)), json!("invalid-client-\nidentity")] {
        let mut value = request(CLIENT);value["clientId"] = client;invalid.push(value);
    }
    for gateway in [json!(null), json!(7), json!("stm32:11112233445566778899aabb"), json!("x".repeat(600))] {
        let mut value = request(CLIENT);value["gatewayId"] = gateway;invalid.push(value);
    }
    for value in invalid {
        let reply = manager.request(value.clone());assert_eq!(reply["ok"], false, "accepted {value}");
    }
    assert!(adapter.clients.lock().unwrap().is_empty());
    assert_eq!(manager.sessions.lock().unwrap().len(), 1, "renewal acquired another session");
    let reply = manager.request(request(CLIENT));
    assert_eq!(reply, json!({"ok":true,"state":{"workspaceRenewal":{"gatewayId":ID,"claimedModules":["module-1"]}}}));
    assert_eq!(*adapter.clients.lock().unwrap(), vec![CLIENT.to_string()]);
    manager.shutdown();
}

#[test]
fn renewal_returns_only_its_receipt_and_propagates_adapter_failure_without_display_queries() {
    let (manager, adapter) = setup(Err("existing claim expired".into()));
    assert_eq!(manager.request(request(CLIENT)), json!({"ok":false,"error":"existing claim expired"}));
    assert_eq!(*adapter.clients.lock().unwrap(), vec![CLIENT.to_string()]);
    manager.closed.store(true, Ordering::Release);
    assert_eq!(manager.request(request(CLIENT))["ok"], false);
    assert_eq!(adapter.clients.lock().unwrap().len(), 1, "closed manager forwarded a renewal");
    manager.shutdown();
}

#[test]
fn renewal_bypasses_profile_write_admission_and_coordinator_mutex() {
    let (manager, adapter) = setup(Ok(vec!["module-1".into()]));
    let admission = manager.profile_admission.write().unwrap();
    let coordinator = manager.profile_session.lock().unwrap();
    let (sent, received) = mpsc::channel();
    let caller = {
        let manager = manager.clone();
        std::thread::spawn(move || {sent.send(manager.request(request(CLIENT))).unwrap();})
    };
    let reply = received.recv_timeout(Duration::from_secs(2));
    // Release both guards before joining even on failure, so regressions fail
    // promptly rather than stranding a request thread behind the test itself.
    drop(coordinator);drop(admission);caller.join().unwrap();
    let reply = reply.expect("workspace renewal waited for profile takeover locks");
    assert_eq!(reply["ok"], true);assert_eq!(reply["state"].as_object().unwrap().len(), 1);
    assert_eq!(*adapter.clients.lock().unwrap(), vec![CLIENT.to_string()]);
    manager.shutdown();
}

#[test]
fn mcp_renewal_is_write_classified_without_operation_ledger_and_uses_private_native_client() {
    let (manager, adapter) = setup(Ok(vec!["module-1".into()]));
    let tools = crate::mcp_tools::Tools::new(manager.clone());
    let catalogue = crate::mcp_tools::catalogue();
    let renewal = catalogue.iter().find(|tool| tool["name"] == "workspace_client_renew").unwrap();
    assert!(crate::mcp_tools::is_mutation("workspace_client_renew"), "server write authorization uses this classification");
    assert_eq!(renewal["annotations"]["readOnlyHint"], false);
    assert_eq!(renewal["inputSchema"]["required"], json!(["gatewayId"]));
    assert!(renewal["inputSchema"]["properties"].get("operationId").is_none());
    let client = "authenticated-workspace-client";
    for _ in 0..2 {
        let reply = tools.call("workspace_client_renew", json!({"gatewayId":ID}), client);
        assert_eq!(reply, json!({"ok":true,"data":{"gatewayId":ID,"claimedModules":["module-1"]}}));
    }
    let native_client = crate::mcp_tools::native_client(client);
    assert_eq!(*adapter.clients.lock().unwrap(), vec![native_client.clone(), native_client.clone()]);
    let invalid = tools.call("workspace_client_renew", json!({"gatewayId":ID,"operationId":uuid::Uuid::new_v4().to_string()}), client);
    assert_eq!(invalid["ok"], false);assert_eq!(adapter.clients.lock().unwrap().len(), 2);
    manager.mcp_authorize_client(&native_client, false);
    assert_eq!(tools.call("workspace_client_renew", json!({"gatewayId":ID}), client)["ok"], false);
    assert_eq!(adapter.clients.lock().unwrap().len(), 2, "revoked MCP client reached an adapter");
    manager.shutdown();
}

#[test]
fn mcp_workspace_page_is_read_only_and_serves_issued_cache_without_adapter_queries() {
    let (manager, adapter) = setup(Ok(vec![]));
    let mut projection = manager.workspace.inner.pages.begin();
    let node = projection.encode(&json!("cached workspace ".repeat(1000))).unwrap();
    projection.commit("workspace.state").unwrap();
    let catalogue = crate::mcp_tools::catalogue();
    let page = catalogue.iter().find(|tool| tool["name"] == "workspace_read_model_page").unwrap();
    assert!(!crate::mcp_tools::is_mutation("workspace_read_model_page"));
    assert_eq!(page["annotations"]["readOnlyHint"], true);
    assert!(page["inputSchema"]["properties"].get("operationId").is_none());
    let tools = crate::mcp_tools::Tools::new(manager.clone());
    let reply = tools.call("workspace_read_model_page", json!({"id":node["$gamePage"]["id"],"offset":0}), "read-only-workspace-client");
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(reply["data"]["offset"], 0);assert_eq!(reply["data"]["chunk"].as_str().unwrap().len(), 8192);
    assert!(adapter.clients.lock().unwrap().is_empty());
    manager.shutdown();
}
