use super::*;

#[test]
fn opening_session_has_a_bounded_transfer_window_and_armed_session_keeps_short_lease() {
    let sink = Arc::new(Sink::default());
    let hub = Hub::new("host".into(), sink);
    let token = paired(&hub, "screen");
    let publication = publish(&hub, fixture());
    assign(&hub, "screen", "instance", &publication.publication_hash);
    let open = connect(&hub, "connection", &token, "instance");
    let opened_at = hub.now_ms();

    let mut heartbeat = frame(&open, "heartbeat", "1");
    // `connect` samples the hub clock for auth/open independently, so leave a
    // small margin while still exercising a delay much longer than 1500ms.
    let prebaseline_at = opened_at + PANEL_PREPARATION_LEASE_MS - 100;
    hub.tick(prebaseline_at);
    assert_eq!(
        hub.handle_wire(heartbeat.clone(), "connection", prebaseline_at)["ok"],
        true,
        "The initial open response and panel preparation may exceed the ordinary input lease."
    );

    assert_eq!(
        baseline(&hub, "connection", &open, &publication)["ok"],
        true
    );
    let baseline_at = prebaseline_at;
    let armed_heartbeat_at = baseline_at + LEASE_MS - 1;
    hub.tick(armed_heartbeat_at);
    heartbeat["sequence"] = json!("2");
    assert_eq!(
        hub.handle_wire(heartbeat.clone(), "connection", armed_heartbeat_at)["ok"],
        true
    );

    let expired_at = armed_heartbeat_at + LEASE_MS;
    hub.tick(expired_at);
    heartbeat["sequence"] = json!("3");
    let expired = hub.handle_wire(heartbeat, "connection", expired_at);
    assert_eq!(
        expired["ok"], false,
        "An armed input session must still expire at 1500ms."
    );
    assert!(expired["error"]
        .as_str()
        .unwrap()
        .contains("Session is obsolete"));
}

#[test]
fn unopened_baseline_session_expires_at_the_bounded_preparation_deadline() {
    let hub = Hub::new("host".into(), Arc::new(NullSourceSink));
    let token = paired(&hub, "screen");
    let publication = publish(&hub, fixture());
    assign(&hub, "screen", "instance", &publication.publication_hash);
    let open = connect(&hub, "connection", &token, "instance");
    let expired_at = hub.now_ms() + PANEL_PREPARATION_LEASE_MS;

    hub.tick(expired_at);
    let expired = hub.handle_wire(frame(&open, "heartbeat", "1"), "connection", expired_at);
    assert_eq!(expired["ok"], false);
    assert!(expired["error"]
        .as_str()
        .unwrap()
        .contains("Session is obsolete"));
}
