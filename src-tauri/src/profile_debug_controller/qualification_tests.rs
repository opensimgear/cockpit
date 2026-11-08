use super::*;
use osg_profiles::{
    Binding, Direction, Endpoint, Port, PortContract, PortValue, ProcessingNode, Resource,
    ResourceKind,
};

#[test]
fn qualification_duration_preserves_both_stop_edges_within_the_capture_window() {
    assert_eq!(burst_duration(None), Ok(192));
    assert_eq!(burst_duration(Some(4_800)), Ok(4_800));
    for duration in [0, 176, 200, 4_816, u64::MAX] {
        assert!(burst_duration(Some(duration)).is_err());
    }
}

#[test]
fn qualification_requires_explicit_enable_and_the_current_neutral_owner() {
    let manager = GatewayManager::mcp_test_manager();
    if !matches!(
        std::env::var("OSG_REACTIVE_QUALIFICATION").as_deref(),
        Ok("1")
    ) {
        let result = run(&manager, Value::Null);
        assert_eq!(
            result["error"],
            "The desktop qualification source is disabled."
        );
        manager.shutdown();
        return;
    }

    let mut document = osg_profiles::default_document();
    let port = |id: &str, direction| Port {
        id: id.into(),
        name: id.into(),
        direction,
        contract: PortContract::State {
            schema: osg_mapping::signal::ScalarSchema::Boolean {},
            freshness_ms: 1000,
            fallback: Some(PortValue::Boolean(false)),
        },
    };
    let nodes = ["debug", "debug-peer"];
    for node in nodes {
        let sink = format!("{node}-sink");
        document.resources.push(Resource {
            id: node.into(),
            name: node.into(),
            kind: ResourceKind::DebugController,
            required: false,
            ports: vec![port("button-1", Direction::Output)],
        });
        document.processing_nodes.push(ProcessingNode {
            id: sink.clone(),
            name: sink.clone(),
            operation: osg_mapping::program::Operation::Identity {},
            draft: None,
            inputs: vec![port("in", Direction::Input)],
            outputs: vec![port("out", Direction::Output)],
        });
        document.bindings.push(Binding {
            id: format!("{node}-wire"),
            from: Endpoint {
                node_id: node.into(),
                port_id: "button-1".into(),
            },
            to: Endpoint {
                node_id: sink,
                port_id: "in".into(),
            },
        });
    }
    let saved = manager.request(json!({
        "command":"profile_editor_save", "clientId":CLIENT_ID, "document":document,
        "expectedRevision":1, "assignments":[]
    }));
    assert_eq!(saved["ok"], true, "{saved}");
    let applied = manager.request(json!({
        "command":"profile_apply", "clientId":CLIENT_ID, "id":document.id, "expectedRevision":2
    }));
    assert_eq!(applied["ok"], true, "{applied}");
    let owner = &applied["state"]["profiles"]["profileSession"]["owner"];
    let clock = Clock::new().unwrap();
    let request = || {
        json!({
            "command":"qualification_debug_burst", "owner":owner, "nodes":nodes,
            "startNs":(clock.now().unwrap() + 50 * NS_PER_MS).to_string()
        })
    };
    let result = run(&manager, request());
    assert_eq!(result["ok"], true, "{result}");
    for (index, count) in [193, 25].into_iter().enumerate() {
        let lane = result["state"]["qualificationBurst"][index]
            .as_array()
            .unwrap();
        assert_eq!(lane.len(), count);
        assert!(lane
            .iter()
            .enumerate()
            .all(|(edge, row)| row["value"] == (edge % 2 == 0)));
    }
    let held = run(&manager, request());
    assert_eq!(
        held["error"],
        "Both qualification lanes require an applied neutral debug button."
    );
    let stopped = manager.request(json!({"command":"profile_stop", "clientId":CLIENT_ID}));
    assert_eq!(stopped["ok"], true, "{stopped}");
    assert_eq!(run(&manager, request())["ok"], false);
    manager.shutdown();
}
