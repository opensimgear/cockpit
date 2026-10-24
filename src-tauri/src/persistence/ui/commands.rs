use super::*;
use tauri::Manager;
/// Read-only bootstrap is available to the owned progress window too. It never
/// grants that window the main portal's gateway mutation permission.
#[tauri::command]
pub(crate) async fn persistence_ui_read<R:tauri::Runtime>(window:tauri::WebviewWindow<R>,state:tauri::State<'_,crate::NativeGateway>)->Result<Value,String>{
    if !["main","profile-activation"].contains(&window.label())||!crate::local_document(&window.url().map_err(|e|e.to_string())?,window.app_handle().config().build.dev_url.as_ref()){return Err("This window cannot read Cockpit UI preferences.".into());}
    let preferences=state.0.ui_preferences.clone();
    tauri::async_runtime::spawn_blocking(move||preferences.snapshot(&FRONTEND_KEYS)).await.map_err(|_|"The UI settings worker failed.".to_owned())?
}
#[cfg(test)]
mod tests {
    use super::*;
    use tauri::test::{get_ipc_response,mock_builder,mock_context,noop_assets};
    #[test]
    fn bootstrap_reads_the_actual_native_owner_from_only_owned_local_windows(){
        let manager=Arc::new(crate::gateways::GatewayManager::mcp_test_manager());
        manager.ui_preferences.write("theme",0,json!(true)).unwrap();
        let app=mock_builder().manage(crate::NativeGateway(manager.clone())).invoke_handler(tauri::generate_handler![persistence_ui_read]).build(mock_context(noop_assets())).unwrap();
        for label in ["main","profile-activation","unexpected"]{
            let window=tauri::WebviewWindowBuilder::new(&app,label,Default::default()).build().unwrap();
            let response=get_ipc_response(&window,tauri::webview::InvokeRequest{cmd:"persistence_ui_read".into(),callback:tauri::ipc::CallbackFn(0),error:tauri::ipc::CallbackFn(1),url:window.url().unwrap(),body:tauri::ipc::InvokeBody::Json(json!({})),headers:Default::default(),invoke_key:tauri::test::INVOKE_KEY.into()});
            if label=="unexpected"{assert!(response.is_err());}else{let value:Value=response.unwrap().deserialize().unwrap();assert_eq!(value["entries"]["theme"]["revision"],1);assert_eq!(value["entries"]["theme"]["value"],true);assert!(value["entries"]["remembered"]["value"].is_null());}
        }
        manager.shutdown();
    }
}
