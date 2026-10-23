//! Native persistence operations execute on blocking command workers.
use serde_json::{json, Value};

#[tauri::command]
pub(crate) async fn persistence_status(database: tauri::State<'_, osg_persistence::Database>) -> Result<Value, String> {
    let database = database.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        database.integrity_check()?;
        Ok(json!({"schemaVersion":osg_persistence::SCHEMA_VERSION,
            "processId":std::process::id(),
            "processId":std::process::id(),
            "path":database.path(),"journalMode":"delete","synchronous":"full",
            "busyTimeoutMs":osg_persistence::BUSY_TIMEOUT.as_millis(),"healthy":true}))
    }).await.map_err(|_| "The persistence worker failed.".to_string())?
}
