use super::*;
fn binding(p: &Publication) -> FeedbackBinding {
    let d = p
        .feedback
        .iter()
        .find(|d| d.kind == FeedbackKind::Number)
        .unwrap();
    FeedbackBinding {
        channel_id: d.channel_id.clone(),
        output_id: d.output_ids[0].clone(),
    }
}
fn sample(p: &Publication) -> FeedbackSample {
    let b = binding(p);
    let d = p
        .feedback
        .iter()
        .find(|d| d.channel_id == b.channel_id)
        .unwrap();
    FeedbackSample {
        channel_id: b.channel_id,
        output_id: b.output_id,
        value: Some(FeedbackValue::Number(42.)),
        status: FeedbackStatus::Fresh,
        provenance: FeedbackProvenance::GameConfirmed,
        source_kind: FeedbackSource::Simulator,
        source_age_ms: 100,
        unit: d.unit.clone().unwrap(),
        range: d.range.clone(),
    }
}
fn setup() -> (Hub, Arc<Sink>, Publication, Value) {
    let sink = Arc::new(Sink::default());
    let h = Hub::new("host".into(), sink.clone());
    let token = paired(&h, "peer");
    let p = publish(&h, fixture());
    assign(&h, "peer", "instance", &p.publication_hash);
    let open = connect(&h, "connection", &token, "instance");
    baseline(&h, "connection", &open, &p);
    (h, sink, p, open)
}
#[test]
fn native_feedback_preserves_provenance_freshness_and_source_acceptance_separation() {
    let (h, sink, p, open) = setup();
    assert!(h.feedback_token("instance").is_err());
    h.configure_feedback("instance", &p.publication_hash, &[binding(&p)])
        .unwrap();
    let token = h.feedback_token("instance").unwrap();
    let before = sink.events.lock().unwrap().len();
    let now = h.now_ms() + 100;
    h.publish_feedback(&token, &[sample(&p)], FeedbackMode::Snapshot, now)
        .unwrap();
    assert_eq!(sink.events.lock().unwrap().len(), before);
    let reply = h.handle_wire(frame(&open, "heartbeat", "1"), "connection", now + 100);
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["ackStage"], "source-accepted");
    assert_eq!(reply["submitted"], false);
    assert_eq!(reply["feedback"]["mode"], "snapshot");
    assert_eq!(
        reply["feedback"]["values"][0]["provenance"],
        "game-confirmed"
    );
    assert_eq!(reply["feedback"]["values"][0]["sourceAgeMs"], 200);
    assert_eq!(reply["feedback"]["values"][0]["status"], "fresh");
    let empty = h.handle_wire(frame(&open, "heartbeat", "2"), "connection", now + 200);
    assert_eq!(empty["feedback"]["mode"], "delta");
    assert!(empty["feedback"]["values"].as_array().unwrap().is_empty());
    let resync = h.handle_wire(
        frame(&open, "feedback-resync", "1"),
        "connection",
        now + 1000,
    );
    assert_eq!(resync["feedback"]["mode"], "snapshot");
    assert_eq!(resync["feedback"]["values"][0]["status"], "stale");
    assert_eq!(sink.events.lock().unwrap().len(), before);
    assert!(h
        .command(
            &json!({"command":"companion_feedback","instanceId":"instance","values":[sample(&p)]})
        )
        .is_err());
    assert_eq!(
        wire(
            &h,
            "connection",
            json!({"type":"feedback","values":[sample(&p)]})
        )["ok"],
        false
    );
}
#[test]
fn wrong_controller_output_revision_and_old_session_are_isolated() {
    let (h, sink, p, open) = setup();
    h.configure_feedback("instance", &p.publication_hash, &[binding(&p)])
        .unwrap();
    let token = h.feedback_token("instance").unwrap();
    let now = h.now_ms() + 100;
    for fault in 0..5 {
        let mut bad = token.clone();
        match fault {
            0 => bad.instance_id = "wrong".into(),
            1 => bad.publication_hash = format!("sha256:{}", "b".repeat(64)),
            2 => bad.session_id = "old-session".into(),
            3 => bad.generation += 1,
            _ => bad.feedback_generation += 1,
        };
        assert!(h
            .publish_feedback(&bad, &[sample(&p)], FeedbackMode::Snapshot, now)
            .is_err());
    }
    let mut bad = sample(&p);
    bad.output_id = "well-formed-wrong-output".into();
    assert!(h
        .publish_feedback(&token, &[bad], FeedbackMode::Delta, now)
        .is_err());
    let mut bad = sample(&p);
    bad.source_kind = FeedbackSource::Host;
    assert!(h
        .publish_feedback(&token, &[bad], FeedbackMode::Delta, now)
        .is_err());
    let before = sink.events.lock().unwrap().len();
    h.clear_feedback("instance", &p.publication_hash, now)
        .unwrap();
    assert_eq!(sink.events.lock().unwrap().len(), before);
    assert!(h
        .publish_feedback(&token, &[sample(&p)], FeedbackMode::Snapshot, now + 34)
        .is_err());
    let replacement = h.feedback_token("instance").unwrap();
    assert!(replacement.feedback_generation > token.feedback_generation);
    let response = h.handle_wire(frame(&open, "heartbeat", "1"), "connection", now + 35);
    assert_eq!(response["feedback"]["values"][0]["status"], "unknown");
    h.publish_feedback(
        &replacement,
        &[sample(&p)],
        FeedbackMode::Snapshot,
        now + 40,
    )
    .unwrap();
    h.disconnect_connection("connection", now + 50);
    assert!(h.feedback_token("instance").is_err());
    assert!(h
        .publish_feedback(&replacement, &[sample(&p)], FeedbackMode::Delta, now + 100)
        .is_err());
    let credential = paired(&h, "other");
    assign(&h, "other", "second", &p.publication_hash);
    let second = connect(&h, "second-connection", &credential, "second");
    baseline(&h, "second-connection", &second, &p);
    h.configure_feedback("second", &p.publication_hash, &[binding(&p)])
        .unwrap();
    let second_token = h.feedback_token("second").unwrap();
    assert!(h
        .publish_feedback(&replacement, &[sample(&p)], FeedbackMode::Delta, now + 150)
        .is_err());
    h.publish_feedback(
        &second_token,
        &[sample(&p)],
        FeedbackMode::Snapshot,
        now + 150,
    )
    .unwrap();
    let response = h.handle_wire(
        frame(&second, "heartbeat", "1"),
        "second-connection",
        now + 151,
    );
    assert_eq!(response["feedback"]["controllerInstanceId"], "second");
    assert_eq!(response["feedback"]["values"][0]["status"], "fresh");
}
#[test]
fn route_configuration_is_atomic_idempotent_and_cannot_spoof_a_published_output() {
    let (h, _, p, open) = setup();
    let b = binding(&p);
    let mut bad = b.clone();
    bad.output_id = "other-output".into();
    assert!(h
        .configure_feedback("instance", &p.publication_hash, &[bad])
        .is_err());
    assert!(h
        .configure_feedback("other", &p.publication_hash, &[b.clone()])
        .is_err());
    assert!(h
        .configure_feedback("instance", "sha256:wrong", &[b.clone()])
        .is_err());
    h.configure_feedback("instance", &p.publication_hash, &[b.clone()])
        .unwrap();
    let old = h.feedback_token("instance").unwrap();
    h.configure_feedback("instance", &p.publication_hash, &[b.clone()])
        .unwrap();
    assert_eq!(h.feedback_token("instance").unwrap(), old);
    assert!(h
        .configure_feedback("instance", &p.publication_hash, &[b.clone(), b])
        .is_err());
    assert_eq!(h.feedback_token("instance").unwrap(), old);
    h.configure_feedback("instance", &p.publication_hash, &[])
        .unwrap();
    assert!(h
        .publish_feedback(&old, &[sample(&p)], FeedbackMode::Snapshot, h.now_ms())
        .is_err());
    let response = wire(&h, "connection", frame(&open, "heartbeat", "1"));
    assert_eq!(response["feedback"]["mode"], "snapshot");
    assert!(response["feedback"]["values"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(h.clear_feedback("instance", "stale", h.now_ms()).is_err());
}
#[test]
fn a_slow_peer_coalesces_latest_values_while_another_peer_keeps_independent_deltas() {
    let h = Hub::new("host".into(), Arc::new(Sink::default()));
    let p = publish(&h, fixture());
    let mut opens = Vec::new();
    for (id, connection) in [
        ("slow", "slow-connection"),
        ("healthy", "healthy-connection"),
    ] {
        let token = paired(&h, id);
        assign(&h, id, id, &p.publication_hash);
        let open = connect(&h, connection, &token, id);
        baseline(&h, connection, &open, &p);
        h.configure_feedback(id, &p.publication_hash, &[binding(&p)])
            .unwrap();
        opens.push(open);
    }
    let slow = h.feedback_token("slow").unwrap();
    let healthy = h.feedback_token("healthy").unwrap();
    let base = h.now_ms() + 100;
    for i in 0..20 {
        let now = base + i * 34;
        let mut value = sample(&p);
        value.value = Some(FeedbackValue::Number(i as f64));
        h.publish_feedback(&slow, &[value.clone()], FeedbackMode::Delta, now)
            .unwrap();
        h.publish_feedback(&healthy, &[value], FeedbackMode::Delta, now)
            .unwrap();
        let reply = h.handle_wire(
            frame(&opens[1], "heartbeat", &(i + 1).to_string()),
            "healthy-connection",
            now,
        );
        assert_eq!(
            reply["feedback"]["values"][0]["value"].as_f64(),
            Some(i as f64)
        );
    }
    let reply = h.handle_wire(
        frame(&opens[0], "heartbeat", "1"),
        "slow-connection",
        base + 680,
    );
    assert_eq!(reply["feedback"]["mode"], "snapshot");
    assert_eq!(reply["feedback"]["values"].as_array().unwrap().len(), 1);
    assert_eq!(reply["feedback"]["values"][0]["value"].as_f64(), Some(19.));
    let mut bad = sample(&p);
    bad.value = Some(FeedbackValue::Number(101.));
    assert!(h
        .publish_feedback(&slow, &[bad], FeedbackMode::Delta, base + 714)
        .is_err());
    assert!(h
        .publish_feedback(&healthy, &[sample(&p)], FeedbackMode::Delta, base + 714)
        .is_ok());
    h.tick(base + 2200);
    assert!(h.feedback_token("slow").is_err());
    assert!(h
        .publish_feedback(&slow, &[], FeedbackMode::Snapshot, base + 2200)
        .is_err());
}
