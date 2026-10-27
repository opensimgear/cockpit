use super::*;

fn page(runtime:&Runtime)->Value {runtime.request(&json!({"command":"activity_query"})).unwrap()["activity"].clone()}
fn entity_events(runtime:&Runtime,id:&str)->Vec<Value> {page(runtime)["events"].as_array().unwrap().iter().filter(|event|event["entityId"]==id).cloned().collect()}
fn fixture(before:Snapshot)->(tempfile::TempDir,Runtime,Arc<Mutex<Snapshot>>) {
    let directory=crate::test_directory();let runtime=Runtime::new(&directory.path().join("config.json"));
    let state=Arc::new(Mutex::new(before));let source=state.clone();runtime.observe(move||source.lock().unwrap().clone());runtime.flush().unwrap();
    (directory,runtime,state)
}
#[test]
fn create_and_delete_are_one_database_row_even_if_polling_precedes_the_acknowledgement() {
    let (_directory,runtime,state)=fixture(Snapshot::default());
    let create=json!({"command":"profile_create","name":"Race"});let document=json!({"id":"race","name":"Race","revision":1});
    let action=runtime.begin(&create).unwrap();state.lock().unwrap().profile_value("race",&document);
    runtime.flush().unwrap();assert!(entity_events(&runtime,"race").is_empty(),"An in-flight observed create must wait for its command.");
    runtime.audit_action(&create,&json!({"ok":true,"state":{"profiles":{"document":document},"activityChangeAfter":document}}),None,Some(&action));drop(action);
    let rows=entity_events(&runtime,"race");assert_eq!(rows.len(),1);assert_eq!(rows[0]["eventType"],"profile_create");
    let delete=json!({"command":"profile_delete","id":"race"});let action=runtime.begin(&delete).unwrap();*state.lock().unwrap()=Snapshot::default();
    runtime.flush().unwrap();assert_eq!(entity_events(&runtime,"race").len(),1);
    runtime.audit_action(&delete,&json!({"ok":true,"state":{"profiles":{"profiles":[]}}}),None,Some(&action));drop(action);
    let rows=entity_events(&runtime,"race");assert_eq!(rows.len(),2);assert_eq!(rows[0]["eventType"],"profile_delete");
    for _ in 0..3 {runtime.flush().unwrap();assert_eq!(entity_events(&runtime,"race").len(),2);}
}
#[test]
fn rapid_edits_remain_distinct_and_background_changes_survive_a_creation_scope() {
    let mut original=Snapshot::default();original.profile_value("race",&json!({"id":"race","name":"Race","revision":1}));
    let (_directory,runtime,state)=fixture(original);
    for revision in 2..=3 {
        let document=json!({"id":"race","name":format!("Race {revision}"),"revision":revision});
        let request=json!({"command":"profile_editor_save","document":document});let action=runtime.begin(&request).unwrap();
        state.lock().unwrap().profile_value("race",&document);
        runtime.audit_action(&request,&json!({"ok":true,"state":{"profiles":{"document":document},"activityChangeAfter":document}}),None,Some(&action));
    }
    assert_eq!(entity_events(&runtime,"race").len(),2);
    let request=json!({"command":"profile_create","name":"New"});let action=runtime.begin(&request).unwrap();
    {let mut state=state.lock().unwrap();state.profile_value("created",&json!({"id":"created","name":"New","revision":1}));state.profile_value("background",&json!({"id":"background","name":"Background","revision":1}));}
    runtime.flush().unwrap();
    runtime.audit_action(&request,&json!({"ok":true,"state":{"profiles":{"document":{"id":"created","name":"New","revision":1}}}}),None,Some(&action));drop(action);
    assert_eq!(entity_events(&runtime,"created").len(),1);let background=entity_events(&runtime,"background");assert_eq!(background.len(),1);assert_eq!(background[0]["source"],"native");
}
#[test]
fn every_observed_family_is_correlated_before_the_database_write() {
    let cases=vec![
        (json!({"command":"controller_update","sourceId":"wheel"}),json!({"ok":true,"state":{"controllers":{"source":{"sourceId":"wheel","name":"Wheel","online":true}}}}),"controller","wheel",json!({"name":"Wheel","online":true})),
        (json!({"command":"module_usb_connect","key":"usb"}),json!({"ok":true,"state":{"moduleUsb":{"module":{"key":"usb","present":true,"snapshot":{"connected":true,"identity":null,"route":null}}}}}),"module","usb",json!({"connected":true,"present":true,"identity":null,"route":null})),
        (json!({"command":"connect","gatewayId":"pico:one"}),json!({"ok":true,"state":{"present":true,"connected":true,"running":false}}),"gateway","pico:one",json!({"present":true,"connected":true,"running":false})),
        (json!({"command":"companion_rename","companion":{"companionId":"screen","name":"Screen"}}),json!({"ok":true,"state":{"companions":{"host":{"clients":[{"companionId":"screen","name":"Screen","connected":true}],"instances":[]}}}}),"companion","screen",json!({"connected":true,"instanceId":osg_companion_service::profile::profile_instance_id("screen"),"name":"Screen"})),
    ];
    for (request,response,kind,id,value) in cases {
        let (_directory,runtime,state)=fixture(Snapshot::default());let action=runtime.begin(&request).unwrap();
        state.lock().unwrap().insert(if kind=="companion"{"companions"}else{"devices"},kind,id,id,value);
        runtime.flush().unwrap();runtime.audit_action(&request,&response,None,Some(&action));drop(action);
        let rows=page(&runtime)["events"].as_array().unwrap().iter().filter(|row|row["eventType"]!="cockpit_started").cloned().collect::<Vec<_>>();
        assert_eq!(rows.len(),1,"{}: {rows:?}",request["command"]);
        assert_eq!(rows[0]["eventType"],request["command"]);
    }
}

#[test]
fn writer_retries_the_same_event_without_duplicate_rows_or_search_hits() {
    let directory=crate::test_directory();let database=Database::for_namespace(&directory.path().join("config.json")).unwrap();
    let captured=event("system","test","info","Unique retry marker".into(),None,None);
    for _ in 0..3 {let mut pending=std::collections::VecDeque::from([captured.clone(),captured.clone()]);writer::persist(&Ok(database.clone()),&mut pending).unwrap();assert!(pending.is_empty());}
    let page=database.activity_query(&osg_persistence::ActivityQuery{search:"Unique retry marker".into(),..Default::default()}).unwrap();assert_eq!(page.total,1);
}

#[test]
fn game_rename_is_not_relogged_as_process_exit_or_integration_update() {
    let game=json!({"id":"sim","name":"Old name","integrationSetup":{"state":"ready","installed":true},"integrationContext":{"status":"waiting-game"}});
    let mut before=Snapshot::default();before.games(std::slice::from_ref(&game));
    let (_directory,runtime,state)=fixture(before);
    let request=json!({"command":"game_save","gameId":"sim","name":"New name"});let action=runtime.begin(&request).unwrap();
    let mut after=game;after["name"]=json!("New name");state.lock().unwrap().games(std::slice::from_ref(&after));
    runtime.flush().unwrap();runtime.audit_action(&request,&json!({"ok":true,"state":{"games":{"catalogue":[after]}}}),None,Some(&action));drop(action);
    let rows=entity_events(&runtime,"sim");assert_eq!(rows.len(),1,"{rows:?}");assert_eq!(rows[0]["eventType"],"game_save");
}

#[test]
fn discovery_apply_has_one_row_even_when_its_applied_state_is_seen_before_reply() {
    let mut before=Snapshot::default();before.discovery(&json!({"scanId":"scan","status":"ready","proposed":0}));
    let (_directory,runtime,state)=fixture(before);
    let request=json!({"command":"game_discovery_apply","actions":[]});let action=runtime.begin(&request).unwrap();
    state.lock().unwrap().discovery(&json!({"scanId":"scan","status":"applied","proposed":0}));runtime.flush().unwrap();
    runtime.audit_action(&request,&json!({"ok":true,"state":{"games":{"catalogue":[],"discovery":{"scanId":"scan","status":"applied","entries":[]}}}}),None,Some(&action));drop(action);
    let rows=page(&runtime)["events"].as_array().unwrap().iter().filter(|row|row["eventType"]!="cockpit_started"&&row["eventType"]!="discovery_completed").cloned().collect::<Vec<_>>();
    assert_eq!(rows.len(),1,"{rows:?}");assert_eq!(rows[0]["eventType"],"game_discovery_apply");
}

#[test]
fn a_partial_controller_page_still_correlates_returned_rows_without_claiming_missing_rows() {
    let mut before=Snapshot::default();before.devices("controller",&[json!({"id":"wheel","name":"Wheel","online":false}),json!({"id":"other","name":"Other","online":true})]);
    let (_directory,runtime,state)=fixture(before);
    let request=json!({"command":"controller_update","sourceId":"wheel"});let action=runtime.begin(&request).unwrap();
    {let mut partial=state.lock().unwrap();partial.devices("controller",&[json!({"id":"wheel","name":"Wheel","online":true})]);partial.unavailable("controller");}
    runtime.flush().unwrap();
    runtime.audit_action(&request,&json!({"ok":true,"state":{"controllers":{"source":{"sourceId":"wheel","name":"Wheel","online":true}}}}),None,Some(&action));drop(action);
    let rows=entity_events(&runtime,"wheel");assert_eq!(rows.len(),1,"{rows:?}");assert_eq!(rows[0]["eventType"],"controller_update");
    assert!(entity_events(&runtime,"other").is_empty());
}

#[test]
fn game_add_and_integration_setup_each_have_one_row_and_keep_setup_diagnostics() {
    let (_directory,runtime,state)=fixture(Snapshot::default());
    let game=json!({"id":"sim","name":"Sim","integrationSetup":{"state":"setup-required"},"integrationContext":{"status":"waiting-game"}});
    let request=json!({"command":"game_add","name":"Sim"});let action=runtime.begin(&request).unwrap();state.lock().unwrap().games(std::slice::from_ref(&game));runtime.flush().unwrap();
    runtime.audit_action(&request,&json!({"ok":true,"state":{"games":{"addedGameId":"sim","catalogue":[game]}}}),None,Some(&action));drop(action);
    assert_eq!(entity_events(&runtime,"sim").len(),1);
    let request=json!({"command":"game_integration_setup","gameId":"sim"});let action=runtime.begin(&request).unwrap();
    let mut after=game;after["integrationSetup"]=json!({"state":"needs-attention","message":"Close Sim before restoring its integration files."});state.lock().unwrap().games(std::slice::from_ref(&after));runtime.flush().unwrap();
    runtime.audit_action(&request,&json!({"ok":true,"state":{"games":{"catalogue":[after]}}}),None,Some(&action));drop(action);
    let rows=entity_events(&runtime,"sim");assert_eq!(rows.len(),2,"{rows:?}");assert_eq!(rows[0]["eventType"],"game_integration_setup");assert_eq!(rows[0]["level"],"warning");assert!(rows[0]["detail"].as_str().unwrap().contains("Close Sim"));
}

#[test]
fn two_deliberate_identical_commands_are_not_deduplicated_as_retries() {
    let mut before=Snapshot::default();before.devices("controller",&[json!({"id":"wheel","name":"Wheel","online":true})]);
    let (_directory,runtime,_state)=fixture(before);let request=json!({"command":"controller_update","sourceId":"wheel"});
    let response=json!({"ok":true,"state":{"controllers":{"source":{"sourceId":"wheel","name":"Wheel","online":true}}}});
    for _ in 0..2 {let action=runtime.begin(&request).unwrap();runtime.audit_action(&request,&response,None,Some(&action));}
    assert_eq!(entity_events(&runtime,"wheel").len(),2);
}
