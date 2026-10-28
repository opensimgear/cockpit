use super::*;

fn debug_document() -> osg_profiles::ProfileDocument {
    use osg_profiles::{
        Binding, Direction, Endpoint, Operation, Port, PortContract, PortValue, ProcessingNode,
        Resource, ResourceKind,
    };
    let mut document = osg_profiles::default_document();
    let contract = PortContract::State {
        schema: osg_mapping::signal::ScalarSchema::Integer {
            min: -32768,
            max: 32767,
            unit: None,
        },
        freshness_ms: 1000,
        fallback: Some(PortValue::Number(0.0)),
    };
    let port = |id: &str, direction| Port {
        id: id.into(),
        name: id.into(),
        direction,
        contract: contract.clone(),
    };
    document.resources.push(Resource {
        id: "debug".into(),
        name: "Debug".into(),
        kind: ResourceKind::DebugController,
        required: false,
        ports: vec![
            port("axis-1", Direction::Output),
            port("axis-2", Direction::Output),
        ],
    });
    document.processing_nodes.push(ProcessingNode {
        id: "identity".into(),
        name: "Identity".into(),
        operation: Operation::Identity {},
        inputs: vec![port("in", Direction::Input)],
        outputs: vec![port("out", Direction::Output)],
        draft: None,
    });
    document.bindings.push(Binding {
        id: "wire".into(),
        from: Endpoint {
            node_id: "debug".into(),
            port_id: "axis-1".into(),
        },
        to: Endpoint {
            node_id: "identity".into(),
            port_id: "in".into(),
        },
    });
    let mut second = document.resources[0].clone();
    second.id = "debug2".into();
    document.resources.push(second);
    let mut identity = document.processing_nodes[0].clone();
    identity.id = "identity2".into();
    document.processing_nodes.push(identity);
    let mut wire = document.bindings[0].clone();
    wire.id = "wire2".into();
    wire.from.node_id = "debug2".into();
    wire.to.node_id = "identity2".into();
    document.bindings.push(wire);
    document
}
#[test]
fn native_debug_gateway_mcp_updates_real_mapping_and_retires_owner() {
    let manager = Arc::new(GatewayManager::mcp_test_manager());
    let request = |mut value: Value| {
        value["clientId"] = json!("debug-gateway-regression");
        manager.request(value)
    };
    let document = debug_document();
    let saved = request(
        json!({"command":"profile_editor_save","document":document,"expectedRevision":1,"assignments":[]}),
    );
    assert_eq!(saved["ok"], true, "{saved}");
    let apply = || {
        let response =
            request(json!({"command":"profile_apply","id":document.id,"expectedRevision":2}));
        assert_eq!(response["ok"], true, "{response}");
        response["state"]["profiles"]["profileSession"]["owner"].clone()
    };
    let owner = apply();
    let tools = crate::mcp_tools::Tools::new(manager.clone());
    let status = tools.call(
        "profile_debug_controller_status",
        json!({"owner":owner,"nodeId":"debug"}),
        "debug-regression",
    );
    assert_eq!(status["ok"], true, "{status}");
    assert_eq!(
        status["data"]["values"],
        json!([{"portId":"axis-1","value":0.0},{"portId":"axis-2","value":0.0}])
    );
    assert!(crate::mcp_tools::is_mutation(
        "profile_debug_controller_input"
    ));
    assert!(!crate::mcp_tools::is_mutation(
        "profile_debug_controller_status"
    ));
    let input = |sequence: u64| json!({"owner":owner,"nodeId":"debug","streamId":"22222222-2222-4222-8222-222222222222","sequence":sequence.to_string(),"changes":[{"portId":"axis-1","value":sequence},{"portId":"axis-2","value":-(sequence as i64)}]});
    let activity_before = manager
        .activity
        .request(&json!({"command":"activity_query","activityFilter":{}}))
        .unwrap()["activity"]["total"]
        .clone();
    for sequence in 1..=160 {
        let response = tools.call(
            "profile_debug_controller_input",
            input(sequence),
            "debug-regression",
        );
        assert_eq!(response["ok"], true, "{response}");
        manager.profile_session.lock().unwrap().poll().unwrap();
    }
    let activity_after = manager
        .activity
        .request(&json!({"command":"activity_query","activityFilter":{}}))
        .unwrap()["activity"]["total"]
        .clone();
    assert_eq!(
        activity_after, activity_before,
        "Streaming inputs must not persist activity rows"
    );
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let mut other = input(1);
    other["nodeId"] = json!("debug2");
    let advanced = tools.call("profile_debug_controller_input", other, "debug-regression");
    assert_eq!(advanced["ok"], true, "{advanced}");
    manager.profile_session.lock().unwrap().poll().unwrap();
    let replay = tools.call(
        "profile_debug_controller_input",
        input(160),
        "debug-regression",
    );
    assert_eq!(replay["ok"], true, "{replay}");
    assert_eq!(replay["data"]["replayed"], true);
    assert_eq!(
        tools.call(
            "profile_debug_controller_input",
            input(159),
            "debug-regression"
        )["ok"],
        false
    );
    let inspected =
        request(json!({"command":"profile_runtime_inspect","owner":owner,"nodeId":"identity"}));
    assert_eq!(inspected["ok"], true, "{inspected}");
    for port in inspected["state"]["profileInspection"]["ports"]
        .as_array()
        .unwrap()
    {
        assert_eq!(port["sample"]["value"]["value"], 160.0, "{port}");
    }
    let mut wrong = owner.clone();
    wrong["generation"] = json!(999);
    assert_eq!(
        tools.call(
            "profile_debug_controller_status",
            json!({"owner":wrong,"nodeId":"debug"}),
            "debug-regression"
        )["ok"],
        false
    );
    assert_eq!(request(json!({"command":"profile_stop"}))["ok"], true);
    assert_eq!(
        tools.call(
            "profile_debug_controller_input",
            input(161),
            "debug-regression"
        )["ok"],
        false
    );
    let new_owner = apply();
    assert_ne!(new_owner, owner);
    assert_eq!(
        tools.call(
            "profile_debug_controller_status",
            json!({"owner":new_owner,"nodeId":"debug"}),
            "debug-regression"
        )["data"]["values"][0]["value"],
        0.0
    );
    manager.shutdown();
}
