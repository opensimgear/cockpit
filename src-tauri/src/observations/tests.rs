use super::*;
use osg_streams::TopicSpec;
use serde_json::json;
use std::{sync::mpsc, time::Duration};

#[test]
fn reader_lifecycle_notifies_input_metadata_and_existing_read_models(){
    let (runtime,_)=setup();
    let wakes=(0..12).map(|_|Arc::new(osg_streams::NativeWake::default())).collect::<Vec<_>>();
    for wake in &wakes {runtime.watch_reader_lifecycle(wake.clone());runtime.watch_reader_lifecycle(wake.clone());}
    assert_eq!(runtime.inner.reader_wake.lock().unwrap().len(),12);
    let (tx,rx)=mpsc::channel();
    let token=runtime.subscribe("lifecycle","ui","test.state",move|delivery|tx.send(delivery).map_err(|e|e.to_string())).unwrap();
    rx.recv_timeout(Duration::from_secs(1)).unwrap();runtime.unsubscribe("lifecycle",&token).unwrap();
    let deadline=Instant::now()+Duration::from_secs(1);
    while wakes.iter().any(|wake|wake.observed()==0){assert!(Instant::now()<deadline);std::thread::yield_now();}
    drop(wakes);runtime.watch_reader_lifecycle(Arc::new(osg_streams::NativeWake::default()));
    assert_eq!(runtime.inner.reader_wake.lock().unwrap().len(),1);
    runtime.close();
}

#[test]
fn manager_prepares_only_authorized_topics_before_browser_subscription() {
    let manager=crate::gateways::GatewayManager::mcp_test_manager();
    let unknown="controllers.input.11111111-1111-4111-8111-111111111111";
    let forged=manager.browser_observation_request(&json!({"operation":"subscribe","clientId":"another-client","topic":unknown}),|_|Ok(()));
    assert!(forged["error"].as_str().unwrap().contains("principal"));
    let invalid=manager.browser_observation_request(&json!({"operation":"subscribe","clientId":"browser-dev","topic":unknown,"authority":"output"}),|_|Ok(()));
    assert!(invalid["error"].as_str().unwrap().contains("fields"));
    let missing=manager.browser_observation_request(&json!({"operation":"subscribe","clientId":"browser-dev","topic":unknown}),|_|Ok(()));
    assert_eq!(missing["error"],"Unknown controller.");
    let usb=manager.browser_observation_request(&json!({"operation":"subscribe","clientId":"browser-dev",
        "topic":format!("usb.inputs.{}","a".repeat(64))}),|_|Ok(()));
    assert_eq!(usb["error"],"Unknown USB input endpoint topic.");
    let malformed=manager.browser_observation_request(&json!({"operation":"subscribe","clientId":"browser-dev",
        "topic":"usb.inputs.not-an-endpoint-hash"}),|_|Ok(()));
    assert!(malformed["error"].as_str().unwrap().contains("hash"));
    let metadata=manager.browser_observation_request(&json!({"operation":"subscribe","clientId":"browser-dev",
        "topic":format!("usb.metadata.{}","a".repeat(64))}),|_|Ok(()));
    assert_eq!(metadata["error"],"Unknown USB metadata endpoint topic.");
    let malformed_metadata=manager.browser_observation_request(&json!({"operation":"subscribe","clientId":"browser-dev",
        "topic":"usb.metadata.not-an-endpoint-hash"}),|_|Ok(()));
    assert!(malformed_metadata["error"].as_str().unwrap().contains("hash"));
    let (send,receive)=mpsc::channel();
    let opened=manager.browser_observation_request(&json!({"operation":"subscribe","clientId":"browser-dev","topic":"preferences.theme"}),move|frame|send.send(frame).map_err(|e|e.to_string()));
    assert_eq!(opened["ok"],true);
    let first=receive.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(first.frame.topic,"preferences.theme");
    let closed=manager.browser_observation_request(&json!({"operation":"unsubscribe","clientId":"browser-dev","subscriptionId":opened["subscriptionId"]}),|_|Ok(()));
    assert_eq!(closed["ok"],true);
    manager.shutdown();
}

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
fn remembered_inventory_publishes_only_its_committed_revision() {
    let manager = crate::gateways::GatewayManager::mcp_test_manager();
    let runtime = manager.observation_runtime();
    let mut browser = runtime.broker().subscribe("browser", "browser", "preferences.remembered").unwrap();
    let mut mcp = runtime.broker().subscribe("mcp-reader", "mcp", "preferences.remembered").unwrap();
    let initial = json!({"ready":true,"revision":"0"});
    assert_eq!(browser.recv().unwrap().payload, Some(initial.clone()));
    assert_eq!(mcp.recv().unwrap().payload, Some(initial));
    let value = json!({"components":[],"forgotten":{"offline":{"signature":"known","sawAbsent":false}}});
    manager.ui_preferences.write("remembered", 0, value.clone()).unwrap();
    assert!(manager.ui_preferences.write("remembered", 0, json!(null)).is_err());
    for reader in [&mut browser, &mut mcp] {
        let frame = reader.recv().unwrap();
        assert_eq!(frame.kind, "change");
        assert_eq!(frame.payload, Some(json!({"ready":true,"revision":"1"})));
    }
    assert_eq!(manager.ui_preferences.read("remembered").unwrap().value, value);
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
