use super::{transport_tests::listen_observations, *};
use std::time::Instant;

const TOPIC: &str = "profiles.library";
const URI: &str = "cockpit://observations/profiles.library";

fn call(endpoint: &str, token: &str, name: &str, arguments: Value) -> Value {
    tests::request(
        endpoint,
        Some(token),
        json!({
            "jsonrpc":"2.0", "id":1, "method":"tools/call",
            "params":{"name":name, "arguments":arguments}
        }),
    )
    .unwrap()
    .into_json()
    .unwrap()
}

fn wait_until(mut condition: impl FnMut() -> bool, failure: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "{failure}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn unread_http_observation_streams_leave_profile_publication_and_stop_available() {
    let manager = Arc::new(GatewayManager::mcp_test_manager());
    let server = Server::open(manager.clone(), None);
    let read = server
        .create_credential("Slow readers".into(), "read".into(), 0)
        .unwrap();
    let write = server
        .create_credential("Independent writer".into(), "write".into(), 1)
        .unwrap();
    let reader = read["token"].as_str().unwrap();
    let writer = write["token"].as_str().unwrap();
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    server
        .configure(SettingsUpdate {
            expected_settings_revision: 2,
            enabled: true,
            port,
        })
        .unwrap();
    let endpoint = format!("http://127.0.0.1:{port}/mcp");
    let capacity = server.subscriptions.available_permits();
    assert!(capacity > 1);
    let mut fast = listen_observations(&endpoint, writer, &[URI]);
    assert_eq!(fast.next().unwrap()["method"], "notifications/subscriptions/acknowledged");
    assert_eq!(fast.next().unwrap()["params"]["uri"], URI);
    let mut streams = Vec::new();
    for _ in 1..capacity {
        let mut stream = listen_observations(&endpoint, reader, &[URI]);
        assert_eq!(
            stream.next().unwrap()["method"],
            "notifications/subscriptions/acknowledged"
        );
        assert_eq!(stream.next().unwrap()["params"]["uri"], URI);
        streams.push(stream);
    }
    assert_eq!(server.subscriptions.available_permits(), 0);

    let mut denied = listen_observations(&endpoint, reader, &[URI]);
    assert_eq!(
        denied.next().unwrap()["method"],
        "notifications/subscriptions/acknowledged"
    );
    assert!(denied.next().unwrap()["error"].is_object());
    drop(denied);

    let mut desktop = manager
        .observations
        .broker()
        .subscribe("desktop-test", "ui", TOPIC)
        .unwrap();
    assert_eq!(desktop.try_recv().unwrap().unwrap().kind, "snapshot");
    let name = "Independent profile with unread subscribers";
    let created = call(
        &endpoint,
        writer,
        "profile_create",
        json!({
            "operationId": uuid::Uuid::new_v4().to_string(), "name": name
        }),
    );
    assert_eq!(
        created["result"]["structuredContent"]["ok"], true,
        "{created}"
    );
    wait_until(
        || {
            desktop
                .try_recv()
                .unwrap()
                .and_then(|frame| frame.payload)
                .is_some_and(|payload| payload.to_string().contains(name))
        },
        "A desktop reader did not receive the committed profile update.",
    );
    let update = fast.next().expect("An established MCP reader must still receive updates");
    assert_eq!(update["params"]["uri"], URI);
    assert!(update["params"]["_meta"]["opensimgear.com/observation"]["payload"]
        .to_string().contains(name), "{update}");
    let stopped = call(
        &endpoint,
        writer,
        "profile_stop",
        json!({
            "operationId": uuid::Uuid::new_v4().to_string()
        }),
    );
    assert_eq!(
        stopped["result"]["structuredContent"]["ok"], true,
        "{stopped}"
    );

    drop(streams);
    drop(fast);
    wait_until(
        || server.subscriptions.available_permits() == capacity,
        "Closed HTTP observation bodies did not release their subscription permits.",
    );
    server.shutdown();
}

#[test]
fn http_observation_overflow_requires_a_fresh_generation_baseline() {
    let manager = Arc::new(GatewayManager::mcp_test_manager());
    let server = Server::open(manager.clone(), None);
    let credential = server.create_credential("Overflow reader".into(), "read".into(), 0).unwrap();
    let token = credential["token"].as_str().unwrap();
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    server.configure(SettingsUpdate { expected_settings_revision: 1, enabled: true, port }).unwrap();
    let endpoint = format!("http://127.0.0.1:{port}/mcp");
    let topic = "workspace.state";
    let uri = "cockpit://observations/workspace.state";
    let publisher = manager.observations.broker().register(osg_streams::TopicSpec {
        topic: topic.into(),
        source: "ordered-fixture".into(),
        audiences: vec!["mcp".into()],
    }, json!({"state":"initial"}), 0, "fixture".into()).unwrap();
    let mut stream = listen_observations(&endpoint, token, &[uri]);
    assert_eq!(stream.next().unwrap()["method"], "notifications/subscriptions/acknowledged");
    let mut next_frame = || stream.next().unwrap()["params"]["_meta"]["opensimgear.com/observation"].clone();
    let baseline = next_frame();
    assert_eq!(baseline["kind"], "snapshot");

    // Hold only delivery authorization, never the producer or broker. At
    // most one notification can leave the subscription before the hold.
    let authorization = server.state.lock().unwrap();
    for sequence in 1..=300 {
        publisher.publish(1, json!({"sequence":sequence}), json!({"edge":sequence}), sequence, "fixture").unwrap();
    }
    drop(authorization);
    let mut overflow = next_frame();
    if overflow["kind"] == "change" {
        overflow = next_frame();
    }
    assert_eq!(overflow["kind"], "loss");
    assert_eq!(overflow["reason"], "overflow");
    assert!(overflow.get("payload").is_none_or(Value::is_null));

    publisher.replace_generation(2, json!({"state":"replacement"}), 301, "fixture").unwrap();
    assert!(publisher.publish(1, json!("old"), json!({"edge":"old"}), 302, "fixture").is_err());
    let replacement = next_frame();
    assert_eq!(replacement["kind"], "loss");
    assert_eq!(replacement["reason"], "generation-replaced");
    let fresh = next_frame();
    assert_eq!(fresh["kind"], "snapshot");
    assert_eq!(fresh["generation"], "2");
    assert_eq!(fresh["payload"], json!({"state":"replacement"}));
    drop(stream);
    server.shutdown();
}
