use super::*;
use osg_streams::TopicSpec;
use serde_json::json;
use std::{sync::mpsc, time::Duration};

fn setup() -> (Runtime, osg_streams::Publisher) {
    let runtime = Runtime::new();
    let publisher = runtime
        .broker()
        .register(
            TopicSpec {
                topic: "test.state".into(),
                source: "test".into(),
                audiences: vec!["ui".into(), "browser".into()],
            },
            json!(0),
            1,
            "test-clock".into(),
        )
        .unwrap();
    (runtime, publisher)
}

#[test]
fn exact_receipts_bound_inflight_and_scope_cancellation() {
    let (runtime, publisher) = setup();
    let (send, receive) = mpsc::channel();
    let token = runtime
        .subscribe("window:main", "ui", "test.state", move |frame| {
            send.send(frame).map_err(|error| error.to_string())
        })
        .unwrap();
    let snapshot = receive.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(snapshot.delivery, "1");
    assert_eq!(snapshot.frame.kind, "snapshot");
    publisher
        .publish(1, json!(1), json!(1), 2, "test-clock")
        .unwrap();
    assert!(receive.recv_timeout(Duration::from_millis(20)).is_err());
    assert!(runtime.acknowledge("other", &token, "1").is_err());
    assert!(runtime.unsubscribe("other", &token).is_err());
    assert!(runtime.acknowledge("window:main", &token, "2").is_err());
    runtime.acknowledge("window:main", &token, "1").unwrap();
    let change = receive.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(change.delivery, "2");
    assert_eq!(change.frame.payload, Some(json!(1)));
    assert!(runtime.acknowledge("window:main", &token, "1").is_err());
    runtime.unsubscribe("window:main", &token).unwrap();
    runtime.unsubscribe("window:main", &token).unwrap();
    assert!(runtime.acknowledge("window:main", &token, "2").is_err());
    runtime.close();
}

#[test]
fn slow_transport_does_not_block_a_producer_or_another_consumer() {
    let (runtime, publisher) = setup();
    let (release, wait) = mpsc::channel();
    let token = runtime
        .subscribe("slow", "ui", "test.state", move |_| {
            wait.recv().map_err(|error| error.to_string())
        })
        .unwrap();
    let (send, receive) = mpsc::channel();
    let healthy = runtime
        .subscribe("healthy", "ui", "test.state", move |frame| {
            send.send(frame).map_err(|error| error.to_string())
        })
        .unwrap();
    receive.recv_timeout(Duration::from_secs(1)).unwrap();
    runtime.acknowledge("healthy", &healthy, "1").unwrap();
    publisher
        .publish(1, json!(1), json!(1), 2, "test-clock")
        .unwrap();
    assert_eq!(
        receive
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .frame
            .payload,
        Some(json!(1))
    );
    runtime.unsubscribe("slow", &token).unwrap();
    drop(release);
    runtime.close_client("healthy");
    assert!(runtime.inner.sessions.lock().unwrap().is_empty());
    runtime.close();
}

#[test]
fn preference_commits_publish_full_values_without_querying_or_authority() {
    let manager = crate::gateways::GatewayManager::mcp_test_manager();
    let runtime = manager.observation_runtime();
    let mut first = runtime
        .broker()
        .subscribe("first", "ui", "preferences.theme")
        .unwrap();
    let mut second = runtime
        .broker()
        .subscribe("second", "mcp", "preferences.theme")
        .unwrap();
    assert_eq!(first.recv().unwrap().payload.unwrap()["revision"], "0");
    second.recv().unwrap();
    manager
        .ui_preferences
        .write("theme", 0, json!(true))
        .unwrap();
    for reader in [&mut first, &mut second] {
        let frame = reader.recv().unwrap();
        assert_eq!(frame.kind, "change");
        assert_eq!(
            frame.payload.unwrap(),
            json!({"ready":true,"revision":"1","value":true})
        );
    }
    assert!(runtime
        .broker()
        .subscribe("other", "ui", "preferences.windows")
        .is_err());
    manager.shutdown();
}

#[test]
fn browser_controls_cannot_select_another_principal_or_general_dispatch() {
    let (runtime, _) = setup();
    for request in [
        json!({}),
        json!({"operation":"subscribe","clientId":"admin","topic":"test.state"}),
        json!({"operation":"subscribe","clientId":"browser-dev","topic":"test.state","audience":"ui"}),
        json!({"operation":"profile_apply","clientId":"browser-dev"}),
    ] {
        assert_eq!(runtime.browser_request(&request, |_| Ok(()))["ok"], false);
    }
    let result = runtime.browser_request(
        &json!({"operation":"subscribe","clientId":"browser-dev","topic":"test.state"}),
        |_| Err("closed".into()),
    );
    assert_eq!(result["ok"], true);
    runtime.close();
}

#[test]
fn browser_protocol_and_runtime_caps_close_without_command_authority() {
    let (runtime, publisher) = setup();
    let (send, receive) = mpsc::channel();
    let result = runtime.browser_request(
        &json!({"operation":"subscribe","clientId":"browser-dev","topic":"test.state"}),
        move |frame| send.send(frame).map_err(|error| error.to_string()),
    );
    let token = result["subscriptionId"].as_str().unwrap();
    receive.recv_timeout(Duration::from_secs(1)).unwrap();
    let ack = json!({"operation":"acknowledge","clientId":"browser-dev","subscriptionId":token,"delivery":"1"});
    assert_eq!(runtime.browser_request(&ack, |_| Ok(()))["ok"], true);
    publisher
        .publish(1, json!(1), json!(1), 2, "test-clock")
        .unwrap();
    receive.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(
        runtime.browser_request(
            &json!({"operation":"unsubscribe","clientId":"browser-dev","subscriptionId":token}),
            |_| Ok(())
        )["ok"],
        true
    );
    let (capped, _) = setup();
    capped.inner.workers.store(64, Ordering::Release);
    assert!(capped
        .subscribe("capped", "ui", "test.state", |_| Ok(()))
        .is_err());
    capped.close();
    runtime.close();
    assert!(runtime
        .subscribe("closed", "ui", "test.state", |_| Ok(()))
        .is_err());
}
