use super::*;

#[test]
fn profile_commands_each_persist_one_action_across_all_document_mutations() {
    let manager=GatewayManager::mcp_test_manager();manager.start_activity();manager.activity.flush().unwrap();
    let request=|mut value:Value|{value["clientId"]=json!("activity-once-native-regression");let response=manager.request(value);assert_eq!(response["ok"],true,"{response}");response};
    let rows=|id:&str|manager.activity.request(&json!({"command":"activity_query","activityFilter":{"entityId":id}})).unwrap()["activity"]["events"].as_array().unwrap().clone();
    let mut document=request(json!({"command":"profile_create","name":"One action"}))["state"]["profiles"]["document"].clone();
    let id=document["id"].as_str().unwrap().to_owned();assert_eq!(rows(&id).len(),1);
    for command in ["profile_rename","profile_save","profile_editor_save","profile_draft_save","profile_assignments_save"] {
        let before=rows(&id).len();
        let mut mutation=json!({"command":command,"expectedRevision":document["revision"]});
        match command {
            "profile_rename"=>{mutation["id"]=json!(id);mutation["name"]=json!("Renamed once");},
            "profile_assignments_save"=>{mutation["id"]=json!(id);mutation["assignments"]=json!([]);},
            _=>{mutation["document"]=document.clone();if command!="profile_save"{mutation["assignments"]=json!([]);}},
        }
        document=request(mutation)["state"]["profiles"]["document"].clone();
        let events=rows(&id);assert_eq!(events.len(),before+1,"{command}: {events:?}");assert_eq!(events[0]["eventType"],command);
    }
    let copy=request(json!({"command":"profile_duplicate","id":id,"expectedRevision":document["revision"],"name":"Copy"}))["state"]["profiles"]["document"].clone();
    let copy_id=copy["id"].as_str().unwrap();let copied=rows(copy_id);assert_eq!(copied.len(),1);assert_eq!(copied[0]["eventType"],"profile_duplicate");
    let imported=request(json!({"command":"profile_import","document":document,"name":"Imported"}))["state"]["profiles"]["document"].clone();
    let imported_id=imported["id"].as_str().unwrap();let imported_rows=rows(imported_id);assert_eq!(imported_rows.len(),1);assert_eq!(imported_rows[0]["eventType"],"profile_import");
    let before=rows(&id).len();request(json!({"command":"profile_delete","id":id,"expectedRevision":document["revision"]}));let deleted=rows(&id);assert_eq!(deleted.len(),before+1);assert_eq!(deleted[0]["eventType"],"profile_delete");
    manager.shutdown();
}

#[test]
fn profile_apply_stop_and_recover_do_not_duplicate_the_session_observer() {
    let manager=GatewayManager::mcp_test_manager();manager.start_activity();manager.activity.flush().unwrap();
    for command in ["profile_apply","profile_stop","profile_recover"] {
        let before=manager.activity.request(&json!({"command":"activity_query","activityFilter":{"categories":["profiles"]}})).unwrap()["activity"]["total"].as_u64().unwrap();
        let mut request=json!({"command":command,"clientId":"activity-once-lifecycle-regression"});
        if command=="profile_apply"{request["id"]=json!(osg_profiles::DEFAULT_PROFILE_ID);request["expectedRevision"]=json!(1);}
        let response=manager.request(request);assert_eq!(response["ok"],true,"{command}: {response}");
        let after=manager.activity.request(&json!({"command":"activity_query","activityFilter":{"categories":["profiles"]}})).unwrap()["activity"].clone();
        assert_eq!(after["total"],before+1,"{command}: {after}");
        assert_eq!(after["events"][0]["eventType"],command);
    }
    manager.shutdown();
}

#[test]
fn mcp_replay_is_one_action_but_a_new_operation_with_the_same_input_is_another() {
    let manager=Arc::new(GatewayManager::mcp_test_manager());manager.start_activity();manager.activity.flush().unwrap();
    let client="activity-once-mcp-client";manager.mcp_authorize_client(&crate::mcp_tools::native_client(client),true);
    let tools=crate::mcp_tools::Tools::new(manager.clone());let first=json!({"name":"Repeated name","operationId":uuid::Uuid::new_v4()});
    let result=tools.call("profile_create",first.clone(),client);assert_eq!(result["ok"],true,"{result}");assert_eq!(tools.call("profile_create",first,client),result);
    let second=tools.call("profile_create",json!({"name":"Repeated name","operationId":uuid::Uuid::new_v4()}),client);assert_eq!(second["ok"],true,"{second}");
    let rows=manager.activity.request(&json!({"command":"activity_query","activityFilter":{"categories":["profiles"]}})).unwrap()["activity"].clone();
    assert_eq!(rows["total"],2,"{rows}");assert!(rows["events"].as_array().unwrap().iter().all(|row|row["source"]=="mcp"&&row["eventType"]=="profile_create"));
    assert_ne!(rows["events"][0]["entityId"],rows["events"][1]["entityId"]);manager.shutdown();
}

#[test]
fn provider_forget_and_its_bookkeeping_tombstone_are_one_event_but_local_forget_is_still_logged() {
    let manager=GatewayManager::mcp_test_manager();
    manager.record_external_change("controller_forget",Some("wheel"),Some("Wheel"),&Value::Null,&Value::Null,None,"cockpit");
    let mut value=json!({"components":[],"forgotten":{"wheel":{"signature":"known","sawAbsent":false,"type":"controller","provider":"controllers","activityRecorded":true}}});
    let request=|revision,value|manager.request(json!({"command":"ui_preferences_save","clientId":"activity-once-forget-regression","key":"remembered","expectedRevision":revision,"value":value}));
    assert_eq!(request(0,value.clone())["ok"],true);
    value["forgotten"]["local"]=json!({"signature":"known","sawAbsent":false,"type":"controller","provider":"controllers"});
    assert_eq!(request(1,value)["ok"],true);
    let rows=manager.activity.request(&json!({"command":"activity_query","activityFilter":{"categories":["devices"]}})).unwrap()["activity"].clone();
    assert_eq!(rows["total"],2,"{rows}");assert_eq!(rows["events"][0]["eventType"],"device_forgotten");assert_eq!(rows["events"][1]["eventType"],"controller_forget");manager.shutdown();
}
