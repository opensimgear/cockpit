use super::*;
use osg_companion_service::{NullSourceSink, Publication, PublicationUpload};
use std::sync::Arc;
fn wire(h: &Hub, connection: &str, frame: Value) -> Value {
    let reply = h.handle_wire(frame, connection, h.now_ms());
    assert_eq!(reply["ok"], true, "{reply}");
    reply
}
fn paired(h: &Hub, id: &str) -> String {
    let offer = h
        .command(&json!({"command":"companion_pair_offer"}))
        .unwrap();
    let request = wire(
        h,
        "pairing",
        json!({"type":"pair","secret":offer["secret"],"companionId":id,"name":id}),
    );
    h.command(&json!({"command":"companion_pair_approve","requestId":request["requestId"],"companionId":id})).unwrap();
    wire(
        h,
        "pairing",
        json!({"type":"pair-status","secret":offer["secret"],"requestId":request["requestId"]}),
    )["token"]
        .as_str()
        .unwrap()
        .into()
}
fn assign(h: &Hub, instance: &str, companion: &str, publication: &Publication) {
    h.command(&json!({"command":"companion_assign","instanceId":instance,"companionId":companion,"publicationHash":publication.publication_hash})).unwrap();
}
fn setup() -> (Hub, Publication, String) {
    let hub = Hub::new("retained-cleanup".into(), Arc::new(NullSourceSink));
    let token = paired(&hub, "installation-a");
    paired(&hub, "installation-b");
    paired(&hub, "installation-c");
    let upload: PublicationUpload = serde_json::from_str(include_str!(
        "../../crates/companion-service/tests/fixtures/c1-flight-publication.json"
    ))
    .unwrap();
    let publication = Publication::validate(upload.clone()).unwrap();
    let mut request = serde_json::to_value(upload).unwrap();
    request["command"] = json!("companion_publish");
    hub.command(&request).unwrap();
    assign(&hub, "panel-a", "installation-b", &publication);
    (hub, publication, token)
}
fn receipt(publication: &Publication, baseline: Option<&str>) -> Value {
    json!({"owner":"667891ca-e20a-459e-9d3c-c0e1c30a2bb0","desired":{"schemaVersion":1,"assignments":[{"instanceId":"panel-a","companionId":"installation-b","publicationHash":publication.publication_hash,"capabilityFingerprint":publication.capability_fingerprint}]},"baseline":{"schemaVersion":1,"assignments":[{"instanceId":"panel-a","companionId":baseline,"publicationHash":baseline.map(|_|&publication.publication_hash),"capabilityFingerprint":baseline.map(|_|&publication.capability_fingerprint)}]},"taken":true,"restored":false})
}
fn armed(h: &Hub, id: &str, token: &str, p: &Publication) {
    wire(h, id, json!({"type":"auth","token":token}));
    let open = wire(h, id, json!({"type":"open","instanceId":id}));
    let values = p
        .controls
        .iter()
        .map(|control| (control.id.clone(), control.fallback()))
        .collect::<std::collections::BTreeMap<_, _>>();
    wire(
        h,
        id,
        json!({"version":1,"type":"baseline","clientId":open["clientId"],"sessionId":open["sessionId"],"generation":open["generation"],"publicationHash":open["publicationHash"],"sequence":"0","values":values}),
    );
}
#[test]
fn restore_is_idempotent_and_only_disarms_the_captured_changed_instance() {
    let (h, p, token) = setup();
    assign(&h, "panel-b", "installation-a", &p);
    armed(&h, "panel-b", &token, &p);
    let journal = receipt(&p, Some("installation-a"));
    let restored = restore(&h, &journal, true).unwrap();
    assert_eq!(restored["assignments"][0]["companionId"], "installation-a");
    assert_eq!(restore(&h, &journal, true).unwrap(), restored);
    let status = h.command(&json!({"command":"companion_status"})).unwrap();
    assert_eq!(
        status["instances"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["instanceId"] == "panel-b")
            .unwrap()["armed"],
        true
    );
    assert_eq!(abandon(&h, &journal).unwrap(), json!({"abandoned":true}));
}
#[test]
fn foreign_assignment_and_new_unified_owner_are_preserved() {
    let (h, p, _) = setup();
    let journal = receipt(&p, Some("installation-a"));
    assign(&h, "panel-a", "installation-c", &p);
    assert!(restore(&h, &journal, true)
        .unwrap_err()
        .contains("preserved"));
    assign(&h, "panel-a", "installation-b", &p);
    let proof = h
        .prepare_profile_assignment("panel-a", &p.publication_hash)
        .unwrap();
    let owned = h
        .capture_profile_assignment(&proof, &uuid::Uuid::new_v4().to_string(), None)
        .unwrap();
    h.acquire_profile_assignment(&owned).unwrap();
    assert!(restore(&h, &journal, true)
        .unwrap_err()
        .contains("unified profile"));
    h.validate_profile_assignment_owner(&owned).unwrap();
}
#[test]
fn explicit_absence_baseline_removes_only_the_captured_assignment() {
    let (h, p, _) = setup();
    assign(&h, "panel-b", "installation-c", &p);
    let journal = receipt(&p, None);
    restore(&h, &journal, true).unwrap();
    assert!(
        h.assignment_snapshot(&["panel-a".into()]).unwrap()["assignments"][0]["companionId"]
            .is_null()
    );
    assert_eq!(
        h.assignment_snapshot(&["panel-b".into()]).unwrap()["assignments"][0]["companionId"],
        "installation-c"
    );
}
#[test]
fn malformed_or_finished_receipts_cannot_overwrite_current_authority() {
    let (h, p, _) = setup();
    let original = receipt(&p, Some("installation-a"));
    for (key, value) in [
        ("owner", json!("forged")),
        ("desired", Value::Null),
        ("baseline", json!({})),
        ("taken", Value::Null),
    ] {
        let mut journal = original.clone();
        journal[key] = value;
        assert!(restore(&h, &journal, true).is_err());
        assert!(abandon(&h, &journal).is_err());
    }
    for (key, value) in [("taken", false), ("restored", true)] {
        let mut journal = original.clone();
        journal[key] = json!(value);
        assert!(restore(&h, &journal, true).is_err());
    }
    assert_eq!(
        h.assignment_snapshot(&["panel-a".into()]).unwrap()["assignments"][0]["companionId"],
        "installation-b"
    );
}
