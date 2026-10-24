use super::*;
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime};

fn invoke(window: &tauri::WebviewWindow<MockRuntime>, command: &str, body: Value) -> Result<Value, Value> {
    get_ipc_response(window, tauri::webview::InvokeRequest {
        cmd: command.into(),
        callback: tauri::ipc::CallbackFn(0),
        error: tauri::ipc::CallbackFn(1),
        url: "http://tauri.localhost".parse().unwrap(),
        body: tauri::ipc::InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: tauri::test::INVOKE_KEY.into(),
    }).map(|body| body.deserialize().unwrap())
}

#[test]
fn ipc_requires_exact_revisions_and_rotates_credentials_atomically() {
    let root = crate::test_directory();
    let path = root.path().join("mcp-v1.json");
    let server = Server::open(Arc::new(GatewayManager::mcp_test_manager()), Some(path.clone()));
    let app = mock_builder().manage(server.clone())
        .invoke_handler(tauri::generate_handler![mcp_settings_get, mcp_settings_update,
            mcp_credential_create, mcp_credential_rotate, mcp_credential_revoke])
        .build(mock_context(noop_assets())).unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    let status = invoke(&window, "mcp_settings_get", json!({})).unwrap();
    assert_eq!(status["settingsRevision"], 0);
    assert!(invoke(&window, "mcp_credential_create", json!({"name":"Missing revision","scope":"write"})).is_err());
    assert!(invoke(&window, "mcp_settings_update", json!({"settings":{"enabled":false,"port":54389}})).is_err());
    let created = invoke(&window, "mcp_credential_create", json!({"name":"IPC writer","scope":"write","expectedSettingsRevision":0})).unwrap();
    let id = created["credential"]["id"].clone();
    assert!(invoke(&window, "mcp_credential_rotate", json!({"id":id,"expectedSettingsRevision":0})).is_err());
    let rotated = invoke(&window, "mcp_credential_rotate", json!({"id":id,"expectedSettingsRevision":1})).unwrap();
    assert_ne!(created["token"], rotated["token"]);
    assert_eq!(rotated["status"]["credentials"].as_array().unwrap().len(), 1);
    assert_eq!(rotated["status"]["settingsRevision"], 2);
    let status = invoke(&window, "mcp_settings_update", json!({"settings":{"enabled":false,"port":54389,"expectedSettingsRevision":2}})).unwrap();
    assert_eq!(status["port"], 54389);
    assert_eq!(status["settingsRevision"], 3);
    assert!(!status.to_string().contains(created["token"].as_str().unwrap()));
    let revoked = invoke(&window, "mcp_credential_revoke", json!({"id":rotated["credential"]["id"],"expectedSettingsRevision":3})).unwrap();
    assert!(revoked["credentials"].as_array().unwrap().is_empty());
    server.shutdown();
    let reopened = Server::open(Arc::new(GatewayManager::mcp_test_manager()), Some(path));
    assert_eq!(reopened.status()["port"], 54389);
    assert!(reopened.status()["credentials"].as_array().unwrap().is_empty());
    reopened.shutdown();
}
