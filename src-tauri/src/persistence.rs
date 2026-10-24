//! Native persistence operations execute on blocking command workers.
use serde_json::{json, Value};

#[tauri::command]
pub(crate) async fn persistence_status(database: tauri::State<'_, osg_persistence::Database>) -> Result<Value, String> {
    let database = database.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        database.integrity_check()?;
        Ok(json!({
            "sqliteVersion":osg_persistence::SQLITE_RUNTIME_VERSION,
            "processId":std::process::id(),
            "profileCount":database.profile_index()?.len(),
            "path":database.path(),"journalMode":"delete","synchronous":"full",
            "busyTimeoutMs":osg_persistence::BUSY_TIMEOUT.as_millis(),"healthy":true}))
    }).await.map_err(|_| "The persistence worker failed.".to_string())?
}

#[cfg(test)]
pub(crate) mod test_support;

/// A typed designer boundary; this accepts no SQL, domains, paths or secret keys.
#[tauri::command]
pub(crate) async fn persistence_workspace(request: Value, state: tauri::State<'_, crate::NativeGateway>) -> Result<Value,String> {
    let service=state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let operation=request["operation"].as_str().ok_or("Missing designer operation.")?;
        let mut fields=request.as_object().ok_or("Invalid designer request.")?.clone();
        fields.remove("operation");
        let command=match operation { "read"=>"designer_workspace_read", "write"=>"designer_workspace_write", _=>return Err("Unknown designer operation.".into()) };
        fields.insert("command".into(),json!(command)); fields.insert("clientId".into(),json!("designer-sqlite-window"));
        let result=service.request(Value::Object(fields));
        if result["ok"]!=true { return Err(result["error"]["message"].as_str().or_else(||result["error"].as_str()).unwrap_or("Designer storage failed.").to_owned()); }
        Ok(result["state"]["designerWorkspace"].clone())
    }).await.map_err(|_|"The designer storage worker failed.".to_owned())?
}
