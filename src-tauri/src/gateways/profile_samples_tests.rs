use super::*;
use crate::profile_session::{inspection::samples::topic, RunOwner};
use osg_profiles::{Binding, Direction, Endpoint, Port, PortContract, PortValue, ProcessingNode,
    Resource, ResourceKind};
use std::time::{Duration, Instant};

fn request(manager: &GatewayManager, mut value: Value) -> Value {
    value["clientId"] = json!("profile-samples-gateway-test");
    let response = manager.request(value);
    assert_eq!(response["ok"], true, "{response}");
    response
}

fn fixture() -> (GatewayManager, RunOwner) {
    let manager = GatewayManager::mcp_test_manager();
    let mut document = osg_profiles::default_document();
    let contract = PortContract::State {
        schema: osg_mapping::signal::ScalarSchema::Integer {
            min: -32768, max: 32767, unit: None,
        },
        freshness_ms: 1000,
        fallback: Some(PortValue::Number(0.0)),
    };
    let port = |id: &str, direction| Port {
        id: id.into(), name: id.into(), direction, contract: contract.clone(),
    };
    document.resources.push(Resource {
        id: "debug".into(), name: "Debug".into(), kind: ResourceKind::DebugController,
        required: false, ports: vec![port("axis-1", Direction::Output)],
    });
    document.processing_nodes.push(ProcessingNode {
        id: "identity".into(), name: "Identity".into(),
        operation: osg_mapping::program::Operation::Identity {}, draft: None,
        inputs: vec![port("in", Direction::Input)], outputs: vec![port("out", Direction::Output)],
    });
    document.bindings.push(Binding {
        id: "wire".into(),
        from: Endpoint { node_id: "debug".into(), port_id: "axis-1".into() },
        to: Endpoint { node_id: "identity".into(), port_id: "in".into() },
    });
    request(&manager, json!({"command":"profile_editor_save","document":document,
        "expectedRevision":1,"assignments":[]}));
    let applied = request(&manager, json!({"command":"profile_apply","id":document.id,"expectedRevision":2}));
    let owner = serde_json::from_value(applied["state"]["profiles"]["profileSession"]["owner"].clone()).unwrap();
    (manager, owner)
}

fn frame(reader: &mut osg_streams::Reader) -> osg_streams::WireFrame {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(frame) = reader.try_recv().unwrap() { return frame; }
        assert!(Instant::now() < deadline, "Native node samples were not delivered");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn profile_samples_gateway_is_lazy_delivers_actual_mapping_and_retires_exact_owner() {
    let (manager, owner) = fixture();
    let broker = manager.observations.broker();
    let sample_topic = topic(&owner, "identity");
    assert!(broker.subscribe("before", "ui", &sample_topic).is_err(), "Apply must not eagerly register samples");
    assert_eq!(manager.profile_session.lock().unwrap().service_interval(), None);
    let before = request(&manager, json!({"command":"profile_debug_controller_status","owner":owner,"nodeId":"debug"}));

    let mut reader = manager.with_profile_samples(&sample_topic,
        || broker.subscribe("preview", "ui", &sample_topic).map_err(|error| error.to_string())).unwrap();
    let baseline = frame(&mut reader);
    assert_eq!(baseline.kind, "snapshot");
    assert_eq!(baseline.source, "profile-samples");
    let payload = baseline.payload.unwrap();
    assert_eq!(payload["owner"], json!(owner));
    assert_eq!(payload["nodeId"], "identity");
    assert_eq!(payload["appliedSnapshot"]["revision"], 2);
    assert_eq!(payload["ports"].as_array().unwrap().len(), 2);
    for port in payload["ports"].as_array().unwrap() {
        assert_eq!(port["sample"]["value"]["value"], 0.0, "{port}");
    }
    let after = request(&manager, json!({"command":"profile_debug_controller_status","owner":owner,"nodeId":"debug"}));
    assert!(before["state"]["debugController"]["values"].is_array());
    assert_eq!(before["state"]["debugController"], after["state"]["debugController"]);
    assert_eq!(manager.profile_session.lock().unwrap().status().owner, Some(owner.clone()));
    assert_eq!(manager.profile_session.lock().unwrap().service_interval(), None,
        "Preview must not start a native polling schedule");

    request(&manager, json!({"command":"profile_debug_controller_input","owner":owner,"nodeId":"debug",
        "streamId":"22222222-2222-4222-8222-222222222222","sequence":"1",
        "changes":[{"portId":"axis-1","value":7}]}));
    manager.profile_session.lock().unwrap().poll().unwrap();
    let changed = frame(&mut reader);
    assert_eq!(changed.kind, "snapshot");
    for port in changed.payload.unwrap()["ports"].as_array().unwrap() {
        assert_eq!(port["sample"]["value"]["value"], 7.0, "{port}");
    }

    let mut reached = false;
    assert!(manager.with_profile_samples(&topic(&owner, "missing"), || { reached = true; Ok(()) }).is_err());
    assert!(!reached, "Invalid node queries must fail before opening a subscription");
    request(&manager, json!({"command":"profile_stop"}));
    assert_eq!(frame(&mut reader).kind, "loss");
    assert!(manager.with_profile_samples(&sample_topic, || { reached = true; Ok(()) }).is_err());
    assert!(!reached, "A retired exact owner must never reopen its sample topic");
    assert!(broker.subscribe("retired", "ui", &sample_topic).is_err());

    let applied = request(&manager, json!({"command":"profile_apply","id":owner.profile_id,"expectedRevision":2}));
    let newer: RunOwner = serde_json::from_value(applied["state"]["profiles"]["profileSession"]["owner"].clone()).unwrap();
    assert_ne!(topic(&newer, "identity"), sample_topic);
    manager.shutdown();
}

#[test]
fn profile_samples_gateway_last_delivery_reader_releases_lazy_topic_without_manual_wake() {
    let (manager, owner) = fixture();
    let sample_topic = topic(&owner, "identity");
    let (send, received) = std::sync::mpsc::channel();
    let token = manager.with_profile_samples(&sample_topic, || manager.observations.subscribe(
        "lifecycle", "ui", &sample_topic,
        move |delivery| send.send(delivery).map_err(|error| error.to_string()))).unwrap();
    let delivery = received.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(delivery.frame.kind, "snapshot");
    manager.observations.acknowledge("lifecycle", &token, &delivery.delivery).unwrap();
    manager.observations.unsubscribe("lifecycle", &token).unwrap();

    // Probes use real delivery lifecycles too. Their final reader drops wake
    // idle retirement; no direct sample-worker notification is injected here.
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match manager.observations.subscribe("idle-probe", "ui", &sample_topic, |_| Ok(())) {
            Err(error) if error == "UnknownTopic" => break,
            Err(error) => panic!("Unexpected observation probe error: {error}"),
            Ok(token) => manager.observations.unsubscribe("idle-probe", &token).unwrap(),
        }
        assert!(Instant::now() < deadline, "Last-reader lifecycle did not retire the sample topic");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(manager.profile_session.lock().unwrap().status().owner, Some(owner));
    let broker = manager.observations.broker();
    let mut fresh = manager.with_profile_samples(&sample_topic,
        || broker.subscribe("reopened", "mcp", &sample_topic).map_err(|error| error.to_string())).unwrap();
    assert_eq!(frame(&mut fresh).kind, "snapshot");
    manager.shutdown();
}
