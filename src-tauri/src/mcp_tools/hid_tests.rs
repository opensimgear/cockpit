use super::*;

#[derive(Default)]
struct HidBackend(Mutex<Vec<Value>>);
impl Backend for HidBackend {
    fn request(&self, request: Value) -> Value {
        self.0.lock().unwrap().push(request.clone());
        json!({"ok":true,"state":{"controllers":{"interfaceId":request["interfaceId"],
            "generation":request["generation"],"contractFingerprint":"native-contract",
            "result":{"status":"known","values":[{"raw":42}],"diagnostics":["native warning"]}}}})
    }
    fn guarded(&self, _: Value, _: Value) -> Value { panic!("HID receives must not acquire profile outputs") }
}

#[test]
fn hid_tasks_are_explicit_bounded_and_forward_the_same_native_projection() {
    let backend=Arc::new(HidBackend::default());
    let tools=Tools {backend:backend.clone(),ledger:Mutex::new(Ledger::default()),drafts:Mutex::new(vec![]),workspace_uploads:Mutex::new(vec![]),integration:OnceLock::new()};
    for name in ["controller_hid_list","controller_hid_catalog","controller_hid_inspect","controller_hid_snapshot","controller_hid_open","controller_hid_state","controller_hid_reports","controller_hid_get","controller_hid_close"] {
        assert!(catalogue().iter().any(|tool|tool["name"]==name),"{name}");
        assert_eq!(is_mutation(name),matches!(name,"controller_hid_open"|"controller_hid_get"|"controller_hid_close"));
    }
    let args=json!({"interfaceId":"selected","generation":"native-generation"});
    let state=tools.call("controller_hid_state",args.clone(),"hid-reader");
    assert_eq!(state["ok"],true,"{state}");assert_eq!(state["data"]["result"]["values"][0]["raw"],42);
    assert_eq!(state["data"]["result"]["diagnostics"],json!(["native warning"]));
    assert_eq!(backend.0.lock().unwrap()[0],json!({"command":"controller_hid_state","clientId":native_client("hid-reader"),"interfaceId":"selected","generation":"native-generation"}));
    let open=json!({"operationId":"00000000-0000-4000-8000-000000000001","interfaceId":"selected","contractFingerprint":"observed-contract"});
    let first=tools.call("controller_hid_open",open.clone(),"hid-reader");
    assert_eq!(first["ok"],true);assert_eq!(tools.call("controller_hid_open",open,"hid-reader"),first);
    assert_eq!(backend.0.lock().unwrap().len(),2);
    for (name,invalid) in [
        ("controller_hid_open",json!({"interfaceId":"selected"})),
        ("controller_hid_reports",json!({"interfaceId":"selected","generation":"native-generation","after":1})),
        ("controller_hid_reports",json!({"interfaceId":"selected","generation":"native-generation","after":"01"})),
        ("controller_hid_reports",json!({"interfaceId":"selected","generation":"native-generation","after":"18446744073709551616"})),
        ("controller_hid_state",json!({"interfaceId":"selected","generation":"native-generation","path":"arbitrary"})),
        ("controller_hid_get",json!({"operationId":"00000000-0000-4000-8000-000000000002","interfaceId":"selected","generation":"native-generation","requestId":"request","kind":"output","reportId":0,"maxBytes":65537,"deadlineMs":2001}))
    ] {assert_eq!(tools.call(name,invalid,"hid-reader")["error"]["code"],"invalid_arguments","{name}");}
    assert_eq!(backend.0.lock().unwrap().len(),2);
}
#[test]
fn recording_and_offline_replay_use_bounded_authenticated_native_tasks() {
    let backend = Arc::new(HidBackend::default());
    let tools = Tools {
        backend: backend.clone(), ledger: Mutex::new(Ledger::default()),
        drafts: Mutex::new(vec![]), workspace_uploads: Mutex::new(vec![]), integration: OnceLock::new(),
    };
    let identity = json!({"interfaceId":"rudder","generation":"first"});
    for (index, (name, args, write)) in [
        ("controller_hid_record_list", json!({}), false),
        ("controller_hid_record_start", identity.clone(), true),
        ("controller_hid_record_status", identity.clone(), false),
        ("controller_hid_record_stop", identity.clone(), true),
        ("controller_hid_record_export", json!({"interfaceId":"rudder","generation":"first","offset":0,"limit":32768}), false),
        ("controller_hid_record_discard", identity, true),
        ("controller_hid_replay_upload_start", json!({"totalBytes":1024}), true),
        ("controller_hid_replay_upload_chunk", json!({"uploadId":"upload","offset":0,"chunk":"e30="}), true),
        ("controller_hid_replay_upload_finish", json!({"uploadId":"upload"}), true),
        ("controller_hid_replay_upload_cancel", json!({"uploadId":"upload"}), true),
        ("controller_hid_replay_read", json!({"replayId":"offline","after":"0","elapsedUs":"3000"}), false),
        ("controller_hid_replay_close", json!({"replayId":"offline"}), true),
    ].into_iter().enumerate() {
        assert_eq!(is_mutation(name), write, "{name}");
        let mut expected = args.clone();
        expected["clientId"] = json!(native_client("inspector"));
        expected["command"] = json!(name);
        let mut call = args;
        if write && !definitions::is_streaming(name) {
            call["operationId"] = json!(format!("00000000-0000-4000-8000-{index:012}"));
        }
        let reply = tools.call(name, call, "inspector");
        assert_eq!(reply["ok"], true, "{name}: {reply}");
        assert_eq!(backend.0.lock().unwrap().last(), Some(&expected));
    }
    let invalid = json!({"interfaceId":"rudder","generation":"first","offset":0,"limit":67_108_864});
    assert_eq!(tools.call("controller_hid_record_export",invalid,"inspector")["error"]["code"],"invalid_arguments");
}
