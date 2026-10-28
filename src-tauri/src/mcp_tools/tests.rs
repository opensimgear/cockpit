use super::*;
use std::collections::BTreeSet;

const CLIENT: &str = "authenticated-test-client";
#[test]
fn widget_tasks_use_native_validation_persistence_and_mutation_ledger(){
    let tools=Tools::new(Arc::new(GatewayManager::mcp_test_manager()));
    let guide=tools.read_resource("cockpit://guide/widgets",CLIENT);
    let mut bundle=guide["data"]["example"].clone();bundle["definitions"][0]["id"]=json!("mcp-native-widget");
    assert!(bundle.get("bundleVersion").is_none());assert!(bundle["definitions"][0].get("revision").is_none());
    let args=json!({"operationId":operation(),"expectedRevision":0,"bundle":bundle});let saved=tools.call("widget_save",args.clone(),CLIENT);assert_eq!(saved["ok"],true,"{saved}");assert_eq!(tools.call("widget_save",args,CLIENT),saved);
    assert_eq!(tools.call("widget_get",json!({"id":"mcp-native-widget"}),CLIENT)["data"]["definitions"],bundle["definitions"]);assert_eq!(tools.call("widgets_list",json!({}),CLIENT)["data"]["total"],2);
    let mut next=bundle.clone();next["definitions"][0]["title"]=json!("Updated capsule");
    let updated=tools.call("widget_save",json!({"operationId":operation(),"expectedRevision":1,"bundle":next}),CLIENT);assert_eq!(updated["ok"],true,"{updated}");assert_eq!(tools.call("widget_get",json!({"id":"mcp-native-widget"}),CLIENT)["data"]["definitions"],next["definitions"]);
    let mut invalid=next.clone();invalid["definitions"][0]["scene"][0]=json!({"id":"bad","kind":"script","code":"alert(1)"});assert_eq!(tools.call("widget_save",json!({"operationId":operation(),"expectedRevision":2,"bundle":invalid}),CLIENT)["ok"],false);assert_eq!(tools.call("widget_get",json!({"id":"mcp-native-widget"}),CLIENT)["data"]["definitions"],next["definitions"]);
    assert_eq!(tools.call("widget_save",json!({"bundle":bundle}),CLIENT)["error"]["code"],"invalid_arguments");

}
#[derive(Default)]
struct Fake {
    service: Mutex<Option<osg_profiles::service::Service>>,
    compilation: crate::profile_compilation::Runtime,
    calls: Mutex<Vec<Value>>,
    guarded_calls: Mutex<Vec<(Value, Value)>>,
}
impl Backend for Fake {
    fn request(&self, mut request: Value) -> Value {
        self.calls.lock().unwrap().push(request.clone());
        request.as_object_mut().unwrap().remove("clientId");
        let command = request["command"].as_str().unwrap().to_owned();
        if command == "profile_virtual_slots_get" || command == "profile_virtual_slot_set" {
            return json!({"ok":true,"state":{"profiles":{"profileId":request["id"],"profileRevision":4,"readOnly":false,"virtualSlots":[{"nodeId":"virtual-1","slot":request.get("slot").cloned().unwrap_or(Value::Null)}]}}});
        }
        if command == "profile_compile" || command == "profile_compilation_inspect" {
            return match self.compilation.request(&request, |_, assignments| {
                Ok(osg_profiles::compiler_models::CompileSnapshot {
                    inventory_session: "fixture".into(),
                    inventory_revision: "1".into(),
                    assignments: assignments.unwrap_or_default(),
                    resources: vec![],
                    game_context: None,
                    backend: Default::default(),
                    inventory_diagnostics: vec![],
                    read_only: false,
                })
            }) {
                Ok(data) => json!({"ok":true,"state":{"profileCompilation":data}}),
                Err(error) => json!({"ok":false,"error":error}),
            };
        }
        if command == "profile_resources_inspect" {
            let offset = request["offset"].as_u64().unwrap_or(0) as usize;
            let limit = request["limit"].as_u64().unwrap_or(32) as usize;
            let ports=(0..130).skip(offset).take(limit).map(|index|json!({"descriptor":{"id":format!("port-{index}"),"name":if index%2==0{"Heading"}else{"Speed"}},"contractHash":format!("contract-{index}")})).collect::<Vec<_>>();
            return json!({"ok":true,"state":{"profileResources":{"session":"fixture-session","revision":"fixture-revision","resource":{"id":request["resourceId"]},"ports":ports,"offset":offset,"total":130}}});
        }
        if command == "profile_resources_list" {
            let offset = request["offset"].as_u64().unwrap_or(0) as usize;
            let limit = request["limit"].as_u64().unwrap_or(32) as usize;
            let resources=(0..150).skip(offset).take(limit).map(|index|json!({"id":format!("resource-{index}"),"resourceType":if index%3==0{"companion"}else{"host-controller"}})).collect::<Vec<_>>();
            return json!({"ok":true,"state":{"profileResources":{"session":"fixture-session","revision":"fixture-revision","resources":resources,"offset":offset,"total":150}}});
        }
        if command == "config_read" {
            return json!({"ok":true,"state":{"moduleConfigs":{(request["moduleId"].as_str().unwrap()):runtime_snapshot()},"modules":[{"id":request["moduleId"],"boot":42}]}});
        }
        if command == "module_usb_config_read" {
            return json!({"ok":true,"state":{"moduleUsb":{"module":{"key":request["key"],"snapshot":{"route":request["route"],"config":runtime_snapshot()}}}}});
        }
        if command == "game_get_state" {
            let vehicles=(0..150).map(|index|json!({"id":format!("car-{index}"),"name":if index%2==0{format!("Sports car {index}")}else{format!("Touring car {index}")}})).collect::<Vec<_>>();
            return json!({"ok":true,"state":{"games":{"catalogue":[{"id":"assetto-corsa","name":"Assetto Corsa","installed":true,"executable":"native-installation","integrationSetup":{"providerSetupRevision":"setup-1","catalogueRevision":"catalogue-1"},"vehicleSupport":{"supported":true,"kind":"car","vehicles":vehicles,"message":"Observed vehicles"},"currentVehicle":{"id":"car-0","name":"Sports car 0"},"vehicleStatus":{"status":"ready"}}]}}});
        }
        if command == "profile_draft_save" {
            request["command"] = json!("profile_editor_save");
        }
        let mut service = self.service.lock().unwrap();
        let service = service.get_or_insert_with(|| osg_profiles::service::Service::new(None));
        match service.request(&request) {
            Ok(data) => json!({"ok":true,"state":{"profiles":data}}),
            Err(error) => json!({"ok":false,"error":error}),
        }
    }
    fn guarded(&self, request: Value, owner: Value) -> Value {
        self.guarded_calls.lock().unwrap().push((request, owner));
        json!({"ok":true,"state":{"guarded":true}})
    }
}
fn setup() -> (Tools, Arc<Fake>) {
    let backend = Arc::new(Fake::default());
    (
        Tools {
            backend: backend.clone(),
            ledger: Mutex::new(Default::default()),
            drafts: Mutex::new(vec![]),
            workspace_uploads: Mutex::new(vec![]),
            integration:OnceLock::new(),
        },
        backend,
    )
}
fn operation() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn runtime_config() -> Value {
    json!({"name":"Before","expanderMask":0,"buttons":[[0,0,0]],"axes":[],"hats":[],"encoders":[],"selectors":[]})
}
fn runtime_snapshot() -> Value {
    json!({"generation":1,"savedGeneration":1,"owner":0,"dirty":false,"pendingApply":false,"valid":true,"view":"active","config":runtime_config()})
}
fn create(tools: &Tools, name: &str) -> Value {
    let result = tools.call(
        "profile_create",
        json!({"name":name,"operationId":operation()}),
        CLIENT,
    );
    assert_eq!(result["ok"], true, "{result}");
    result["data"]["document"].clone()
}

#[test]
fn catalogue_is_explicit_strict_unique_and_annotated_consistently() {
    let definitions = catalogue();
    let names = definitions
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(names.len(), definitions.len());
    assert!(names.len() > 35);
    for tool in &definitions {
        assert_eq!(tool["inputSchema"]["additionalProperties"], false, "{tool}");
        assert_eq!(
            tool["annotations"]["readOnlyHint"],
            !is_mutation(tool["name"].as_str().unwrap())
        );
        if is_mutation(tool["name"].as_str().unwrap()) {
            assert!(tool["inputSchema"]["required"]
                .as_array()
                .unwrap()
                .contains(&json!("operationId")));
        }
    }
    for forbidden in [
        "invoke",
        "gateway_request",
        "shell",
        "file_read",
        "firmware_flash",
        "action_invoke",
        "game_launch",
    ] {
        assert!(!names.contains(forbidden));
    }
    let (tools, fake) = setup();
    let result = tools.call(
        "profile_get",
        json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"clientId":"forged"}),
        CLIENT,
    );
    assert_eq!(result["error"]["code"], "invalid_arguments");
    assert!(fake.calls.lock().unwrap().is_empty());
    assert_eq!(
        tools.call(
            "profile_create",
            json!({"name":"Test","operationId":"not-a-uuid"}),
            CLIENT
        )["ok"],
        false
    );
    assert_eq!(tools.call("profile_apply",json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1,"operationId":operation()}),CLIENT)["error"]["code"],"invalid_arguments");
}

#[test]
fn virtual_slot_tools_forward_local_profile_requests_with_strict_arguments() {
    let (tools, fake) = setup();
    let id = uuid::Uuid::new_v4().to_string();
    let read = tools.call("profile_virtual_slots_get", json!({"id":id}), CLIENT);
    assert_eq!(read["ok"], true, "{read}");
    assert_eq!(read["data"]["profileId"], id);
    assert_eq!(read["data"]["virtualSlots"][0]["nodeId"], "virtual-1");
    assert_eq!(fake.calls.lock().unwrap()[0], json!({"command":"profile_virtual_slots_get","id":id,"clientId":native_client(CLIENT)}));

    let operation_id = operation();
    let set_args = json!({"id":id,"expectedRevision":4,"nodeId":"virtual-1","slot":3,"operationId":operation_id});
    let saved = tools.call("profile_virtual_slot_set", set_args.clone(), CLIENT);
    assert_eq!(saved["ok"], true, "{saved}");
    assert_eq!(saved["data"]["virtualSlots"][0]["slot"], 3);
    assert_eq!(tools.call("profile_virtual_slot_set", set_args, CLIENT), saved);
    assert_eq!(fake.calls.lock().unwrap()[1], json!({"command":"profile_virtual_slot_set","id":id,"expectedRevision":4,"nodeId":"virtual-1","slot":3,"clientId":native_client(CLIENT)}));

    for invalid in [
        json!({"id":id,"expectedRevision":4,"nodeId":"virtual-1","slot":17,"operationId":operation()}),
        json!({"id":id,"expectedRevision":4,"nodeId":"virtual-1","slot":1.5,"operationId":operation()}),
        json!({"id":id,"expectedRevision":4,"nodeId":"bad/id","slot":1,"operationId":operation()}),
        json!({"id":"not-a-uuid","expectedRevision":4,"nodeId":"virtual-1","slot":null,"operationId":operation()}),
    ] {
        assert_eq!(tools.call("profile_virtual_slot_set", invalid, CLIENT)["error"]["code"], "invalid_arguments");
    }
    let null_slot = tools.call("profile_virtual_slot_set", json!({"id":id,"expectedRevision":4,"nodeId":"virtual-1","slot":null,"operationId":operation()}), CLIENT);
    assert_eq!(null_slot["ok"], true, "{null_slot}");
}

#[test]
fn reconnect_replays_create_once_and_rejects_reusing_identity_for_different_work() {
    let (tools, fake) = setup();
    let id = operation();
    let args = json!({"name":"Flight","operationId":id});
    let first = tools.call("profile_create", args.clone(), CLIENT);
    let second = tools.call("profile_create", args, CLIENT);
    assert_eq!(first, second);
    assert_eq!(first["ok"], true);
    assert_eq!(
        fake.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call["command"] == "profile_create")
            .count(),
        1
    );
    let conflict = tools.call(
        "profile_create",
        json!({"name":"Different","operationId":id}),
        CLIENT,
    );
    assert_eq!(conflict["error"]["code"], "operation_conflict");
    let status = tools.call("operation_status", json!({"operationId":id}), CLIENT);
    assert_eq!(status["data"]["state"], "completed");
    assert_eq!(status["data"]["result"], first);
    assert_eq!(
        tools.call(
            "operation_status",
            json!({"operationId":id}),
            "another-client"
        )["error"]["code"],
        "operation_unknown"
    );
}

#[test]
fn pending_duplicates_and_full_ledger_fail_closed_without_eviction() {
    let mut ledger = Ledger::default();
    let first = operation();
    assert!(matches!(
        ledger.begin(CLIENT, &first, "signature".into()).unwrap(),
        Admission::Fresh
    ));
    let Admission::Reply(pending) = ledger.begin(CLIENT, &first, "signature".into()).unwrap()
    else {
        panic!("must not execute pending work twice")
    };
    assert_eq!(pending["data"]["state"], "pending");
    for _ in 1..MAX_OPERATIONS {
        assert!(matches!(
            ledger
                .begin(CLIENT, &operation(), "signature".into())
                .unwrap(),
            Admission::Fresh
        ));
    }
    assert_eq!(
        ledger
            .begin(CLIENT, &operation(), "signature".into())
            .err()
            .unwrap()["error"]["code"],
        "capacity"
    );
    assert!(matches!(
        ledger.begin(CLIENT, &first, "signature".into()).unwrap(),
        Admission::Reply(_)
    ));
    let result = json!({"ok":true,"data":{"document":{"id":"profile","revision":2,"graph":"x".repeat(MAX_OPERATION_REPLY)}}});
    ledger.finish(CLIENT, &first, &result);
    let retained = ledger.status(CLIENT, &first);
    assert_eq!(
        retained["data"]["result"]["data"]["document"]["id"],
        "profile"
    );
    assert!(retained.to_string().len() < 2048);
}

#[test]
fn prepared_new_graph_compiles_without_writes_and_saves_one_atomic_transaction() {
    let (tools, fake) = setup();
    let prepared=tools.call("profile_prepare_changes",json!({"name":"Prepared flight","expectedRevision":null,"edits":[{"kind":"set_eligibility","eligibility":{"gameIds":["dcs"]}}]}),CLIENT);
    assert_eq!(prepared["ok"], true, "{prepared}");
    assert_eq!(
        prepared["data"]["effects"],
        json!({"saved":false,"selected":false,"applied":false})
    );
    assert_eq!(prepared["data"]["validation"]["valid"], true);
    assert!(prepared["data"]["compilation"]["compilationId"].is_string());
    assert_eq!(fake.calls.lock().unwrap().len(), 1);
    assert_eq!(fake.calls.lock().unwrap()[0]["command"], "profile_compile");
    let before = tools.call("profiles_list", json!({}), CLIENT);
    assert_eq!(before["data"]["total"], 1);
    let args = json!({"draftId":prepared["data"]["draftId"],"operationId":operation()});
    let saved = tools.call("profile_save_prepared", args.clone(), CLIENT);
    assert_eq!(saved["ok"], true, "{saved}");
    assert_eq!(saved["data"]["document"], prepared["data"]["document"]);
    assert_eq!(tools.call("profile_save_prepared", args, CLIENT), saved);
    let calls = fake.calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|call| call["command"] == "profile_draft_save")
            .count(),
        1
    );
    assert!(!calls
        .iter()
        .any(|call| call["command"] == "profile_apply" || call["command"] == "profile_select"));
}

#[test]
fn concurrent_ui_save_invalidates_prepared_revision_without_partial_persistence() {
    let (tools, fake) = setup();
    let doc = create(&tools, "Before");
    let prepared=tools.call("profile_prepare_changes",json!({"profileId":doc["id"],"expectedRevision":1,"edits":[{"kind":"set_name","name":"MCP draft"}]}),CLIENT);
    assert_eq!(prepared["ok"], true, "{prepared}");
    assert_eq!(
        prepared["data"]["diff"]["changes"][0]["path"],
        "/document/name"
    );
    let changed=fake.request(json!({"command":"profile_rename","id":doc["id"],"name":"UI saved revision","expectedRevision":1}));
    assert_eq!(changed["ok"], true);
    let saved = tools.call(
        "profile_save_prepared",
        json!({"draftId":prepared["data"]["draftId"],"operationId":operation()}),
        CLIENT,
    );
    assert_eq!(saved["error"]["code"], "revision_conflict", "{saved}");
    let current = tools.call("profile_get", json!({"id":doc["id"]}), CLIENT);
    assert_eq!(current["data"]["document"]["name"], "UI saved revision");
    assert_eq!(current["data"]["document"]["revision"], 2);
}

#[test]
fn drafts_are_client_bound_expire_and_validate_nested_fields() {
    let (tools, fake) = setup();
    let doc = create(&tools, "Draft");
    let mut invalid = doc.clone();
    invalid["runtimeOwner"] = json!({});
    let rejected = tools.call(
        "profile_prepare_changes",
        json!({"document":invalid,"expectedRevision":1}),
        CLIENT,
    );
    assert_eq!(rejected["error"]["code"], "invalid_arguments");
    let rejected=tools.call("profile_prepare_changes",json!({"profileId":doc["id"],"expectedRevision":1,"edits":[{"kind":"set_name","name":"Hi","claim":true}]}),CLIENT);
    assert_eq!(rejected["error"]["code"], "invalid_arguments");
    let prepared = tools.call(
        "profile_prepare_changes",
        json!({"profileId":doc["id"],"expectedRevision":1}),
        CLIENT,
    );
    assert_eq!(prepared["ok"], true, "{prepared}");
    let draft = prepared["data"]["draftId"].clone();
    assert_eq!(
        tools.call(
            "profile_save_prepared",
            json!({"draftId":draft,"operationId":operation()}),
            "another-client"
        )["error"]["code"],
        "draft_expired"
    );
    // Windows monotonic time may start at boot; subtracting thirty minutes on
    // a fresh CI runner underflows. Observe a future time instead of backdating.
    let expired_at = tools.drafts.lock().unwrap()[0].created
        + DRAFT_LIFETIME + Duration::from_secs(1);
    assert_eq!(
        tools.save_prepared_at(&json!({"draftId":draft}), CLIENT, expired_at)
            .unwrap_err()["error"]["code"],
        "draft_expired"
    );
    assert!(!fake
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|call| call["command"] == "profile_draft_save"));
}

#[test]
fn search_and_companion_filter_have_correct_bounded_pagination() {
    let (tools, _) = setup();
    let search = tools.call(
        "resource_search_ports",
        json!({"resourceId":"fixture","search":"heading","offset":63,"limit":1}),
        CLIENT,
    );
    assert_eq!(search["ok"], true, "{search}");
    assert_eq!(search["data"]["total"], 65);
    assert_eq!(search["data"]["ports"].as_array().unwrap().len(), 1);
    assert_eq!(search["data"]["ports"][0]["descriptor"]["id"], "port-126");
    assert_eq!(search["data"]["nextOffset"], 64);
    let companion = tools.call("companions_list", json!({"offset":48,"limit":1}), CLIENT);
    assert_eq!(companion["data"]["total"], 50);
    assert_eq!(companion["data"]["companions"][0]["id"], "resource-144");
    assert_eq!(companion["data"]["nextOffset"], 49);
    let page = tools.call("resources_list", json!({"offset":149,"limit":1}), CLIENT);
    assert_eq!(page["data"]["nextOffset"], Value::Null);
    for args in [
        json!({"limit":0}),
        json!({"limit":65}),
        json!({"offset":-1}),
        json!({"limit":1.5}),
    ] {
        assert_eq!(
            tools.call("resources_list", args, CLIENT)["error"]["code"],
            "invalid_arguments"
        );
    }
}

#[test]
fn device_controls_use_shared_native_admission_and_never_forward_caller_commands() {
    let (tools, fake) = setup();
    let args = json!({"gatewayId":"stm32:0123456789abcdef01234567","moduleId":"module","action":"claim","expectedActiveOwner":null,"operationId":operation()});
    let result = tools.call("gateway_config_change", args, CLIENT);
    assert_eq!(result["ok"], true, "{result}");
    let calls = fake.guarded_calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0["command"], "claim");
    assert_eq!(calls[0].1, Value::Null);
    assert!(calls[0].0.get("expectedActiveOwner").is_none());
    assert!(calls[0].0.get("operationId").is_none());
    assert!(crate::protocol::identifier(
        calls[0].0["clientId"].as_str().unwrap(),
        16,
        80
    ));
    assert_eq!(tools.call("gateway_config_change",json!({"gatewayId":"gateway","moduleId":"module","action":"action_invoke","expectedActiveOwner":null,"operationId":operation()}),CLIENT)["error"]["code"],"invalid_arguments");
    assert_eq!(tools.call("profile_apply",json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1,"expectedActiveOwner":{"owner":"forged"},"operationId":operation()}),CLIENT)["error"]["code"],"invalid_arguments");
}

#[test]
fn context_resources_are_bounded_and_do_not_resolve_arbitrary_paths() {
    let (tools, _) = setup();
    assert_eq!(resources().len(), 6);
    for resource in resources() {
        let result = tools.read_resource(resource["uri"].as_str().unwrap(), CLIENT);
        assert_eq!(result["ok"], true, "{result}");
        assert!(result.to_string().len() < 65_536);
    }
    assert_eq!(
        tools.read_resource("file:///C:/Windows/win.ini", CLIENT)["error"]["code"],
        "resource_not_found"
    );
    assert_eq!(
        tools.call("shell", json!({}), CLIENT)["error"]["code"],
        "unknown_tool"
    );
}

#[test]
fn processing_operations_are_discoverable_searchable_and_paginated_without_effects() {
    let (tools, fake) = setup();
    let first = tools.call("processing_operations_list", json!({"limit":2}), CLIENT);
    assert_eq!(first["ok"], true, "{first}");
    assert!(first["data"]["total"].as_u64().unwrap() > 20);
    assert_eq!(first["data"]["operations"].as_array().unwrap().len(), 2);
    assert_eq!(first["data"]["nextOffset"], 2);
    let next = tools.call(
        "processing_operations_list",
        json!({"offset":2,"limit":2}),
        CLIENT,
    );
    assert_eq!(next["data"]["schemaHash"], first["data"]["schemaHash"]);
    assert_ne!(
        next["data"]["operations"][0]["kind"],
        first["data"]["operations"][0]["kind"]
    );
    let scale = tools.call(
        "processing_operations_list",
        json!({"search":"scale"}),
        CLIENT,
    );
    assert!(scale["data"]["operations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["kind"] == "scale"));
    let actions = tools.call(
        "processing_operations_list",
        json!({"category":"action"}),
        CLIENT,
    );
    assert_eq!(actions["data"]["total"], 2);
    for item in actions["data"]["operations"].as_array().unwrap() {
        let schema = &item["parametersSchema"];
        assert!(!schema.to_string().contains("$ref"));
        let example = schema_example(schema, schema);
        serde_json::from_value::<osg_profiles::Operation>(example).unwrap();
        assert!(item["schemaHash"].as_str().unwrap().len() == 64);
    }
    assert!(fake.calls.lock().unwrap().is_empty());
    assert!(fake.guarded_calls.lock().unwrap().is_empty());
}

#[test]
fn device_configuration_prepare_is_strict_reviewable_and_never_claims_or_applies() {
    let (tools, fake) = setup();
    let mut candidate = runtime_config();
    candidate["name"] = json!("Reviewed candidate");
    let result=tools.call("device_config_prepare",json!({"transport":"gateway","gatewayId":"exact-gateway","moduleId":"module","config":candidate}),CLIENT);
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["data"]["validation"]["valid"], true);
    assert_eq!(result["data"]["diff"]["changes"][0]["path"], "/name");
    assert_eq!(result["data"]["target"]["boot"], 42);
    assert_eq!(
        result["data"]["effects"],
        json!({"claimed":false,"applied":false,"saved":false})
    );
    let result = tools.call(
        "device_config_prepare",
        json!({"transport":"usb","key":"exact-usb","route":{"boot":42},"config":candidate}),
        CLIENT,
    );
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["data"]["target"]["route"]["boot"], 42);
    assert!(fake.guarded_calls.lock().unwrap().is_empty());
    assert_eq!(fake.calls.lock().unwrap().len(), 2);
    for invalid in [
        {
            let mut value = candidate.clone();
            value["hiddenClaim"] = json!(true);
            value
        },
        {
            let mut value = candidate.clone();
            value["buttons"] = json!([[152, 0, 0]]);
            value
        },
        {
            let mut value = candidate.clone();
            value["buttons"] = json!([[0, 0]]);
            value
        },
        {
            let mut value = candidate.clone();
            value["axes"] = json!([[0, 48, 4095, 0, 0, 0, 0]]);
            value
        },
    ] {
        assert_eq!(tools.call("device_config_prepare",json!({"transport":"gateway","gatewayId":"gateway","moduleId":"module","config":invalid}),CLIENT)["ok"],false);
    }
    assert_eq!(
        fake.calls.lock().unwrap().len(),
        2,
        "invalid candidates must be rejected before any native read"
    );
}

#[test]
fn typed_authoring_builds_a_complete_transformed_controller_graph_and_persists_it() {
    let (tools, fake) = setup();
    let examples = schema::examples();
    let prepared=tools.call("profile_prepare_changes",json!({"name":"Transformed controls","expectedRevision":null,"edits":[
        {"kind":"upsert_resource","resource":examples["resource"]},
        {"kind":"upsert_processing_node","node":examples["processingNode"]},
        {"kind":"replace_virtual_outputs","outputs":[examples["virtualOutput"]]},
        {"kind":"upsert_binding","binding":examples["binding"]},
        {"kind":"upsert_binding","binding":{"id":"not-to-controller","from":{"nodeId":"invert-button","portId":"out"},"to":{"nodeId":"virtual-pad","portId":"button-1"}}}
    ]}),CLIENT);
    assert_eq!(prepared["ok"], true, "{prepared}");
    assert_eq!(prepared["data"]["validation"]["valid"], true, "{prepared}");
    assert_eq!(
        prepared["data"]["document"]["bindings"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_ne!(prepared["data"]["compilation"]["readiness"], Value::Null);
    let saved = tools.call(
        "profile_save_prepared",
        json!({"draftId":prepared["data"]["draftId"],"operationId":operation()}),
        CLIENT,
    );
    assert_eq!(saved["ok"], true, "{saved}");
    assert_eq!(
        saved["data"]["document"]["processingNodes"][0]["operation"]["kind"],
        "not"
    );
    assert!(!fake
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|call| call["command"] == "profile_apply"));
}

fn schema_example(root: &Value, definition: &Value) -> Value {
    if let Some(name) = definition["$ref"]
        .as_str()
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
    {
        return schema_example(root, &root["$defs"][name]);
    }
    if let Some(value) = definition.get("const") {
        return value.clone();
    }
    if let Some(variants) = definition["oneOf"]
        .as_array()
        .or_else(|| definition["anyOf"].as_array())
    {
        return schema_example(root, &variants[0]);
    }
    if let Some(values) = definition["enum"].as_array() {
        return values[0].clone();
    }
    let kind = definition["type"].as_str().or_else(|| {
        definition["type"]
            .as_array()
            .and_then(|types| types.first())
            .and_then(Value::as_str)
    });
    match kind {
        Some("object") => Value::Object(
            definition["required"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|key| {
                    (
                        key.into(),
                        schema_example(root, &definition["properties"][key]),
                    )
                })
                .collect(),
        ),
        Some("array") => json!([]),
        Some("string") => json!("sample"),
        Some("number") | Some("integer") => json!(1),
        Some("boolean") => json!(false),
        _ => Value::Null,
    }
}

#[test]
fn exported_native_variant_contracts_deserialize_into_the_real_domain_types() {
    let root = schema::document();
    for definition in root["$defs"]["Operation"]["oneOf"].as_array().unwrap() {
        let value = schema_example(&root, definition);
        serde_json::from_value::<osg_profiles::Operation>(value.clone())
            .unwrap_or_else(|error| panic!("Operation schema drift: {error}: {value}"));
    }
    for definition in root["$defs"]["Rule"]["oneOf"].as_array().unwrap() {
        let value = schema_example(&root, definition);
        serde_json::from_value::<osg_mapping::rules::Rule>(value.clone())
            .unwrap_or_else(|error| panic!("Rule schema drift: {error}: {value}"));
    }
    for definition in root["$defs"]["PortContract"]["oneOf"].as_array().unwrap() {
        let value = schema_example(&root, definition);
        serde_json::from_value::<osg_profiles::PortContract>(value.clone())
            .unwrap_or_else(|error| panic!("Port schema drift: {error}: {value}"));
    }
    for definition in root["$defs"]["ScalarSchema"]["oneOf"].as_array().unwrap() {
        let value = schema_example(&root, definition);
        serde_json::from_value::<osg_mapping::signal::ScalarSchema>(value.clone())
            .unwrap_or_else(|error| panic!("Scalar schema drift: {error}: {value}"));
    }
    for definition in root["$defs"]["DesktopAction"]["oneOf"].as_array().unwrap() {
        let value = schema_example(&root, definition);
        serde_json::from_value::<osg_mapping::desktop::Action>(value.clone())
            .unwrap_or_else(|error| panic!("Desktop action schema drift: {error}: {value}"));
    }
    let profile = serde_json::to_value(osg_profiles::default_document()).unwrap();
    definitions::validate(&root, &profile, "profile").unwrap();
    let mut profile = profile;
    profile["processingNodes"] = json!([{"id":"script","name":"Multiline script","operation":{"kind":"script","source":"return {\n out = input.in\n}","parameters":{}},"inputs":[],"outputs":[]}]);
    definitions::validate(&root, &profile, "profile").unwrap();
    profile["processingNodes"][0]["operation"]["claim"] = json!(true);
    assert!(definitions::validate(&root, &profile, "profile").is_err());
}

#[test]
fn game_vehicle_catalogues_have_search_pages_setup_stamps_and_compact_game_summaries() {
    let (tools, fake) = setup();
    let games = tools.call("games_list", json!({}), CLIENT);
    assert_eq!(games["ok"], true, "{games}");
    assert_eq!(
        games["data"]["catalogue"][0]["vehicleSupport"]["vehicleCount"],
        150
    );
    assert!(games["data"]["catalogue"][0]["vehicleSupport"]
        .get("vehicles")
        .is_none());
    let first = tools.call(
        "game_vehicles_list",
        json!({"gameId":"assetto-corsa","search":"sports","offset":72,"limit":2}),
        CLIENT,
    );
    assert_eq!(first["ok"], true, "{first}");
    assert_eq!(first["data"]["total"], 75);
    assert_eq!(first["data"]["vehicles"].as_array().unwrap().len(), 2);
    assert_eq!(first["data"]["vehicles"][0]["id"], "car-144");
    assert_eq!(first["data"]["nextOffset"], 74);
    assert_eq!(first["data"]["setup"]["providerSetupRevision"], "setup-1");
    assert_eq!(first["data"]["currentVehicle"]["id"], "car-0");
    let last = tools.call(
        "game_vehicles_list",
        json!({"gameId":"assetto-corsa","search":"sports","offset":74,"limit":2}),
        CLIENT,
    );
    assert_eq!(
        last["data"]["catalogueStamp"],
        first["data"]["catalogueStamp"]
    );
    assert_eq!(last["data"]["nextOffset"], Value::Null);
    assert_eq!(
        tools.call("game_vehicles_list", json!({"gameId":"unknown"}), CLIENT)["error"]["code"],
        "not_found"
    );
    assert!(fake.guarded_calls.lock().unwrap().is_empty());
    assert!(fake
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|call| call["command"] == "game_get_state"));
}

#[test]
fn storage_tools_require_mutation_identity_and_never_accept_paths_or_sql(){
 let tools=Tools::new(Arc::new(GatewayManager::mcp_test_manager()));
 assert!(!is_mutation("storage_status"));
 for name in ["storage_create","storage_remove","storage_restore","storage_restart"]{assert!(is_mutation(name));let result=tools.call(name,json!({}),CLIENT);assert_eq!(result["error"]["code"],"invalid_arguments","{name}: {result}");}
 for name in ["storage_status","storage_create","storage_remove","storage_restore","storage_restart"]{let tool=catalogue().into_iter().find(|tool|tool["name"]==name).unwrap();assert_eq!(tool["inputSchema"]["additionalProperties"],false);assert_eq!(tool["annotations"]["readOnlyHint"],name=="storage_status");}
 assert_eq!(tools.call("storage_restore",json!({"operationId":operation(),"id":uuid::Uuid::new_v4().to_string(),"expectedRevision":0,"confirmed":true,"path":"external.sqlite3"}),CLIENT)["error"]["code"],"invalid_arguments");
 assert_eq!(tools.call("storage_status",json!({}),CLIENT)["ok"],false);
}
