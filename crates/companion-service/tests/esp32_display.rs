//! ESP32's shared publication and display-only wire sequence against the real Hub.
use osg_companion_service::{Hub, SourceChange, SourceDefinition, SourceSink, SourceToken, FeedbackBinding, FeedbackSample, FeedbackValue, FeedbackStatus, FeedbackProvenance, FeedbackSource, FeedbackRange, FeedbackMode};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
#[derive(Default)]
struct Recorder(Mutex<Vec<&'static str>>);
impl SourceSink for Recorder {
    fn register(&self, _: SourceDefinition) -> Result<(), String> { Ok(()) }
    fn baseline(&self, _: &SourceToken, _: u64, _: &[SourceChange]) -> Result<(), String> { self.0.lock().unwrap().push("baseline"); Ok(()) }
    fn changes(&self, _: &SourceToken, _: u64, _: &[SourceChange]) -> Result<(), String> { self.0.lock().unwrap().push("input"); Ok(()) }
    fn loss(&self, _: &SourceToken) -> Result<(), String> { Ok(()) }
}
#[test]
fn esp32_display_publication_pairing_feedback_and_revoke_without_input() {
    let fixture: Value = serde_json::from_str(include_str!("../../../apps/companion-esp32/test/live-publication.json")).unwrap();
    let sink = Arc::new(Recorder::default());
    let hub = Hub::new("embedded-host".into(), sink.clone());
    let hash = fixture["publicationHash"].clone();
    hub.command(&json!({"command":"companion_publish","canonicalDocument":fixture["canonicalDocument"],"publicationHash":hash,"capabilityFingerprint":fixture["capabilityFingerprint"],"assets":[]})).unwrap();
    let offer = hub.command(&json!({"command":"companion_pair_offer"})).unwrap();
    let pending = hub.handle_wire(json!({"type":"pair","secret":offer["secret"],"companionId":"esp32-screen","name":"Generic ESP32 screen"}), "embedded-connection", hub.now_ms());
    assert_eq!(pending["type"], "pair-pending");
    hub.command(&json!({"command":"companion_pair_approve","requestId":pending["requestId"]})).unwrap();
    let paired = hub.handle_wire(json!({"type":"pair-status","secret":offer["secret"],"requestId":pending["requestId"]}), "embedded-connection", hub.now_ms());
    assert_eq!(paired["type"], "paired");
    hub.command(&json!({"command":"companion_assign","instanceId":"embedded-panel","companionId":"esp32-screen","publicationHash":hash})).unwrap();
    let auth = hub.handle_wire(json!({"type":"auth","token":paired["token"]}), "embedded-connection", hub.now_ms());
    assert_eq!(auth["type"], "authenticated");
    let open = hub.handle_wire(json!({"type":"open","instanceId":"embedded-panel","publicationHash":hash,"takeover":false}), "embedded-connection", hub.now_ms());
    assert_eq!(open["type"], "session-open");
    assert_eq!(open["publication"]["canonicalDocument"], fixture["canonicalDocument"]);
    assert_eq!(open["publication"]["publicationHash"], hash);
    hub.configure_feedback("embedded-panel", hash.as_str().unwrap(), &[FeedbackBinding { channel_id: "speed-channel".into(), output_id: "speed".into() }]).unwrap();
    let feedback_token = hub.feedback_token("embedded-panel").unwrap();
    hub.publish_feedback(&feedback_token, &[FeedbackSample { channel_id: "speed-channel".into(), output_id: "speed".into(), value: Some(FeedbackValue::Number(125.5)), status: FeedbackStatus::Fresh, provenance: FeedbackProvenance::Local, source_kind: FeedbackSource::Host, source_age_ms: 0, unit: "kt".into(), range: Some(FeedbackRange { min: 0., max: 600. }) }], FeedbackMode::Snapshot, hub.now_ms()).unwrap();
    let heartbeat = json!({"version":1,"type":"heartbeat","clientId":open["clientId"],"sessionId":open["sessionId"],"generation":open["generation"],"publicationHash":hash,"sequence":"1"});
    let ack = hub.handle_wire(heartbeat.clone(), "embedded-connection", hub.now_ms());
    assert_eq!(ack["type"], "ack");
    assert_eq!(ack["feedback"]["values"][0]["value"], 125.5);
    assert_eq!(ack["ackStage"], "source-accepted");
    assert_eq!(ack["submitted"], false);
    assert!(sink.0.lock().unwrap().is_empty(), "display-only clients never send a baseline/input");
    hub.command(&json!({"command":"companion_revoke","companionId":"esp32-screen"})).unwrap();
    assert_eq!(hub.handle_wire(heartbeat, "embedded-connection", hub.now_ms())["ok"], false);
}

#[derive(Default)]
struct LiveRecorder {
    values: Mutex<std::collections::BTreeMap<String, bool>>,
    edges: Mutex<Vec<(String, bool)>>,
}
impl SourceSink for LiveRecorder {
    fn register(&self, _: SourceDefinition) -> Result<(), String> { Ok(()) }
    fn baseline(&self, token: &SourceToken, sequence: u64, changes: &[SourceChange]) -> Result<(), String> {
        assert_eq!(sequence, 0);
        assert_eq!(changes.len(), 1);
        assert_eq!(serde_json::to_value(&changes[0].value).unwrap(), false);
        self.values.lock().unwrap().insert(token.source_id.clone(), false);
        Ok(())
    }
    fn changes(&self, token: &SourceToken, _: u64, changes: &[SourceChange]) -> Result<(), String> {
        for change in changes {
            assert_eq!(change.control_id, "master");
            let down = serde_json::to_value(&change.value).unwrap().as_bool().unwrap();
            self.values.lock().unwrap().insert(token.source_id.clone(), down);
            self.edges.lock().unwrap().push((token.source_id.clone(), down));
        }
        Ok(())
    }
    fn loss(&self, token: &SourceToken) -> Result<(), String> {
        self.values.lock().unwrap().insert(token.source_id.clone(), false);
        Ok(())
    }
}
fn pair_live(hub: &Hub, device: &str, connection: &str, instance: &str, hash: &Value) -> (Value, Value) {
    let offer = hub.command(&json!({"command":"companion_pair_offer"})).unwrap();
    let pending = hub.handle_wire(json!({"type":"pair","secret":offer["secret"],"companionId":device,"name":"ESP32 touch"}), connection, hub.now_ms());
    hub.command(&json!({"command":"companion_pair_approve","requestId":pending["requestId"]})).unwrap();
    let paired = hub.handle_wire(json!({"type":"pair-status","secret":offer["secret"],"requestId":pending["requestId"]}), connection, hub.now_ms());
    assert_eq!(paired["type"], "paired");
    hub.command(&json!({"command":"companion_assign","instanceId":instance,"companionId":device,"publicationHash":hash})).unwrap();
    assert_eq!(hub.handle_wire(json!({"type":"auth","token":paired["token"]}), connection, hub.now_ms())["ok"], true);
    let open = hub.handle_wire(json!({"type":"open","instanceId":instance,"publicationHash":hash,"takeover":false}), connection, hub.now_ms());
    assert_eq!(open["type"], "session-open");
    (paired["token"].clone(), open)
}
fn live_frame(open: &Value, kind: &str, sequence: &str) -> Value {
    let mut frame = json!({"version":1,"type":kind,"clientId":open["clientId"],"sessionId":open["sessionId"],"generation":open["generation"],"publicationHash":open["publicationHash"]});
    if kind != "close" { frame["sequence"] = json!(sequence); }
    if kind == "baseline" { frame["values"] = json!({"master":false}); }
    frame
}
#[test]
fn esp32_live_touch_baseline_ordering_isolated_lease_and_fresh_reconnect() {
    let fixture: Value = serde_json::from_str(include_str!("../../../apps/companion-esp32/test/live-publication.json")).unwrap();
    let sink = Arc::new(LiveRecorder::default());
    let hub = Hub::new("live-host".into(), sink.clone());
    let hash = fixture["publicationHash"].clone();
    hub.command(&json!({"command":"companion_publish","canonicalDocument":fixture["canonicalDocument"],"publicationHash":hash,"capabilityFingerprint":fixture["capabilityFingerprint"],"assets":[]})).unwrap();
    let (token, first) = pair_live(&hub, "first-screen", "first-connection", "first-panel", &hash);
    let (_, second) = pair_live(&hub, "second-screen", "second-connection", "second-panel", &hash);
    let base = hub.now_ms();
    let mut press = live_frame(&first, "input", "1");
    press["changes"] = json!([{"controlId":"master","value":true}]);
    assert_eq!(hub.handle_wire(press.clone(), "first-connection", base)["ok"], false);
    assert!(sink.edges.lock().unwrap().is_empty());
    let mut non_neutral = live_frame(&first, "baseline", "0");
    non_neutral["values"]["master"] = json!(true);
    assert_eq!(hub.handle_wire(non_neutral, "first-connection", base)["ok"], false);
    assert_eq!(hub.handle_wire(live_frame(&first, "baseline", "0"), "first-connection", base)["sequence"], "0");
    assert_eq!(hub.handle_wire(live_frame(&second, "baseline", "0"), "second-connection", base)["ok"], true);
    assert_eq!(hub.handle_wire(press.clone(), "first-connection", base + 1)["ok"], true);
    assert_eq!(hub.handle_wire(live_frame(&first, "baseline", "0"), "first-connection", base + 1)["ok"], false);
    let mut release = live_frame(&first, "input", "2");
    release["changes"] = json!([{"controlId":"master","value":false},{"controlId":"master","value":true}]);
    assert_eq!(hub.handle_wire(release, "first-connection", base + 2)["ok"], true);
    assert_eq!(sink.edges.lock().unwrap().as_slice(), &[("first-panel".to_string(), true),("first-panel".to_string(), false),("first-panel".to_string(), true)]);
    let mut other = live_frame(&second, "input", "1");
    other["changes"] = json!([{"controlId":"master","value":true}]);
    assert_eq!(hub.handle_wire(other, "second-connection", base + 2)["ok"], true);
    assert_eq!(hub.handle_wire(live_frame(&first, "heartbeat", "1"), "first-connection", base + 1200)["ok"], true);
    assert_eq!(hub.handle_wire(live_frame(&second, "heartbeat", "1"), "second-connection", base + 1200)["ok"], true);
    assert_eq!(hub.handle_wire(live_frame(&second, "heartbeat", "2"), "second-connection", base + 2000)["ok"], true);
    hub.tick(base + 2701);
    assert_eq!(sink.values.lock().unwrap().get("first-panel"), Some(&false));
    assert_eq!(sink.values.lock().unwrap().get("second-panel"), Some(&true));
    assert_eq!(hub.handle_wire(press.clone(), "first-connection", base + 2701)["ok"], false);
    hub.disconnect_connection("first-connection", base + 2701);
    assert_eq!(hub.handle_wire(json!({"type":"auth","token":token}), "replacement", base + 2701)["ok"], true);
    let fresh = hub.handle_wire(json!({"type":"open","instanceId":"first-panel","publicationHash":hash,"takeover":false}), "replacement", base + 2701);
    assert_eq!(fresh["type"], "session-open");
    assert_ne!(fresh["sessionId"], first["sessionId"]);
    assert_eq!(hub.handle_wire(press, "replacement", base + 2701)["ok"], false);
    let mut fresh_press = live_frame(&fresh, "input", "1");
    fresh_press["changes"] = json!([{"controlId":"master","value":true}]);
    assert_eq!(hub.handle_wire(fresh_press.clone(), "replacement", base + 2701)["ok"], false);
    assert_eq!(hub.handle_wire(live_frame(&fresh, "baseline", "0"), "replacement", base + 2701)["ok"], true);
    assert_eq!(hub.handle_wire(fresh_press, "replacement", base + 2701)["ok"], true);
    hub.disconnect_connection("replacement", base + 2702); // Lost ACK/socket: host fallback releases only this source.
    assert_eq!(sink.values.lock().unwrap().get("first-panel"), Some(&false));
    assert_eq!(sink.values.lock().unwrap().get("second-panel"), Some(&true));
    assert_eq!(hub.handle_wire(live_frame(&second, "close", ""), "second-connection", base + 2702)["type"], "closed");
    assert_eq!(sink.values.lock().unwrap().get("second-panel"), Some(&false));
}
