use super::*;
use crate::profile_session::{RunOwner, State, Status};
use std::collections::VecDeque;

fn game() -> Value {
    json!({"id":"test-racing","name":"Test Racing","installPath":"C:/Games/Test","executable":"C:/Games/Test/Game.exe","installed":true,
        "installations":[{"id":"local","available":true}],"selectedInstallationId":"local","integrationKey":"assetto-corsa","provider":"assetto","integrationCompatible":true,
        "defaultBackground":{"image":"data:image/jpeg;base64,first"},"profileContext":{"status":"not-running","processSession":null},
        "integrationSetup":{"state":"setup-required","installed":false,"verified":false},"integrationContext":{"status":"waiting-game","contextSession":null,"processVerified":false}})
}
fn games(game: Value) -> Snapshot { let mut capture=Snapshot::default(); capture.games(&[game]); capture }
fn kinds(events: &[NewActivityEvent]) -> Vec<&str> { events.iter().map(|event|event.event_type.as_str()).collect() }
#[test]
fn actual_catalogue_shape_captures_process_integration_installation_and_artwork_changes() {
    let original=game(); let first=games(original.clone());
    assert!(capture::diff(&first,&first).is_empty());
    let mut running=original.clone();running["profileContext"]=json!({"status":"verified","processSession":"process-1"});
    let second=games(running.clone());let launched=capture::diff(&first,&second);
    assert_eq!(kinds(&launched),["game_process_launched"]);assert_eq!(launched[0].category,"games");
    let mut live=running.clone();live["integrationSetup"]["state"]=json!("ready");live["integrationContext"]=json!({"status":"ready","contextSession":"session-1","processVerified":true});
    let changes=capture::diff(&second,&games(live.clone()));assert!(kinds(&changes).contains(&"integration_changed"));assert!(kinds(&changes).contains(&"game_session_changed"));
    live["defaultBackground"]["image"]=json!("data:image/jpeg;base64,second");live["installations"][0]["available"]=json!(false);
    let changed=capture::diff(&games(running),&games(live));assert!(kinds(&changed).contains(&"game_updated"));
    let serialized=format!("{:?}",games(original.clone()));assert!(!serialized.contains("base64"));
    assert_eq!(kinds(&capture::diff(&second,&games(original))),["game_process_exited"]);
    let removed=capture::diff(&first,&Snapshot::default());assert_eq!(kinds(&removed),["game_removed"]);
}
#[test]
fn controller_input_sequences_do_not_log_but_presence_changes_do() {
    let mut before=Snapshot::default();before.devices("controller",&[json!({"id":"wheel","name":"Wheel","online":true,"sequence":"1","values":[1]})]);
    let mut sample=Snapshot::default();sample.devices("controller",&[json!({"id":"wheel","name":"Wheel","online":true,"sequence":"999","values":[0]})]);
    assert!(capture::diff(&before,&sample).is_empty());
    let mut offline=Snapshot::default();offline.devices("controller",&[json!({"id":"wheel","name":"Wheel","online":false})]);
    assert_eq!(kinds(&capture::diff(&before,&offline)),["controller_offline"]);
    let mut unavailable=Snapshot::default();unavailable.unavailable("controller");unavailable.preserve_unavailable(&before);assert!(capture::diff(&before,&unavailable).is_empty());
    assert_eq!(kinds(&capture::diff(&Snapshot::default(),&before)),["controller_discovered"]);
}
#[test]
fn profile_native_lifecycle_observer_keeps_brief_transitions_and_deduplicates_ticks() {
    let (sender,receiver)=mpsc::channel();let mut observer=session::Observer::new(sender,Arc::new(Mutex::new(reconcile::Reconciler::default())));
    let document=osg_profiles::default_document();let id=document.id.clone();observer.names(&[document]);
    let mut status=Status{state:State::Stopped,owner:None,applied_snapshot:None,degradation:vec![],fault:None};
    observer.observe(&status);
    status.owner=Some(RunOwner{session:"test-session".into(),generation:1,profile_id:id.clone(),profile_revision:1,game_id:None});status.state=State::Running;observer.observe(&status);observer.observe(&status);
    status.state=State::Degraded;status.degradation=vec!["Wheel unavailable".into()];observer.observe(&status);
    status.state=State::Running;status.degradation.clear();observer.observe(&status);
    status.state=State::Faulted;status.fault=Some("Output could not be restored".into());observer.observe(&status);
    status.state=State::Stopped;status.owner=None;status.fault=None;observer.observe(&status);
    let events=receiver.try_iter().flat_map(|batch|batch.events.into_iter().map(|observation|observation.event)).collect::<Vec<_>>();assert_eq!(kinds(&events),["profile_session_activated","profile_session_degraded","profile_session_recovered","profile_session_failed","profile_session_stopped"]);
    assert!(events.iter().all(|event|event.entity_id.as_deref()==Some(&id)));assert!(events.iter().all(|event|event.entity_name.as_deref()!=Some("Profile session")));
}
#[test]
fn audit_uses_safe_native_identities_errors_and_source_without_request_bodies() {
    let request=json!({"command":"companion_usb_provision","clientId":"cockpit-mcp-test","companion":{"companionId":"panel-1","name":"Panel","ssid":"private network","password":"wifi-password","secret":"pair-secret"}});
    let event=audit::mutation(&request,&json!({"ok":false,"error":"Password wifi-password was rejected"})).unwrap();
    assert_eq!(event.entity_id.as_deref(),Some("panel-1"));assert_eq!(event.source,"mcp");assert_eq!(event.level,"error");
    let value=serde_json::to_string(&event).unwrap();assert!(!value.contains("wifi-password"));assert!(!value.contains("pair-secret"));assert!(!value.contains("private network"));
    let error=audit::mutation(&json!({"command":"profile_apply","profileId":"profile-1","gameId":"game-1"}),&json!({"ok":false,"error":"The controller is offline; reconnect Wheel."})).unwrap();assert_eq!(error.entity_id.as_deref(),Some("profile-1"));assert!(error.detail.contains("reconnect Wheel"));
    let created=audit::mutation(&json!({"command":"profile_create","name":"Race"}),&json!({"ok":true,"state":{"profiles":{"document":{"id":"new-profile","name":"Race"}}}})).unwrap();assert_eq!(created.entity_id.as_deref(),Some("new-profile"));assert_eq!(created.entity_name.as_deref(),Some("Race"));
    for command in ["game_get_state","profile_status","game_telemetry_inspect","controller_list","action_invoke","action_renew","module_usb_events_read","companion_feedback"] { assert!(audit::mutation(&json!({"command":command}),&json!({"ok":true})).is_none(),"{command}"); }
    for command in ["game_integration_setup","game_integration_remove","game_integration_recover","companion_approve","companion_usb_flash","controller_update","config_apply_save","claim","release","start","stop","save","apply","update-profile"] { assert!(audit::mutation(&json!({"command":command}),&json!({"ok":true})).is_some(),"{command}"); }
}
#[test]
fn inventory_cache_success_is_silent_but_settings_and_storage_failures_remain_visible() {
    let request=json!({"command":"ui_preferences_save","key":"remembered","clientId":"inventory-poll-client"});
    for _ in 0..100 {assert!(audit::mutation(&request,&json!({"ok":true})).is_none());}
    let failed=audit::mutation(&request,&json!({"ok":false,"error":"SQLite disk full."})).unwrap();
    assert_eq!(failed.level,"error");assert_eq!(failed.message,"Remembered devices could not be saved. SQLite disk full.");assert_eq!(failed.detail,"SQLite disk full.");
    for (key,label) in [("theme","Appearance"),("devices-mode","Device view"),("canvas-navigation","Canvas navigation"),("canvas-styling","Canvas styling"),("tray","Tray settings"),("progress","Progress notifications"),("windows","Window layout"),("unknown","Preferences")] {
        let changed=audit::mutation(&json!({"command":"ui_preferences_save","key":key}),&json!({"ok":true})).unwrap();assert_eq!(changed.message,format!("{label} updated."));
    }
}
#[test]
fn writer_batches_more_than_database_append_budget_and_retains_failed_events() {
    let directory=crate::test_directory();let database=Database::for_namespace(&directory.path().join("config.json")).unwrap();
    let mut pending=(0..1500).map(|i|event("system","test","info",format!("Event {i}"),None,None)).collect::<VecDeque<_>>();
    assert!(writer::persist(&Err("temporarily unavailable".into()),&mut pending).is_err());assert_eq!(pending.len(),1500);
    writer::persist(&Ok(database.clone()),&mut pending).unwrap();assert!(pending.is_empty());assert_eq!(database.activity_query(&Default::default()).unwrap().total,1500);
}
#[test]
fn runtime_flush_query_settings_and_clear_share_durable_authority() {
    let directory=crate::test_directory();let runtime=Runtime::new(&directory.path().join("config.json"));
    assert!(runtime.audit(&json!({"command":"game_save","gameId":"racing","name":"Racing","clientId":"test-cockpit-client"}),&json!({"ok":true})));
    let page=runtime.request(&json!({"command":"activity_query","activityFilter":{"search":"Racing","eventTypes":["game_save"]}})).unwrap();assert_eq!(page["activity"]["total"],1);assert_eq!(page["activity"]["events"][0]["source"],"cockpit");
    assert_eq!(runtime.request(&json!({"command":"activity_settings_get"})).unwrap()["activitySettings"]["retentionDays"],7);
    assert_eq!(runtime.request(&json!({"command":"activity_settings_save","retentionDays":30})).unwrap()["activitySettings"]["retentionDays"],30);
    assert!(runtime.request(&json!({"command":"activity_settings_save","retentionDays":0})).is_err());assert!(runtime.request(&json!({"command":"activity_query","activityFilter":{"limit":0}})).is_err());
    assert_eq!(runtime.request(&json!({"command":"activity_clear"})).unwrap()["activityDeleted"],1);runtime.shutdown();assert!(runtime.flush().is_err());
    let reopened=Runtime::new(&directory.path().join("config.json"));assert_eq!(reopened.request(&json!({"command":"activity_settings_get"})).unwrap()["activitySettings"]["retentionDays"],30);
}
#[test]
fn gateway_and_mcp_commands_share_authorization_validation_and_idempotency() {
    let manager=Arc::new(crate::gateways::GatewayManager::mcp_test_manager());let client="activity-native-client";
    let request=|command:&str,fields:Value|{let mut fields=fields;fields["command"]=json!(command);fields["clientId"]=json!(client);manager.request(fields)};
    assert_eq!(request("activity_settings_get",json!({}))["ok"],true);
    assert_eq!(manager.request(json!({"command":"activity_query","clientId":"short"}))["ok"],false);
    assert_eq!(request("profile_create",json!({"name":"Activity Test"}))["ok"],true);
    assert_eq!(request("activity_query",json!({"activityFilter":{"eventTypes":["profile_create"]}}))["state"]["activity"]["total"],1);
    let tools=crate::mcp_tools::Tools::new(manager.clone());let mcp_client="authenticated-activity-client";
    let operation=uuid::Uuid::new_v4().to_string();let args=json!({"retentionDays":21,"operationId":operation});
    let saved=tools.call("activity_settings_save",args.clone(),mcp_client);assert_eq!(saved["ok"],true,"{saved}");assert_eq!(saved["data"]["retentionDays"],21);assert_eq!(tools.call("activity_settings_save",args,mcp_client),saved);
    let read=tools.call("activity_list",json!({"eventTypes":["profile_create"],"limit":1}),mcp_client);assert_eq!(read["ok"],true,"{read}");assert_eq!(read["data"]["total"],1);
    assert_eq!(tools.call("activity_clear",json!({}),mcp_client)["ok"],false);
    manager.mcp_authorize_client(&crate::mcp_tools::native_client(mcp_client),false);
    assert_eq!(tools.call("activity_settings_save",json!({"retentionDays":22,"operationId":uuid::Uuid::new_v4()}),mcp_client)["ok"],false);
    assert_eq!(request("activity_settings_get",json!({}))["state"]["activitySettings"]["retentionDays"],21);
    assert_eq!(request("activity_clear",json!({}))["ok"],true);manager.shutdown();assert_eq!(request("activity_query",json!({}))["ok"],false);
}

#[test]
fn background_review_setup_reason_vehicle_and_system_events_are_useful_without_ui() {
    let directory=crate::test_directory();let runtime=Runtime::new(&directory.path().join("config.json"));
    let current=Arc::new(Mutex::new(Snapshot::default()));let source=current.clone();
    current.lock().unwrap().discovery(&json!({"scanId":"scan-1","status":"scanning","proposed":0,"diagnostics":[]}));
    runtime.observe(move||source.lock().unwrap().clone());runtime.flush().unwrap();
    let query=|kind:&str|runtime.request(&json!({"command":"activity_query","activityFilter":{"eventTypes":[kind]}})).unwrap()["activity"].clone();
    assert_eq!(query("discovery_started")["total"],1);
    let mut ready=Snapshot::default();ready.discovery(&json!({"scanId":"scan-1","status":"ready","proposed":2,"artworkReady":1,"tagged":1,"diagnostics":["One library could not be read."]}));
    let mut failed_game=game();failed_game["integrationSetup"]=json!({"state":"needs-attention","message":"Close Racing before restoring its integration files."});failed_game["currentVehicle"]=json!({"id":"GT3","name":"GT3 Car"});ready.games(&[failed_game]);*current.lock().unwrap()=ready;
    let complete=query("discovery_completed");assert_eq!(complete["total"],1);assert!(complete["events"][0]["detail"].as_str().unwrap().contains("2 proposed"));
    let integration=query("integration_changed");assert_eq!(integration["events"][0]["level"],"warning");assert!(integration["events"][0]["detail"].as_str().unwrap().contains("Close Racing"));
    assert_eq!(query("game_vehicle_changed")["events"][0]["detail"],"Current vehicle: “None” → “GT3 Car”");
    current.lock().unwrap().discovery(&json!({"scanId":"scan-2","status":"failed","diagnostics":["Scan failed because the library is inaccessible."]}));assert_eq!(query("discovery_failed")["events"][0]["level"],"error");
    assert_eq!(query("discovery_failed")["total"],1);runtime.shutdown();
    // Receiver disconnection exits cleanly rather than spinning while idle.
    let (sender,receiver)=mpsc::sync_channel(1);let (lifecycle_sender,lifecycle_receiver)=mpsc::channel();drop(sender);drop(lifecycle_sender);writer::run(Err("Unavailable".into()),receiver,lifecycle_receiver,Arc::new(Mutex::new(reconcile::Reconciler::default())));
}
