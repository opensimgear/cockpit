//! Native desktop clients over real pinned WSS; fixture feedback is not game qualification.
use super::*;
use crate::{
    FeedbackBinding, FeedbackMode, FeedbackProvenance, FeedbackSample, FeedbackSource,
    FeedbackStatus, FeedbackValue, Publication, PublicationUpload,
};
#[cfg(windows)]
#[test]
fn large_feedback_snapshots_native_resync_freshness_and_disconnected_peers_are_isolated() {
    let directory = tempfile::tempdir().unwrap();
    let sink = Arc::new(Recorder::default());
    let hub = Hub::new("feedback-host".into(), sink.clone());
    let server = ServerRuntime::new(hub.clone(), directory.path().join("host")).unwrap();
    server
        .start(ServerConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            advertised_host: "127.0.0.1".into(),
            discovery: false,
        })
        .unwrap();
    let mut d: serde_json::Value = serde_json::from_str(&document()).unwrap();
    let bounds = d["ui"]["pages"][0]["items"][0]["bounds"].clone();
    let mut widgets = d["presentation"]["widgets"].as_array().unwrap().clone();
    let mut items = d["ui"]["pages"][0]["items"].as_array().unwrap().clone();
    let mut outputs = Vec::new();
    for i in 0..64 {
        outputs.push(json!({"id":format!("output-{i}"),"channelId":format!("channel-{i}"),"kind":"text","label":"Label"}));
        items.push(json!({"kind":"feedback","refId":format!("output-{i}"),"bounds":bounds}));
        widgets.push(json!({"id":format!("label-{i}"),"kind":"text","pageId":"page","label":"Label","bounds":bounds,"controlIds":[],"feedbackIds":[format!("output-{i}")],"config":{"prefix":"","suffix":""}}));
    }
    d["ui"]["feedbackWidgets"] = json!(outputs);
    d["ui"]["pages"][0]["items"] = json!(items);
    d["presentation"]["widgets"] = json!(widgets);
    let canonical = serde_json_canonicalizer::to_string(&d).unwrap();
    let publication_hash = tls::sha256(canonical.as_bytes());
    let upload = PublicationUpload {
        canonical_document: canonical,
        publication_hash: publication_hash.clone(),
        capability_fingerprint: None,
        assets: vec![],
    };
    let p = Publication::validate(upload.clone()).unwrap();
    let mut command = serde_json::to_value(upload).unwrap();
    command["command"] = json!("companion_publish");
    hub.command(&command).unwrap();
    let bindings = p
        .feedback
        .iter()
        .map(|d| FeedbackBinding {
            channel_id: d.channel_id.clone(),
            output_id: d.output_ids[0].clone(),
        })
        .collect::<Vec<_>>();
    let mut clients = Vec::new();
    for (i, instance) in ["large", "small"].iter().enumerate() {
        let offer = hub
            .command(&json!({"command":"companion_pair_offer"}))
            .unwrap();
        let payload = server
            .pairing_payload(
                offer["secret"].as_str().unwrap(),
                offer["expiresAtMs"].as_u64().unwrap(),
            )
            .unwrap();
        let client = ClientRuntime::open(directory.path().join(format!("client-{i}"))).unwrap();
        client
            .command(json!({"type":"pair-start","payload":payload,"name":format!("Client {i}")}))
            .unwrap();
        wait(|| client.status().phase == "awaiting-approval");
        let approved = hub
            .command(
                &json!({"command":"companion_pair_approve","requestId":client.status().request_id}),
            )
            .unwrap();
        hub.command(&json!({"command":"companion_assign","instanceId":instance,"companionId":approved["companionId"],"publicationHash":publication_hash})).unwrap();
        wait(|| client.status().phase == "paired" && !client.status().busy);
        hub.configure_feedback(
            instance,
            &publication_hash,
            if i == 0 { &bindings } else { &bindings[..1] },
        )
        .unwrap();
        client
            .command(json!({"type":"connect","instanceId":instance}))
            .unwrap();
        wait(|| client.status().phase == "ready");
        clients.push(client);
    }
    let sample = |b: &FeedbackBinding, text: String, provenance| FeedbackSample {
        channel_id: b.channel_id.clone(),
        output_id: b.output_id.clone(),
        value: Some(FeedbackValue::Text(text)),
        status: FeedbackStatus::Fresh,
        provenance,
        source_kind: if provenance == FeedbackProvenance::GameConfirmed {
            FeedbackSource::Simulator
        } else {
            FeedbackSource::Host
        },
        source_age_ms: 0,
        unit: "text".into(),
        range: None,
    };
    let large = hub.feedback_token("large").unwrap();
    let small = hub.feedback_token("small").unwrap();
    let samples = bindings
        .iter()
        .map(|b| {
            sample(
                b,
                char::from_u32(0x754c).unwrap().to_string().repeat(256),
                FeedbackProvenance::GameConfirmed,
            )
        })
        .collect::<Vec<_>>();
    hub.publish_feedback(&large, &samples, FeedbackMode::Snapshot, hub.now_ms())
        .unwrap();
    hub.publish_feedback(
        &small,
        &[sample(
            &bindings[0],
            "Requested".into(),
            FeedbackProvenance::Requested,
        )],
        FeedbackMode::Snapshot,
        hub.now_ms(),
    )
    .unwrap();
    wait(|| {
        clients[0].status().feedback.len() == 64
            && clients[0]
                .status()
                .feedback
                .iter()
                .all(|s| s["status"] == "fresh" && s["provenance"] == "game-confirmed")
    });
    wait(|| {
        clients[1]
            .status()
            .feedback
            .first()
            .is_some_and(|s| s["value"] == "Requested")
    });
    assert!(serde_json::to_vec(&clients[0].status()).unwrap().len() < crate::FEEDBACK_BYTES);
    assert!(clients[0].status().ack.unwrap().get("feedback").is_none());
    let before = sink.events.lock().unwrap().clone();
    let revision = clients[0].status().feedback_revision;
    let epoch = clients[0].status().epoch;
    clients[0]
        .command(json!({"type":"feedback-resync","epoch":epoch}))
        .unwrap();
    wait(|| {
        clients[0].status().feedback_revision > revision && clients[0].status().feedback.len() == 64
    });
    assert_eq!(clients[0].status().phase, "ready");
    assert_eq!(*sink.events.lock().unwrap(), before);
    wait(|| {
        clients[1]
            .status()
            .feedback
            .first()
            .is_some_and(|s| s["status"] == "stale")
    });
    clients[1].command(json!({"type":"disconnect"})).unwrap();
    wait(|| hub.feedback_token("small").is_err());
    assert!(hub
        .publish_feedback(&small, &[], FeedbackMode::Delta, hub.now_ms())
        .is_err());
    hub.clear_feedback("large", &publication_hash, hub.now_ms())
        .unwrap();
    assert!(hub
        .publish_feedback(&large, &samples, FeedbackMode::Snapshot, hub.now_ms())
        .is_err());
    let current = hub.feedback_token("large").unwrap();
    hub.publish_feedback(
        &current,
        &[sample(
            &bindings[0],
            "New epoch".into(),
            FeedbackProvenance::HostAccepted,
        )],
        FeedbackMode::Snapshot,
        hub.now_ms(),
    )
    .unwrap();
    wait(|| {
        clients[0]
            .status()
            .feedback
            .first()
            .is_some_and(|s| s["value"] == "New epoch")
    });
    assert_eq!(clients[0].status().phase, "ready");
    server.stop();
}
