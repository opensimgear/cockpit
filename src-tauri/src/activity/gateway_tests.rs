use super::*;

struct Discovery(Arc<Mutex<Result<Vec<PortInfo>,String>>>);
impl Provider for Discovery {
    fn ports(&self)->Result<Vec<PortInfo>,String>{self.0.lock().unwrap().clone()}
    fn adapter(&self,_:&str)->Arc<dyn Adapter>{panic!("Activity discovery cannot open hardware")}
}
struct Cached(Mutex<Value>);
impl Adapter for Cached {
    fn request(&self,_:Value)->Value{panic!("Activity collection must use cached gateway state")}
    fn cached_state(&self)->Value{self.0.lock().unwrap().clone()}
    fn shutdown(&self){}
}
fn port()->PortInfo{PortInfo{path:"COM241".into(),serial_number:Some("OSG-PICO-AABBCCDD".into()),vendor_id:Some("cafe".into()),product_id:Some("4010".into()),manufacturer:Some("OpenSimGear".into()),friendly_name:Some("Gateway".into())}}
fn page(manager:&GatewayManager,kind:&str)->Value{manager.activity.request(&json!({"command":"activity_query","activityFilter":{"eventTypes":[kind]}})).unwrap()["activity"].clone()}

#[test]
fn background_collector_logs_discovery_cached_presence_and_profile_changes_without_ui() {
    let root=crate::test_directory();let anchor=root.path().join("config.json");
    let discovery=Arc::new(Mutex::new(Ok(vec![])));
    let manager=GatewayManager::with_options_at(Arc::new(Discovery(discovery.clone())),false,Some(anchor.clone()));
    manager.start_activity();manager.activity.flush().unwrap();
    assert_eq!(page(&manager,"cockpit_started")["total"],1);
    *discovery.lock().unwrap()=Ok(vec![port()]);
    assert_eq!(page(&manager,"gateway_discovered")["events"][0]["entityName"],"Gateway");
    let id="pico:OSG-PICO-AABBCCDD";
    let cached=Arc::new(Cached(Mutex::new(json!({"connected":true,"running":true,"modules":[{"id":"module-a","name":"Panel Module","connected":true}]}))));
    manager.sessions.lock().unwrap().insert(id.into(),cached.clone());
    assert_eq!(page(&manager,"gateway_online")["total"],1);assert_eq!(page(&manager,"module_discovered")["total"],1);
    manager.profiles.lock().unwrap().request(&json!({"command":"profile_create","name":"Created in background"})).unwrap();
    assert_eq!(page(&manager,"profile_created")["events"][0]["entityName"],"Created in background");
    assert_eq!(page(&manager,"profile_create")["total"],0,"Background changes cannot depend on Gateway command auditing");
    assert_eq!(page(&manager,"profile_created")["total"],1,"Unchanged reads/polls cannot add duplicate changes");
    cached.0.lock().unwrap()["connected"]=json!(false);cached.0.lock().unwrap()["modules"][0]["connected"]=json!(false);
    assert_eq!(page(&manager,"gateway_offline")["total"],1);assert_eq!(page(&manager,"module_offline")["total"],1);
    *discovery.lock().unwrap()=Err("Transient serial discovery failure".into());manager.controllers.shutdown();manager.companion_host.shutdown();manager.activity.flush().unwrap();
    assert_eq!(page(&manager,"gateway_offline")["total"],1,"An observation error must preserve known gateway state");
    manager.record_external_activity("set_autostart",true);manager.record_external_activity("mcp_settings_update",false);
    assert_eq!(page(&manager,"set_autostart")["total"],1);assert_eq!(page(&manager,"mcp_settings_update_failed")["events"][0]["source"],"mcp");
    manager.shutdown();manager.shutdown();
    let database=osg_persistence::Database::for_namespace(&anchor).unwrap();
    assert_eq!(database.activity_query(&osg_persistence::ActivityQuery{event_types:vec!["cockpit_stopped".into()],..Default::default()}).unwrap().total,1);
}

#[test]
fn inventory_polls_do_not_flood_history_while_explicit_forget_and_profile_edits_are_recorded() {
    let manager=GatewayManager::mcp_test_manager();
    let request=|revision:u64,value:Value| manager.request(json!({"command":"ui_preferences_save","key":"remembered","expectedRevision":revision,"value":value,"clientId":"inventory-noise-regression"}));
    let component=json!({"id":"offline","name":"Offline wheel","type":"controller","role":"","presence":"offline","hidden":false,"enabled":true,"owner":null,"capture":"unknown","status":"Disconnected","diagnostics":[],"connections":[],"targets":[],"evidence":[],"references":[],"firstSeen":10,"lastSeen":20,"observedAt":30,"provider":"controllers","stale":true,"forget":{"allowed":false,"reason":"Refresh first","effect":"Local metadata"}});
    for revision in 0..30 {
        let mut component=component.clone();component["lastSeen"]=json!(revision+100);
        assert_eq!(request(revision,json!({"components":[component],"forgotten":{}}))["ok"],true);
    }
    assert_eq!(page(&manager,"ui_preferences_save")["total"],0);
    let forgotten=json!({"components":[],"forgotten":{"offline":{"signature":"known","sawAbsent":false,"type":"controller","provider":"controllers"}}});
    assert_eq!(request(30,forgotten.clone())["ok"],true);
    let cleanup=page(&manager,"device_forgotten");assert_eq!(cleanup["total"],1);assert_eq!(cleanup["events"][0]["message"],"Offline wheel forgotten.");
    assert_eq!(request(31,forgotten.clone())["ok"],true);assert_eq!(page(&manager,"device_forgotten")["total"],1);
    let mut second=forgotten.clone();second["forgotten"]["missing-name"]=json!({"signature":"known","sawAbsent":false,"type":"controller","provider":"controllers"});
    let mut mcp=json!({"command":"ui_preferences_save","key":"remembered","expectedRevision":32,"value":second,"clientId":"cockpit-mcp-inventory-regression"});
    assert_eq!(manager.request(mcp.clone())["ok"],true);let events=page(&manager,"device_forgotten");assert_eq!(events["total"],2);assert_eq!(events["events"][0]["source"],"mcp");
    mcp["expectedRevision"]=json!(0);assert_eq!(manager.request(mcp)["ok"],false);assert_eq!(page(&manager,"ui_preferences_save_failed")["total"],1);
    assert_eq!(manager.request(json!({"command":"profile_create","name":"Useful activity","clientId":"inventory-noise-regression"}))["ok"],true);
    assert_eq!(page(&manager,"profile_create")["total"],1);manager.shutdown();
}
