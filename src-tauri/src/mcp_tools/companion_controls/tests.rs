use super::*;
#[derive(Default)]
struct Fixture {requests:Mutex<Vec<Value>>}
impl Backend for Fixture {
    fn request(&self,request:Value)->Value{self.requests.lock().unwrap().push(request.clone());json!({"ok":true,"state":{"companions":{"accepted":request,"usb":{"connected":false}}}})}
    fn guarded(&self,_:Value,_:Value)->Value{panic!("Companion tools use native lifecycle admission")}
}
fn fixture()->(Tools,Arc<Fixture>){let backend=Arc::new(Fixture::default());(Tools{backend:backend.clone(),ledger:Mutex::new(Default::default()),drafts:Mutex::new(vec![]),workspace_uploads:Mutex::new(vec![]),integration:OnceLock::new()},backend)}
fn arguments(schema:&Value)->Value {
    let mut args=json!({});
    for key in schema["required"].as_array().unwrap(){let key=key.as_str().unwrap();let property=&schema["properties"][key];args[key]=if key=="operationId"{json!(uuid::Uuid::new_v4().to_string())}
        else if key=="base64"{json!(STANDARD.encode([1,2,3]))}
        else if let Some(value)=property["enum"].as_array().and_then(|values|values.first()){value.clone()}
        else {match property["type"].as_str().unwrap(){"boolean"=>json!(true),"integer"=>property["minimum"].clone(),"array"=>json!([]),_=>json!("native-value")}};
    }args
}
#[test]
fn every_companion_tool_has_a_fixed_native_route_typed_fields_stable_client_and_replay(){
    let (tools,backend)=fixture();let catalogue=catalogue();let mut names=std::collections::BTreeSet::new();
    for tool in catalogue.iter().filter(|tool|handles(tool["name"].as_str().unwrap())) {
        let name=tool["name"].as_str().unwrap();assert!(names.insert(name));let args=arguments(&tool["inputSchema"]);let before=backend.requests.lock().unwrap().len();
        let result=tools.call(name,args.clone(),"operator");assert_eq!(result["ok"],true,"{name}: {result}");
        let request=backend.requests.lock().unwrap().last().unwrap().clone();assert_eq!(request["clientId"],native_client("operator"));assert!(request["companion"].get("operationId").is_none());assert!(request["companion"].get("command").is_none());
        assert_eq!(request["command"],match name{"companion_certificate_get"=>"companion_browser_certificate","companion_usb_status"=>"companion_inventory",_=>name});
        if name=="companion_publish_chunk"{assert_eq!(request["companion"]["bytes"],json!([1,2,3]));assert!(request["companion"].get("base64").is_none());}
        if mutation(name){assert_eq!(tools.call(name,args,"operator"),result);assert_eq!(backend.requests.lock().unwrap().len(),before+1);}else{assert_eq!(tool["annotations"]["readOnlyHint"],true);}
        let rejected=tools.call(name,json!({"command":"arbitrary","operationId":uuid::Uuid::new_v4().to_string()}),"operator");assert_eq!(rejected["error"]["code"],"invalid_arguments");
    }
    assert_eq!(names.len(),30);assert!(!handles("companion_feedback"));assert!(!mutation("arbitrary"));
}
#[test]
fn invalid_base64_empty_or_oversized_chunks_never_reach_native_and_tokens_are_required(){
    let (tools,backend)=fixture();for value in ["!".to_owned(),"".into(),STANDARD.encode(vec![0;8193])]{let result=tools.call("companion_publish_chunk",json!({"operationId":uuid::Uuid::new_v4().to_string(),"uploadId":"owned","offset":0,"base64":value}),"operator");assert_eq!(result["ok"],false);}
    assert!(tools.companion_tool("companion_inventory",&Value::Null,"operator").is_err());
    assert_eq!(tools.call("companion_revoke",json!({"operationId":uuid::Uuid::new_v4().to_string(),"companionId":"paired"}),"operator")["error"]["code"],"invalid_arguments");assert!(backend.requests.lock().unwrap().is_empty());
}
#[test]
fn selection_schema_rejects_unknown_fields_and_forwards_the_exact_observed_host_token(){
    let (tools,backend)=fixture();let mut args=json!({"operationId":uuid::Uuid::new_v4().to_string(),"uploadId":"upload","saveAsLatest":true,"selection":{"expectedHostToken":"observed:8","companionIds":["screen"]}});
    assert_eq!(tools.call("companion_publish_commit",args.clone(),"operator")["ok"],true);assert_eq!(backend.requests.lock().unwrap()[0]["companion"]["selection"],args["selection"]);
    args["operationId"]=json!(uuid::Uuid::new_v4().to_string());args["selection"]["sql"]=json!("unsafe");assert_eq!(tools.call("companion_publish_commit",args,"operator")["error"]["code"],"invalid_arguments");assert_eq!(backend.requests.lock().unwrap().len(),1);
}
#[test]
fn real_native_uploads_are_client_bound_and_validation_uses_the_portal_boundary(){
    let tools=Tools::new(Arc::new(GatewayManager::mcp_test_manager()));
    assert_eq!(tools.call("companion_usb_status",json!({}),"one")["data"]["usb"]["connected"],false);
    let mutate=|name:&str,mut args:Value,client:&str|{args["operationId"]=json!(uuid::Uuid::new_v4().to_string());tools.call(name,args,client)};
    let upload:Value=serde_json::from_str(include_str!("../../../../crates/companion-service/tests/fixtures/compiled-toggle-publication.json")).unwrap();let bytes=serde_json::to_vec(&upload).unwrap();
    let begun=mutate("companion_publish_begin",json!({"size":bytes.len()}),"one");assert_eq!(begun["ok"],true,"{begun}");let id=begun["data"]["uploadId"].clone();
    assert_eq!(mutate("companion_publish_chunk",json!({"uploadId":id,"offset":0,"base64":STANDARD.encode(&bytes[..8192.min(bytes.len())])}),"two")["ok"],false);
    for (index,chunk) in bytes.chunks(8192).enumerate(){let reply=mutate("companion_publish_chunk",json!({"uploadId":id,"offset":index*8192,"base64":STANDARD.encode(chunk)}),"one");assert_eq!(reply["ok"],true,"{reply}");}
    let saved=mutate("companion_publish_commit",json!({"uploadId":id,"saveAsLatest":true}),"one");assert_eq!(saved["ok"],true,"{saved}");assert_eq!(saved["data"]["selectionCommitted"],true);assert_eq!(saved["data"]["panelSaved"]["publicationHash"],upload["publicationHash"]);
    let token=saved["data"]["host"]["snapshotToken"].clone();let read=tools.call("companion_inventory",json!({}),"two");assert_eq!(read["data"]["host"]["snapshotToken"],token);
    let stale=mutate("companion_prune",json!({"expectedHostToken":"stale:0","hashes":[],"panelIds":[]}),"one");assert_eq!(stale["error"]["code"],"revision_conflict");
    let pruned=mutate("companion_prune",json!({"expectedHostToken":token,"hashes":[],"panelIds":[]}),"one");assert_eq!(pruned["ok"],true,"{pruned}");
}
