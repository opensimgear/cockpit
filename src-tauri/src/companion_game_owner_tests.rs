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
fn assign(h: &Hub, id: &str, companion: &str, hash: &str) {
    h.command(&json!({"command":"companion_assign","instanceId":id,"companionId":companion,"publicationHash":hash})).unwrap();
}
fn setup() -> (Hub, Publication, String) {
    let h = Hub::new("game-host".into(), Arc::new(NullSourceSink));
    let token = paired(&h, "installation-a");
    paired(&h, "installation-b");
    paired(&h, "installation-c");
    let upload: PublicationUpload = serde_json::from_str(include_str!(
        "../../crates/companion-service/tests/fixtures/c1-flight-publication.json"
    ))
    .unwrap();
    let publication = Publication::validate(upload.clone()).unwrap();
    let mut request = serde_json::to_value(upload).unwrap();
    request["command"] = json!("companion_publish");
    h.command(&request).unwrap();
    assign(
        &h,
        "panel-a",
        "installation-a",
        &publication.publication_hash,
    );
    (h, publication, token)
}
fn desired(h: &Hub) -> Value {
    let mut c = capture(h, &json!({"instanceIds":["panel-a"]})).unwrap();
    c["assignments"][0]["companionId"] = json!("installation-b");
    c
}
fn armed(h: &Hub, id: &str, token: &str, p: &Publication) {
    wire(h, id, json!({"type":"auth","token":token}));
    let open = wire(h, id, json!({"type":"open","instanceId":id}));
    let values = p
        .controls
        .iter()
        .map(|c| (c.id.clone(), c.fallback()))
        .collect::<std::collections::BTreeMap<_, _>>();
    wire(
        h,
        id,
        json!({"version":1,"type":"baseline","clientId":open["clientId"],"sessionId":open["sessionId"],"generation":open["generation"],"publicationHash":open["publicationHash"],"sequence":"0","values":values}),
    );
}
#[test]
fn capture_and_preflight_are_read_only_and_require_complete_selected_assignments() {
    let (h, _, _) = setup();
    let before = h.command(&json!({"command":"companion_status"})).unwrap();
    let c = capture(&h, &json!({})).unwrap();
    assert_eq!(c["assignments"].as_array().unwrap().len(), 1);
    let snapshot = snapshot(&h, &c).unwrap();
    assert_eq!(snapshot["taken"], false);
    assert_eq!(snapshot["restored"], false);
    assert_eq!(
        h.command(&json!({"command":"companion_status"})).unwrap(),
        before
    );
    for payload in [
        json!({"instanceIds":[]}),
        json!({"instanceIds":"panel-a"}),
        json!({"instanceIds":["panel-a","panel-a"]}),
        json!({"instanceIds":["bad/id"]}),
        json!({"instanceIds":["missing"]}),
        json!({"instanceIds":vec!["panel-a";9]}),
    ] {
        assert!(capture(&h, &payload).is_err());
    }
    let empty = Hub::new("empty".into(), Arc::new(NullSourceSink));
    assert!(capture(&empty, &json!({})).is_err());
}
#[test]
fn apply_and_restore_are_idempotent_and_release_only_the_changed_instance() {
    let (h, p, token) = setup();
    assign(&h, "panel-b", "installation-a", &p.publication_hash);
    armed(&h, "panel-a", &token, &p);
    armed(&h, "panel-b", &token, &p);
    let mut journal = snapshot(&h, &desired(&h)).unwrap();
    assert!(take(&h, &journal).is_err());
    journal["taken"] = json!(true);
    let result = take(&h, &journal).unwrap();
    assert_eq!(take(&h, &journal).unwrap(), result);
    let status = h.command(&json!({"command":"companion_status"})).unwrap();
    let instances = status["instances"].as_array().unwrap();
    assert_eq!(
        instances
            .iter()
            .find(|a| a["instanceId"] == "panel-a")
            .unwrap()["armed"],
        false
    );
    assert_eq!(
        instances
            .iter()
            .find(|a| a["instanceId"] == "panel-b")
            .unwrap()["armed"],
        true
    );
    let restored = restore(&h, &journal, false).unwrap();
    assert_eq!(restore(&h, &journal, true).unwrap(), restored);
    assert_eq!(restored["assignments"][0]["companionId"], "installation-a");
    assert!(abandon(&h, &journal).is_ok());
    journal["restored"] = json!(true);
    assert!(take(&h, &journal).is_err());
}
#[test]
fn external_changes_are_preserved_before_apply_and_during_recovery() {
    let (h, p, _) = setup();
    let mut journal = snapshot(&h, &desired(&h)).unwrap();
    journal["taken"] = json!(true);
    assign(&h, "panel-a", "installation-c", &p.publication_hash);
    assert!(take(&h, &journal).unwrap_err().contains("changed"));
    assert!(restore(&h, &journal, true)
        .unwrap_err()
        .contains("preserved"));
    assert_eq!(
        capture(&h, &json!({"instanceIds":["panel-a"]})).unwrap()["assignments"][0]["companionId"],
        "installation-c"
    );
    assign(&h, "panel-a", "installation-a", &p.publication_hash);
    take(&h, &journal).unwrap();
    assign(&h, "panel-a", "installation-c", &p.publication_hash);
    assert!(restore(&h, &journal, true).is_err());
    assert_eq!(
        capture(&h, &json!({"instanceIds":["panel-a"]})).unwrap()["assignments"][0]["companionId"],
        "installation-c"
    );
}
#[test]
fn absent_baseline_is_explicitly_compared_and_new_assignment_is_removed_on_restore() {
    let (h, _, _) = setup();
    let mut c = desired(&h);
    c["assignments"][0]["instanceId"] = json!("new-panel");
    let mut journal = snapshot(&h, &c).unwrap();
    assert!(journal["baseline"]["assignments"][0]["companionId"].is_null());
    journal["taken"] = json!(true);
    take(&h, &journal).unwrap();
    assert!(capture(&h, &json!({"instanceIds":["new-panel"]})).is_ok());
    restore(&h, &journal, true).unwrap();
    assert!(capture(&h, &json!({"instanceIds":["new-panel"]})).is_err());
}
#[test]
fn invalid_credentials_revisions_and_recovery_scope_fail_without_changing_assignments() {
    let (h, _, _) = setup();
    let before = capture(&h, &json!({})).unwrap();
    for (key, v) in [
        ("companionId", json!("unknown")),
        (
            "publicationHash",
            json!(format!("sha256:{}", "c".repeat(64))),
        ),
        (
            "capabilityFingerprint",
            json!(format!("sha256:{}", "c".repeat(64))),
        ),
    ] {
        let mut c = desired(&h);
        c["assignments"][0][key] = v;
        assert!(snapshot(&h, &c).is_err());
    }
    assert_eq!(capture(&h, &json!({})).unwrap(), before);
    let mut journal = snapshot(&h, &desired(&h)).unwrap();
    journal["baseline"]["assignments"][0]["instanceId"] = json!("other");
    assert!(take(&h, &journal).is_err());
    assert!(restore(&h, &journal, true).is_err());
    assert!(abandon(&h, &journal).is_err());
}
#[test]
fn row_order_does_not_create_false_conflicts_and_restored_phase_cannot_overwrite_current_state() {
    let (h, p, _) = setup();
    assign(&h, "panel-b", "installation-a", &p.publication_hash);
    let c = capture(&h, &json!({"instanceIds":["panel-b","panel-a"]})).unwrap();
    let mut journal = snapshot(&h, &c).unwrap();
    journal["baseline"]["assignments"]
        .as_array_mut()
        .unwrap()
        .reverse();
    journal["taken"] = json!(true);
    take(&h, &journal).unwrap();
    journal["restored"] = json!(true);
    assign(&h, "panel-a", "installation-c", &p.publication_hash);
    assert!(restore(&h, &journal, true).is_err());
}
