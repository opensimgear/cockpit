use super::*;
#[test]
fn ui_settings_mcp_uses_the_same_native_rows_revisions_and_validation_as_the_portal(){
    let manager=Arc::new(GatewayManager::mcp_test_manager());let tools=Tools::new(manager.clone());let client="ui-preferences-mcp";
    let mutate=|key:&str,revision:u64,value:Value|tools.call("ui_preferences_save",json!({"operationId":uuid::Uuid::new_v4().to_string(),"key":key,"expectedRevision":revision,"value":value}),client);
    let snapshot=tools.call("ui_preferences_get",json!({}),client);assert_eq!(snapshot["data"]["entries"]["theme"]["revision"],0);
    let saved=mutate("theme",0,json!(true));assert_eq!(saved["ok"],true,"{saved}");assert_eq!(manager.ui_preferences.read("theme").unwrap().revision,1);
    let stale=mutate("theme",0,json!(false));assert_eq!(stale["error"]["code"],"revision_conflict");assert_eq!(tools.call("ui_preferences_get",json!({"key":"theme"}),client)["data"]["value"],true);
    for(key,value)in [("devices-mode",json!("topology")),("canvas-navigation",json!({"wheelAction":"zoom"})),("canvas-styling",json!({"arrowStyle":"orthogonal","cornerRadius":8,"collapseParallelWires":false})),("progress",json!({"delivery":"window","position":"top-right"})),("remembered",json!({"components":[],"forgotten":{"offline":{"signature":"observed","sawAbsent":false}}}))]{let result=mutate(key,0,value.clone());assert_eq!(result["ok"],true,"{result}");assert_eq!(manager.ui_preferences.read(key).unwrap().value,value);}
    assert_eq!(mutate("progress",1,json!({"delivery":"window","position":"center"}))["ok"],false);assert_eq!(mutate("tray",0,json!({"closeToTray":true,"minimizeToTray":true}))["ok"],false);
    assert_eq!(tools.call("ui_preferences_get",json!({"key":"secrets"}),client)["error"]["code"],"invalid_arguments");
    assert_eq!(tools.call("ui_preferences_save",json!({"operationId":uuid::Uuid::new_v4().to_string(),"key":"theme","expectedRevision":1,"value":false,"sql":"unsafe"}),client)["error"]["code"],"invalid_arguments");
    manager.shutdown();
}
