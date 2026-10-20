use super::*;
use crate::publication::hash;
use base64::Engine;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
#[path = "profile_tests.rs"]
mod profile_tests;
#[path = "operation_tests.rs"]
mod operation_tests;
#[path = "profile_latest_tests.rs"]
mod profile_latest_tests;

#[derive(Default)]
struct Sink {
    defs: Mutex<BTreeMap<String, SourceDefinition>>,
    owners: Mutex<BTreeMap<String, SourceToken>>,
    events: Mutex<Vec<(String, Vec<SourceChange>)>>,
    reject: AtomicBool,
}
impl SourceSink for Sink {
    fn register(&self, d: SourceDefinition) -> Result<(), String> {
        self.defs.lock().unwrap().insert(d.source_id.clone(), d);
        Ok(())
    }
    fn baseline(&self, t: &SourceToken, _: u64, c: &[SourceChange]) -> Result<(), String> {
        self.owners
            .lock()
            .unwrap()
            .insert(t.source_id.clone(), t.clone());
        self.events
            .lock()
            .unwrap()
            .push(("baseline".into(), c.to_vec()));
        Ok(())
    }
    fn changes(&self, t: &SourceToken, _: u64, c: &[SourceChange]) -> Result<(), String> {
        if self.reject.load(Ordering::SeqCst) {
            return Err("Queue overflow".into());
        }
        assert_eq!(self.owners.lock().unwrap().get(&t.source_id), Some(t));
        self.events
            .lock()
            .unwrap()
            .push(("changes".into(), c.to_vec()));
        Ok(())
    }
    fn loss(&self, t: &SourceToken) -> Result<(), String> {
        let mut o = self.owners.lock().unwrap();
        if o.get(&t.source_id) == Some(t) {
            o.remove(&t.source_id);
            self.events.lock().unwrap().push(("loss".into(), vec![]));
        }
        Ok(())
    }
}
#[derive(Default)]
struct Memory {
    value: Mutex<Option<Vec<u8>>>,
    fail: AtomicBool,
}
impl persistence::StateStorage for Memory {
    fn read(&self) -> Result<Option<Vec<u8>>, String> {
        Ok(self.value.lock().unwrap().clone())
    }
    fn write(&self, b: &[u8]) -> Result<(), String> {
        if self.fail.load(Ordering::SeqCst) {
            return Err("Commit failed".into());
        }
        *self.value.lock().unwrap() = Some(b.to_vec());
        Ok(())
    }
}
fn fixture() -> PublicationUpload {
    serde_json::from_str(include_str!("../tests/fixtures/c1-flight-publication.json")).unwrap()
}
fn mutated(mutator: impl FnOnce(&mut Value)) -> PublicationUpload {
    let mut p = fixture();
    let mut d: Value = serde_json::from_str(&p.canonical_document).unwrap();
    mutator(&mut d);
    p.canonical_document =
        String::from_utf8(serde_json_canonicalizer::to_vec(&d).unwrap()).unwrap();
    p.publication_hash = format!("sha256:{}", hash(p.canonical_document.as_bytes()));
    p.capability_fingerprint = None;
    p
}
fn wire(h: &Hub, connection: &str, value: Value) -> Value {
    h.handle_wire(value, connection, h.now_ms())
}
fn operator(h: &Hub, value: Value) -> Value {
    h.command(&value).unwrap()
}
fn paired(h: &Hub, companion: &str) -> String {
    let offer = operator(h, json!({"command":"companion_pair_offer"}));
    let secret = offer["secret"].as_str().unwrap();
    let request = wire(
        h,
        "pairing",
        json!({"type":"pair","secret":secret,"companionId":companion,"name":companion}),
    );
    assert_eq!(request["ok"], true, "{request}");
    operator(
        h,
        json!({"command":"companion_pair_approve","requestId":request["requestId"],"companionId":companion}),
    );
    let reply = wire(
        h,
        "pairing",
        json!({"type":"pair-status","secret":secret,"requestId":request["requestId"]}),
    );
    reply["token"].as_str().unwrap().to_owned()
}
fn publish(h: &Hub, p: PublicationUpload) -> Publication {
    let result = Publication::validate(p.clone()).unwrap();
    let mut command = serde_json::to_value(p).unwrap();
    command["command"] = json!("companion_publish");
    operator(h, command);
    result
}
fn assign(h: &Hub, companion: &str, instance: &str, hash: &str) {
    operator(
        h,
        json!({"command":"companion_assign","companionId":companion,"instanceId":instance,"publicationHash":hash}),
    );
}
fn connect(h: &Hub, connection: &str, token: &str, instance: &str) -> Value {
    assert_eq!(
        wire(h, connection, json!({"type":"auth","token":token}))["ok"],
        true
    );
    let open = wire(h, connection, json!({"type":"open","instanceId":instance}));
    assert_eq!(open["ok"], true, "{open}");
    open
}
fn frame(open: &Value, kind: &str, sequence: &str) -> Value {
    json!({"version":1,"type":kind,"clientId":open["clientId"],"sessionId":open["sessionId"],"generation":open["generation"],"publicationHash":open["publicationHash"],"sequence":sequence})
}
fn baseline(h: &Hub, connection: &str, open: &Value, p: &Publication) -> Value {
    let mut f = frame(open, "baseline", "0");
    f["values"] = serde_json::to_value(
        p.controls
            .iter()
            .map(|c| (c.id.clone(), c.fallback()))
            .collect::<BTreeMap<_, _>>(),
    )
    .unwrap();
    wire(h, connection, f)
}
fn input(open: &Value, p: &Publication, sequence: &str) -> Value {
    let c = p
        .controls
        .iter()
        .find(|c| matches!(c.kind, SourceKind::Button))
        .unwrap();
    let mut f = frame(open, "input", sequence);
    f["changes"] = json!([{"controlId":c.id,"value":true},{"controlId":c.id,"value":false}]);
    f
}

#[test]
fn native_publication_matches_actual_c1_canonical_bytes_and_capability_fingerprint() {
    let p = Publication::validate(fixture()).unwrap();
    assert!(p.controls.len() > 4);
    assert!(p
        .definition("instance-a")
        .controls
        .iter()
        .any(|c| matches!(c.kind, SourceKind::Axis { .. })));
    assert!(p.bytes() > 1000);
    let mixed: PublicationUpload = serde_json::from_str(include_str!(
        "../tests/fixtures/c1-mixed-numbers-publication.json"
    ))
    .unwrap();
    assert!(mixed.canonical_document.contains("1e-7"));
    assert!(mixed.canonical_document.contains("0.000001"));
    Publication::validate(mixed).unwrap();
    let mut bad = fixture();
    bad.canonical_document.push(' ');
    assert!(Publication::validate(bad).is_err());
    let mut bad = fixture();
    bad.capability_fingerprint = Some("sha256:".to_owned() + &"0".repeat(64));
    assert!(Publication::validate(bad).is_err());
}
#[test]
fn malformed_publications_and_shape_confusion_fail_before_source_registration() {
    let cases: Vec<Box<dyn FnOnce(&mut Value)>> = vec![
        Box::new(|d| d["credentials"] = json!("secret")),
        Box::new(|d| d["schemaVersion"] = json!(3)),
        Box::new(|d| d["ui"]["controls"][0]["kind"] = json!("unknown")),
        Box::new(|d| d["ui"]["id"] = json!("../path")),
        Box::new(|d| d["presentation"]["widgets"][0]["bounds"]["width"] = json!(2)),
        Box::new(|d| d["presentation"]["widgets"][0]["controlIds"] = json!(["absent"])),
        Box::new(|d| d["ui"]["pages"][0]["items"] = json!([])),
        Box::new(|d| d["bindings"] = json!([])),
        Box::new(|d| d["presentation"]["grid"]["columns"] = json!(0)),
        Box::new(|d| d["ui"]["feedbackWidgets"][0]["kind"] = json!("script")),
        Box::new(|d| {
            d["presentation"]["viewports"][0]["safeInsets"] =
                json!({"top":1,"right":0,"bottom":0,"left":0})
        }),
        Box::new(|d| d["presentation"]["widgets"][0]["groupId"] = json!("unknown")),
        Box::new(|d| d["targets"][0]["buttons"] = json!(0)),
    ];
    for case in cases {
        assert!(Publication::validate(mutated(case)).is_err());
    }
    let mut p = fixture();
    p.assets.push(AssetPayload {
        asset_id: "absent".into(),
        base64: "YQ==".into(),
    });
    assert!(Publication::validate(p).is_err());
}
#[test]
fn native_publication_checks_assets_digests_dimensions_and_static_raster_containers() {
    let bytes=base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jv3cAAAAASUVORK5CYII=").unwrap();
    let mut p = mutated(|d| {
        d["ui"]["assets"] = json!([{"id":"image-a","sha256":hash(&bytes),"sizeBytes":bytes.len(),"mime":"image/png"}])
    });
    p.assets = vec![AssetPayload {
        asset_id: "image-a".into(),
        base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
    }];
    assert!(Publication::validate(p.clone()).is_ok());
    p.assets[0].base64 = "YQ==".into();
    assert!(Publication::validate(p).is_err());
}

#[test]
fn native_publication_matches_designer_viewport_target_and_utf16_text_boundaries() {
    for (left, right, valid) in [(0.25, 0.249, true), (0.25, 0.25, false)] {
        let upload = mutated(|d| {
            d["presentation"]["viewports"][0]["safeInsets"] =
                json!({"top":0,"right":right,"bottom":0,"left":left});
        });
        assert_eq!(Publication::validate(upload).is_ok(), valid);
    }
    assert!(Publication::validate(mutated(|d| {
        d["presentation"]["viewports"][0]["safeInsets"] =
            json!({"top":0.25,"right":0,"bottom":0.25,"left":0});
    }))
    .is_err());
    assert!(Publication::validate(mutated(|d| {
        d["targets"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"empty-target","label":"Empty","axes":[],"buttons":0,"hats":0}));
    }))
    .is_err());
    for (text, valid) in [
        ("\u{1f600}".repeat(60), true),
        ("\u{1f600}".repeat(61), false),
        ("é".repeat(120), true),
        ("line\nbreak".into(), false),
    ] {
        let upload = mutated(|d| d["ui"]["title"] = json!(text));
        assert_eq!(Publication::validate(upload).is_ok(), valid);
        for (kind, key) in [("text", "prefix"), ("numeric", "unit"), ("gauge", "unit")] {
            let upload = mutated(|d| {
                let widget = d["presentation"]["widgets"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|w| w["kind"] == kind)
                    .unwrap();
                widget["config"][key] = json!(text);
            });
            assert_eq!(Publication::validate(upload).is_ok(), valid, "{kind}.{key}");
        }
    }
}
#[test]
fn native_publication_caps_the_complete_decoded_asset_set() {
    let mut bytes = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jv3cAAAAASUVORK5CYII=")
        .unwrap();
    bytes[16..20].copy_from_slice(&4096u32.to_be_bytes());
    bytes[20..24].copy_from_slice(&4096u32.to_be_bytes());
    for (count, valid) in [(2, true), (3, false)] {
        let mut upload = mutated(|d| {
            d["ui"]["assets"] = json!((0..count)
                .map(|i| json!({"id":format!("image-{i}"),"sha256":hash(&bytes),"sizeBytes":bytes.len(),"mime":"image/png"}))
                .collect::<Vec<_>>());
        });
        upload.assets = (0..count)
            .map(|i| AssetPayload {
                asset_id: format!("image-{i}"),
                base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
            })
            .collect();
        assert_eq!(Publication::validate(upload).is_ok(), valid);
    }
}

#[test]
fn pairing_requires_one_use_secret_expiry_and_explicit_host_approval() {
    let h = Hub::new("host".into(), Arc::new(Sink::default()));
    let offer = operator(&h, json!({"command":"companion_pair_offer"}));
    let request = wire(
        &h,
        "c",
        json!({"type":"pair","secret":offer["secret"],"companionId":"peer","name":"Peer"}),
    );
    assert_eq!(request["ok"], true);
    assert_eq!(
        wire(
            &h,
            "c",
            json!({"type":"pair","secret":offer["secret"],"companionId":"peer","name":"Peer"})
        )["ok"],
        false
    );
    let pending = wire(
        &h,
        "c",
        json!({"type":"pair-status","secret":offer["secret"],"requestId":request["requestId"]}),
    );
    assert_eq!(pending["type"], "pair-pending");
    assert!(pending.get("token").is_none());
    assert!(h.command(&json!({"command":"companion_approve","requestId":request["requestId"],"companionId":"forged"})).is_err());
    operator(
        &h,
        json!({"command":"companion_reject","requestId":request["requestId"]}),
    );
    assert_eq!(
        wire(
            &h,
            "c",
            json!({"type":"pair-status","secret":offer["secret"],"requestId":request["requestId"]})
        )["ok"],
        false
    );
    let offer = operator(&h, json!({"command":"companion_pair_offer"}));
    assert_eq!(
        h.handle_wire(
            json!({"type":"pair","secret":offer["secret"],"companionId":"peer","name":"Peer"}),
            "c",
            h.now_ms() + PAIRING_MS
        )["ok"],
        false
    );
}
#[test]
fn two_distinct_panels_and_two_instances_share_a_hub_with_isolated_sources() {
    let sink = Arc::new(Sink::default());
    let h = Hub::new("host".into(), sink.clone());
    let a = paired(&h, "a");
    let b = paired(&h, "b");
    let c = paired(&h, "c");
    let p = publish(&h, fixture());
    let q = publish(
        &h,
        mutated(|d| {
            d["ui"]["id"] = json!("second-ui");
            d["ui"]["title"] = json!("Other panel");
        }),
    );
    assign(&h, "a", "instance-a", &p.publication_hash);
    assign(&h, "b", "instance-b", &p.publication_hash);
    assign(&h, "c", "instance-c", &q.publication_hash);
    let oa = connect(&h, "ca", &a, "instance-a");
    let ob = connect(&h, "cb", &b, "instance-b");
    let oc = connect(&h, "cc", &c, "instance-c");
    for (connection, open, p) in [("ca", &oa, &p), ("cb", &ob, &p), ("cc", &oc, &q)] {
        assert_eq!(baseline(&h, connection, open, p)["ok"], true);
        assert_eq!(wire(&h, connection, input(open, p, "1"))["ok"], true);
    }
    assert_eq!(sink.owners.lock().unwrap().len(), 3);
    h.disconnect_connection("ca", h.now_ms());
    assert_eq!(sink.owners.lock().unwrap().len(), 2);
    assert_eq!(wire(&h, "cb", frame(&ob, "heartbeat", "1"))["ok"], true);
    let status = operator(&h, json!({"command":"companion_status"}));
    assert_eq!(status["publications"].as_array().unwrap().len(), 2);
    assert!(!status.to_string().contains(&a));
}
#[test]
fn source_transport_is_native_adapter_metadata_and_tracks_the_current_owner() {
    let hub = Hub::new("host".into(), Arc::new(Sink::default()));
    let secret = paired(&hub, "panel");
    let publication = publish(&hub, fixture());
    assign(&hub, "panel", "instance", &publication.publication_hash);
    assert_eq!(hub.handle_serial_wire(json!({"type":"auth","token":secret}), "usb-connection", hub.now_ms())["ok"], true);
    let pending=operator(&hub,json!({"command":"companion_status"}));
    assert_eq!(pending["clients"][0]["connected"],true);
    assert_eq!(pending["clients"][0]["transports"],json!([]));
    assert!(pending["instances"][0]["transport"].is_null());
    let opened = hub.handle_serial_wire(json!({"type":"open","instanceId":"instance"}), "usb-connection", hub.now_ms());
    let opened_status=operator(&hub,json!({"command":"companion_status"}));
    assert_eq!(opened_status["clients"][0]["transports"],json!(["usb"]));
    assert_eq!(opened_status["instances"][0]["transport"],"usb");
    assert_eq!(opened_status["instances"][0]["armed"],false);
    let mut initial = frame(&opened, "baseline", "0");
    initial["values"] = serde_json::to_value(publication.controls.iter().map(|control| (control.id.clone(), control.fallback())).collect::<BTreeMap<_,_>>()).unwrap();
    assert_eq!(hub.handle_serial_wire(initial, "usb-connection", hub.now_ms())["ok"], true);
    assert_eq!(hub.source_transport("instance"), Some(SourceTransport::Usb));
    hub.disconnect_connection("usb-connection", hub.now_ms());
    assert_eq!(hub.source_transport("instance"), None);
    let disconnected=operator(&hub,json!({"command":"companion_status"}));
    assert_eq!(disconnected["clients"][0]["transports"],json!([]));
    assert!(disconnected["instances"][0]["transport"].is_null());
    let opened = connect(&hub, "network-connection", &secret, "instance");
    assert_eq!(baseline(&hub, "network-connection", &opened, &publication)["ok"], true);
    assert_eq!(hub.source_transport("instance"), Some(SourceTransport::Network));
    let connected=operator(&hub,json!({"command":"companion_status"}));
    assert_eq!(connected["clients"][0]["transports"],json!(["network"]));
    assert_eq!(connected["instances"][0]["transport"],"network");
}
#[test]
fn source_transport_status_deduplicates_actual_concurrent_sessions_without_kind_inference() {
    let hub=Hub::new("host".into(),Arc::new(Sink::default()));
    let token=paired(&hub,"same-installation");
    let publication=publish(&hub,fixture());
    for instance in ["usb-instance","network-one","network-two"] {assign(&hub,"same-installation",instance,&publication.publication_hash);}
    assert_eq!(hub.handle_serial_wire(json!({"type":"auth","token":token}),"usb",hub.now_ms())["ok"],true);
    assert_eq!(hub.handle_serial_wire(json!({"type":"open","instanceId":"usb-instance"}),"usb",hub.now_ms())["ok"],true);
    connect(&hub,"network-one",&token,"network-one");
    let status=operator(&hub,json!({"command":"companion_status"}));
    assert_eq!(status["clients"][0]["kind"],"desktop");
    assert_eq!(status["clients"][0]["transports"],json!(["network","usb"]));
    assert_eq!(status["instances"].as_array().unwrap().iter().find(|instance|instance["instanceId"]=="usb-instance").unwrap()["transport"],"usb");
    hub.disconnect_connection("network-one",hub.now_ms());
    assert_eq!(operator(&hub,json!({"command":"companion_status"}))["clients"][0]["transports"],json!(["usb"]));
    connect(&hub,"network-two",&token,"network-two");
    assert_eq!(operator(&hub,json!({"command":"companion_status"}))["clients"][0]["transports"],json!(["network","usb"]));
    hub.disconnect_connection("usb",hub.now_ms());
    connect(&hub,"network-one",&token,"network-one");
    assert_eq!(operator(&hub,json!({"command":"companion_status"}))["clients"][0]["transports"],json!(["network"]));
    hub.disconnect_connection("network-two",hub.now_ms());
    assert_eq!(operator(&hub,json!({"command":"companion_status"}))["clients"][0]["transports"],json!(["network"]));
    hub.disconnect_connection("network-one",hub.now_ms());
    assert_eq!(operator(&hub,json!({"command":"companion_status"}))["clients"][0]["transports"],json!([]));
}
#[test]
fn forged_assignments_versions_and_sessions_never_gain_input_authority() {
    let sink = Arc::new(Sink::default());
    let h = Hub::new("host".into(), sink.clone());
    let token = paired(&h, "peer");
    let p = publish(&h, fixture());
    assign(&h, "peer", "instance", &p.publication_hash);
    let open = connect(&h, "conn", &token, "instance");
    assert_eq!(
        wire(&h, "other", json!({"type":"open","instanceId":"instance"}))["ok"],
        false
    );
    assert_eq!(
        wire(&h, "conn", json!({"type":"open","instanceId":"forged"}))["ok"],
        false
    );
    let mut f = input(&open, &p, "1");
    f["version"] = json!(2);
    assert_eq!(wire(&h, "conn", f)["ok"], false);
    assert_eq!(baseline(&h, "conn", &open, &p)["ok"], true);
    let mut f = input(&open, &p, "1");
    f["sessionId"] = json!("stale");
    assert_eq!(wire(&h, "conn", f)["ok"], false);
    assert_eq!(wire(&h, "conn", input(&open, &p, "1"))["ok"], true);
    assert_eq!(wire(&h, "conn", input(&open, &p, "1"))["ok"], false);
    assert_eq!(sink.owners.lock().unwrap().len(), 1);
}
#[test]
fn operator_takeover_falls_back_first_and_late_socket_cleanup_cannot_cancel_replacement() {
    let sink = Arc::new(Sink::default());
    let h = Hub::new("host".into(), sink.clone());
    let token = paired(&h, "peer");
    let p = publish(&h, fixture());
    assign(&h, "peer", "instance", &p.publication_hash);
    let old = connect(&h, "old", &token, "instance");
    baseline(&h, "old", &old, &p);
    assert_eq!(
        wire(&h, "new", json!({"type":"auth","token":token}))["ok"],
        true
    );
    assert_eq!(
        wire(
            &h,
            "new",
            json!({"type":"open","instanceId":"instance","takeover":true})
        )["ok"],
        false
    );
    operator(
        &h,
        json!({"command":"companion_takeover","instanceId":"instance"}),
    );
    let new = wire(&h, "new", json!({"type":"open","instanceId":"instance"}));
    assert_eq!(new["ok"], true);
    baseline(&h, "new", &new, &p);
    h.disconnect_connection("old", h.now_ms());
    assert_eq!(
        sink.owners
            .lock()
            .unwrap()
            .get("instance")
            .unwrap()
            .session_id,
        new["sessionId"].as_str().unwrap()
    );
    assert_eq!(wire(&h, "new", input(&new, &p, "1"))["ok"], true);
    let events = sink.events.lock().unwrap();
    assert_eq!(
        events.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(),
        vec!["baseline", "loss", "baseline", "changes"]
    );
}
#[test]
fn gap_overflow_and_malformed_current_frames_invalidate_generation_and_prevent_replay() {
    let sink = Arc::new(Sink::default());
    let h = Hub::new("host".into(), sink.clone());
    let token = paired(&h, "peer");
    let p = publish(&h, fixture());
    assign(&h, "peer", "instance", &p.publication_hash);
    let open = connect(&h, "c", &token, "instance");
    baseline(&h, "c", &open, &p);
    assert_eq!(wire(&h, "c", input(&open, &p, "2"))["ok"], false);
    assert!(sink.owners.lock().unwrap().is_empty());
    assert_eq!(baseline(&h, "c", &open, &p)["ok"], false);
    let mut fresh = open.clone();
    fresh["generation"] = json!("2");
    assert_eq!(baseline(&h, "c", &fresh, &p)["ok"], true);
    assert_eq!(wire(&h, "c", input(&open, &p, "1"))["ok"], false);
    sink.reject.store(true, Ordering::SeqCst);
    assert_eq!(wire(&h, "c", input(&fresh, &p, "1"))["ok"], false);
    assert!(sink.owners.lock().unwrap().is_empty());
    fresh["generation"] = json!("3");
    baseline(&h, "c", &fresh, &p);
    let mut invalid = input(&fresh, &p, "1");
    invalid["changes"][0]["value"] = json!("bad");
    assert_eq!(wire(&h, "c", invalid)["ok"], false);
}
#[test]
fn persistent_authority_survives_restart_but_active_input_never_resumes_and_failed_revoke_is_atomic(
) {
    let memory = Arc::new(Memory::default());
    let sink = Arc::new(Sink::default());
    let h = Hub::with_storage(memory.clone(), sink.clone()).unwrap();
    let host = h.host_id();
    let token = paired(&h, "peer");
    let p = publish(&h, fixture());
    assign(&h, "peer", "instance", &p.publication_hash);
    let disk = String::from_utf8(memory.value.lock().unwrap().clone().unwrap()).unwrap();
    assert!(!disk.contains(&token));
    assert!(!disk.contains("canonicalDocument"));
    let reopened = Hub::with_storage(memory.clone(), Arc::new(Sink::default())).unwrap();
    assert_eq!(reopened.host_id(), host);
    let status = operator(&reopened, json!({"command":"companion_status"}));
    assert_eq!(status["instances"][0]["armed"], false);
    assert!(status["publications"].as_array().unwrap().is_empty());
    assert_eq!(
        wire(&reopened, "c", json!({"type":"auth","token":token}))["ok"],
        true
    );
    assert_eq!(
        wire(
            &reopened,
            "c",
            json!({"type":"open","instanceId":"instance"})
        )["ok"],
        false
    );
    memory.fail.store(true, Ordering::SeqCst);
    assert!(reopened
        .command(&json!({"command":"companion_revoke","companionId":"peer"}))
        .is_err());
    assert_eq!(
        operator(&reopened, json!({"command":"companion_status"}))["clients"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    memory.fail.store(false, Ordering::SeqCst);
    operator(
        &reopened,
        json!({"command":"companion_revoke","companionId":"peer"}),
    );
    assert_eq!(
        wire(&reopened, "new", json!({"type":"auth","token":token}))["ok"],
        false
    );
}
#[test]
fn invalid_saved_authority_is_not_silently_replaced() {
    let memory = Arc::new(Memory::default());
    *memory.value.lock().unwrap() = Some(b"{}".to_vec());
    assert!(Hub::with_storage(memory.clone(), Arc::new(Sink::default())).is_err());
    assert_eq!(*memory.value.lock().unwrap(), Some(b"{}".to_vec()));
}
#[test]
fn quantization_qr_and_unknown_protocol_limits() {
    assert_eq!(quantize_axis(-1., AxisMode::Bipolar, 0.).unwrap(), -32768);
    assert_eq!(quantize_axis(1., AxisMode::Unipolar, 0.).unwrap(), 32767);
    assert_eq!(
        quantize_axis(f64::NAN, AxisMode::Bipolar, 0.5).unwrap(),
        16384
    );
    assert!(quantize_axis(0., AxisMode::Unipolar, -1.).is_err());
    let qr = qrcode_matrix("test").unwrap();
    assert!(qr.len() > 10);
    assert!(qrcode_matrix(&"x".repeat(4097)).is_err());
    let h = Hub::new("host".into(), Arc::new(Sink::default()));
    assert_eq!(
        wire(
            &h,
            "c",
            json!({"type":"gateway_write","command":"anything"})
        )["ok"],
        false
    );
    assert_eq!(
        wire(
            &h,
            "c",
            json!({"type":"auth","token":"x".repeat(FRAME_BYTES)})
        )["ok"],
        false
    );
    assert!(h.command(&json!({"command":"arbitrary"})).is_err());
}

#[test]
fn healthy_heartbeats_preserve_stationary_held_input_and_lease_loss_is_per_instance() {
    let sink = Arc::new(Sink::default());
    let h = Hub::new("host".into(), sink.clone());
    let token = paired(&h, "peer");
    let p = publish(&h, fixture());
    assign(&h, "peer", "a", &p.publication_hash);
    assign(&h, "peer", "b", &p.publication_hash);
    let a = connect(&h, "ca", &token, "a");
    let b = connect(&h, "cb", &token, "b");
    baseline(&h, "ca", &a, &p);
    baseline(&h, "cb", &b, &p);
    let mut held = input(&a, &p, "1");
    held["changes"].as_array_mut().unwrap().truncate(1);
    assert_eq!(wire(&h, "ca", held)["ok"], true);
    let now = h.now_ms();
    assert_eq!(
        h.handle_wire(frame(&a, "heartbeat", "1"), "ca", now + 1000)["ok"],
        true
    );
    h.tick(now + 1500);
    assert_eq!(sink.owners.lock().unwrap().len(), 1);
    assert!(sink.owners.lock().unwrap().contains_key("a"));
    assert_eq!(
        h.handle_wire(frame(&a, "heartbeat", "2"), "ca", now + 999)["ok"],
        true
    ); // Trusted worker samples may arrive out of lock order.
    h.tick(now + 3000);
    assert!(sink.owners.lock().unwrap().is_empty());
}

#[test]
fn rolling_input_rate_cannot_be_reset_by_cancel_and_rebaseline() {
    let sink = Arc::new(Sink::default());
    let h = Hub::new("host".into(), sink.clone());
    let token = paired(&h, "peer");
    let p = publish(&h, fixture());
    assign(&h, "peer", "instance", &p.publication_hash);
    let mut open = connect(&h, "c", &token, "instance");
    baseline(&h, "c", &open, &p);
    for sequence in 1..=120 {
        let reply = wire(&h, "c", input(&open, &p, &sequence.to_string()));
        assert_eq!(reply["ok"], true, "{reply}");
    }
    assert_eq!(wire(&h, "c", input(&open, &p, "121"))["ok"], false);
    assert!(sink.owners.lock().unwrap().is_empty());
    open["generation"] = json!("2");
    assert_eq!(baseline(&h, "c", &open, &p)["ok"], true);
    assert_eq!(wire(&h, "c", input(&open, &p, "1"))["ok"], false);
}

#[test]
fn takeover_reassigns_only_the_chosen_instance_and_keeps_other_installation_sessions() {
    let sink = Arc::new(Sink::default());
    let h = Hub::new("host".into(), sink.clone());
    let a = paired(&h, "a");
    let b = paired(&h, "b");
    let p = publish(&h, fixture());
    assign(&h, "a", "one", &p.publication_hash);
    assign(&h, "b", "two", &p.publication_hash);
    let one = connect(&h, "ca", &a, "one");
    let two = connect(&h, "cb", &b, "two");
    baseline(&h, "ca", &one, &p);
    baseline(&h, "cb", &two, &p);
    operator(
        &h,
        json!({"command":"companion_takeover","instanceId":"one","companionId":"b"}),
    );
    assert!(sink.owners.lock().unwrap().contains_key("two"));
    assert!(!sink.owners.lock().unwrap().contains_key("one"));
    assert_eq!(
        wire(&h, "ca", json!({"type":"open","instanceId":"one"}))["ok"],
        false
    );
    let next = connect(&h, "cb-new", &b, "one");
    assert_eq!(baseline(&h, "cb-new", &next, &p)["ok"], true);
    assert_eq!(wire(&h, "cb", input(&two, &p, "1"))["ok"], true);
    operator(
        &h,
        json!({"command":"companion_disconnect","instanceId":"one","companionId":"b"}),
    );
    assert!(sink.owners.lock().unwrap().contains_key("two"));
    assert!(!sink.owners.lock().unwrap().contains_key("one"));
    operator(&h, json!({"command":"companion_revoke","companionId":"b"}));
    assert!(sink.owners.lock().unwrap().is_empty());
    assert_eq!(wire(&h, "cb", frame(&two, "heartbeat", "1"))["ok"], false);
}

#[test]
fn complete_128_control_baseline_is_atomic_and_not_limited_to_incremental_batch_size() {
    let upload = mutated(|d| {
        let page = d["ui"]["pages"][0]["id"].clone();
        let target = d["targets"][0]["id"].clone();
        let bounds = json!({"x":0,"y":0,"width":0.1,"height":0.1});
        d["ui"]["controls"] = json!((0..128)
            .map(|i| json!({"id":format!("input-{i}"),"kind":"button","label":"Button"}))
            .collect::<Vec<_>>());
        d["ui"]["feedbackWidgets"] = json!([]);
        d["presentation"]["widgets"]=json!((0..128).map(|i|json!({"id":format!("widget-{i}"),"kind":"button","pageId":page,"label":"Button","bounds":bounds,"controlIds":[format!("input-{i}")],"feedbackIds":[],"config":{"buttonMode":"momentary","pulseMs":0,"repeatMs":0}})).collect::<Vec<_>>());
        d["ui"]["pages"][0]["items"] = json!((0..128)
            .map(|i| json!({"kind":"control","refId":format!("input-{i}"),"bounds":bounds}))
            .collect::<Vec<_>>());
        d["targets"] = json!([{"id":target,"label":"Target","axes":[],"buttons":128,"hats":0}]);
        d["bindings"]=json!((0..128).map(|i|json!({"controlId":format!("input-{i}"),"targetId":target,"kind":"button","index":i})).collect::<Vec<_>>());
    });
    let h = Hub::new("host".into(), Arc::new(Sink::default()));
    let token = paired(&h, "peer");
    let p = publish(&h, upload);
    assign(&h, "peer", "instance", &p.publication_hash);
    let open = connect(&h, "c", &token, "instance");
    assert_eq!(baseline(&h, "c", &open, &p)["ok"], true);
    let mut too_many = input(&open, &p, "1");
    too_many["changes"] = json!((0..65)
        .map(|i| json!({"controlId":format!("input-{i}"),"value":true}))
        .collect::<Vec<_>>());
    assert_eq!(wire(&h, "c", too_many)["ok"], false);
}

#[test]
fn stale_cancellation_after_disconnect_cannot_rearm_or_mutate_new_sessions() {
    let h = Hub::new("host".into(), Arc::new(Sink::default()));
    let token = paired(&h, "peer");
    let p = publish(&h, fixture());
    assign(&h, "peer", "instance", &p.publication_hash);
    let old = connect(&h, "c", &token, "instance");
    let mut cancel = frame(&old, "close", "0");
    cancel.as_object_mut().unwrap().remove("sequence");
    assert_eq!(wire(&h, "c", cancel.clone())["ok"], true);
    assert_eq!(baseline(&h, "c", &old, &p)["ok"], false);
    let new = wire(&h, "c", json!({"type":"open","instanceId":"instance"}));
    assert_ne!(old["sessionId"], new["sessionId"]);
    baseline(&h, "c", &new, &p);
    assert_eq!(wire(&h, "c", cancel)["ok"], false);
    assert_eq!(wire(&h, "c", input(&new, &p, "1"))["ok"], true);
}

#[test]
fn malformed_and_exhausted_counters_fail_without_numeric_precision_loss() {
    for value in [
        json!(1),
        json!("01"),
        json!("18446744073709551616"),
        json!("-1"),
        json!("18446744073709551615"),
    ] {
        let h = Hub::new("host".into(), Arc::new(Sink::default()));
        let token = paired(&h, "peer");
        let p = publish(&h, fixture());
        assign(&h, "peer", "instance", &p.publication_hash);
        let open = connect(&h, "c", &token, "instance");
        baseline(&h, "c", &open, &p);
        let mut f = input(&open, &p, "1");
        f["sequence"] = value;
        assert_eq!(wire(&h, "c", f)["ok"], false);
    }
}

#[test]
fn mapping_blueprints_require_current_assignment_authority_and_exact_publication() {
    let memory = Arc::new(Memory::default());
    let hub = Hub::with_storage(memory.clone(), Arc::new(Sink::default())).unwrap();
    assert!(hub.assigned_publication("not-assigned").is_err());
    assert!(hub.assigned_publication("../invalid").is_err());
    paired(&hub, "installation");
    let publication = publish(&hub, fixture());
    assign(
        &hub,
        "installation",
        "instance",
        &publication.publication_hash,
    );
    let blueprint = hub.assigned_publication("instance").unwrap();
    assert_eq!(blueprint.publication_hash, publication.publication_hash);
    assert_eq!(blueprint.canonical_document, publication.canonical_document);
    assert_eq!(
        blueprint.definition("instance"),
        publication.definition("instance")
    );
    assert_eq!(
        operator(&hub, json!({"command":"companion_status"}))["instances"][0]["armed"],
        false
    );

    let reopened = Hub::with_storage(memory.clone(), Arc::new(Sink::default())).unwrap();
    assert!(reopened
        .assigned_publication("instance")
        .unwrap_err()
        .contains("available again"));
    publish(&reopened, fixture());
    assert_eq!(
        reopened
            .assigned_publication("instance")
            .unwrap()
            .publication_hash,
        publication.publication_hash
    );

    let mut saved: Value =
        serde_json::from_slice(memory.value.lock().unwrap().as_ref().unwrap()).unwrap();
    saved["instances"][0]["definition"]["name"] = json!("changed descriptor");
    *memory.value.lock().unwrap() = Some(serde_json::to_vec(&saved).unwrap());
    let changed = Hub::with_storage(memory, Arc::new(Sink::default())).unwrap();
    publish(&changed, fixture());
    assert!(changed
        .assigned_publication("instance")
        .unwrap_err()
        .contains("descriptor changed"));

    operator(
        &hub,
        json!({"command":"companion_revoke","companionId":"installation"}),
    );
    assert!(hub.assigned_publication("instance").is_err());
}

#[path = "assignment_tests.rs"]
mod assignment_cases;
#[path = "hub_feedback_tests.rs"]
mod feedback_cases;

#[test]
fn unassign_releases_only_the_selected_source_and_keeps_pairing() {
    let sink=Arc::new(Sink::default());let h=Hub::new("host".into(),sink.clone());
    let a=paired(&h,"a");let b=paired(&h,"b");let panel=publish(&h,fixture());
    assign(&h,"a","one",&panel.publication_hash);assign(&h,"b","two",&panel.publication_hash);
    let oa=connect(&h,"ca",&a,"one");let ob=connect(&h,"cb",&b,"two");
    assert_eq!(baseline(&h,"ca",&oa,&panel)["ok"],true);assert_eq!(baseline(&h,"cb",&ob,&panel)["ok"],true);
    operator(&h,json!({"command":"companion_unassign","instanceId":"one"}));
    let state=operator(&h,json!({"command":"companion_status"}));
    assert_eq!(state["instances"].as_array().unwrap().len(),1);
    assert_eq!(state["clients"].as_array().unwrap().len(),2);
    assert!(!sink.owners.lock().unwrap().contains_key("one"));
    assert!(sink.owners.lock().unwrap().contains_key("two"));
    assert_eq!(wire(&h,"ca",frame(&oa,"heartbeat","1"))["ok"],false);
    assert_eq!(wire(&h,"cb",frame(&ob,"heartbeat","1"))["ok"],true);
    assert!(h.command(&json!({"command":"companion_unassign","instanceId":"missing"})).is_err());
}

#[test]
fn panel_snapshot_pruning_preserves_live_and_explicitly_pinned_panels() {
    let h=Hub::new("host".into(),Arc::new(Sink::default()));paired(&h,"a");
    let current=publish(&h,fixture());
    let pinned=publish(&h,mutated(|d|d["ui"]["title"]=json!("Captured layout")));
    let unused=publish(&h,mutated(|d|d["ui"]["title"]=json!("Unused layout")));
    assign(&h,"a","one",&current.publication_hash);
    let foreign=publish(&h,mutated(|d|d["ui"]["id"]=json!("foreign-panel")));
    let result=operator(&h,json!({"command":"companion_prune","hashes":[pinned.publication_hash],"panelIds":[current.ui_id]}));
    assert_eq!(result["removed"],1);
    let state=operator(&h,json!({"command":"companion_status"}));
    let hashes=state["publications"].as_array().unwrap().iter().map(|p|p["publicationHash"].as_str().unwrap()).collect::<Vec<_>>();
    assert!(hashes.contains(&current.publication_hash.as_str()));
    assert!(hashes.contains(&pinned.publication_hash.as_str()));
    assert!(!hashes.contains(&unused.publication_hash.as_str()));
    assert!(hashes.contains(&foreign.publication_hash.as_str()));
    assert!(h.command(&json!({"command":"companion_prune","hashes":["invalid"],"panelIds":[current.ui_id]})).is_err());
    assert_eq!(operator(&h,json!({"command":"companion_status"}))["publications"].as_array().unwrap().len(),3);
}


#[test]
fn short_codes_are_bounded_cancelled_expired_and_single_use() {
    use base64::Engine;
    let hub=Hub::new("code-host".into(),Arc::new(NullSourceSink));
    let offer=hub.command(&json!({"command":"companion_pair_offer","autoApprove":true,"kind":"mobile"})).unwrap();
    let code=hub.command(&json!({"command":"companion_pair_code","payload":{"secret":offer["secret"]}})).unwrap();
    assert_eq!(code["code"].as_str().unwrap().len(),8);
    for _ in 0..5 {
        let (_,message)=crate::pairing_code::start("AAAAAAAA");
        assert_eq!(hub.handle_wire(json!({"type":"pair-code","message":base64::engine::general_purpose::STANDARD.encode(message)}),"peer",hub.now_ms())["ok"],true);
    }
    assert_eq!(hub.handle_wire(json!({"type":"pair-code","message":"invalid"}),"peer",hub.now_ms())["ok"],false);
    hub.command(&json!({"command":"companion_pair_cancel"})).unwrap();
    assert_eq!(hub.handle_wire(json!({"type":"pair","secret":offer["secret"],"companionId":"mobile","name":"Phone"}),"peer",hub.now_ms())["ok"],false);
    let offer=hub.command(&json!({"command":"companion_pair_offer","autoApprove":true,"kind":"mobile"})).unwrap();
    let frame=json!({"type":"pair","secret":offer["secret"],"companionId":"mobile","name":"Phone"});
    let pending=hub.handle_wire(frame.clone(),"peer",hub.now_ms());
    assert_eq!(pending["ok"],true);
    assert_eq!(hub.command(&json!({"command":"companion_status"})).unwrap()["clients"][0]["kind"],"mobile");
    assert_eq!(hub.handle_wire(frame,"peer",hub.now_ms())["ok"],false);
    let offer=hub.command(&json!({"command":"companion_pair_offer"})).unwrap();
    hub.command(&json!({"command":"companion_pair_code","payload":{"secret":offer["secret"]}})).unwrap();
    let (_,message)=crate::pairing_code::start("AAAAAAAA");
    assert_eq!(hub.handle_wire(json!({"type":"pair-code","message":base64::engine::general_purpose::STANDARD.encode(message)}),"peer",hub.now_ms()+PAIRING_MS+1)["ok"],false);
}


#[test]
fn editing_companion_name_preserves_identity_credential_and_connection() {
    let hub=Hub::new("settings-host".into(),Arc::new(NullSourceSink));
    let offer=hub.command(&json!({"command":"companion_pair_offer","autoApprove":true})).unwrap();
    let pending=hub.handle_wire(json!({"type":"pair","secret":offer["secret"],"companionId":"settings-screen","name":"Original"}),"settings-peer",hub.now_ms());
    let paired=hub.handle_wire(json!({"type":"pair-status","requestId":pending["requestId"],"secret":offer["secret"]}),"settings-peer",hub.now_ms());
    assert_eq!(hub.handle_wire(json!({"type":"auth","token":paired["token"]}),"settings-peer",hub.now_ms())["ok"],true);
    let updated=hub.command(&json!({"command":"companion_update","companionId":"settings-screen","name":"Renamed"})).unwrap();
    assert_eq!(updated["clients"][0]["name"],"Renamed");
    assert_eq!(updated["clients"][0]["companionId"],"settings-screen");
    assert_eq!(updated["clients"][0]["connected"],true);
    assert_eq!(hub.handle_wire(json!({"type":"presence"}),"settings-peer",hub.now_ms())["ok"],true);
    assert!(hub.command(&json!({"command":"companion_update","companionId":"unknown","name":"Other"})).is_err());
    assert!(hub.command(&json!({"command":"companion_update","companionId":"settings-screen","name":""})).is_err());
    assert_eq!(hub.command(&json!({"command":"companion_status"})).unwrap()["clients"][0]["name"],"Renamed");
}

#[test]
fn browser_pairing_credential_survives_host_reload_and_is_revoked() {
    let storage=Arc::new(Memory::default());
    let sink=Arc::new(Sink::default());
    let hub=Hub::with_storage(storage.clone(),sink.clone()).unwrap();
    let offer=operator(&hub,json!({"command":"companion_pair_offer","kind":"browser","autoApprove":true}));
    let pending=wire(&hub,"pair-link",json!({"type":"pair","secret":offer["secret"],"companionId":"browser-one","name":"Browser companion"}));
    let credential=wire(&hub,"pair-link",json!({"type":"pair-status","secret":offer["secret"],"requestId":pending["requestId"]}));
    let token=credential["token"].clone();assert!(token.is_string());
    drop(hub);
    let hub=Hub::with_storage(storage,sink).unwrap();
    let status=operator(&hub,json!({"command":"companion_status"}));
    assert_eq!(status["clients"][0]["kind"],"browser");
    assert_eq!(status["clients"][0]["connected"],false);
    assert_eq!(wire(&hub,"new-window",json!({"type":"auth","token":token}))["ok"],true);
    operator(&hub,json!({"command":"companion_revoke","companionId":"browser-one"}));
    assert_eq!(wire(&hub,"reopened-app",json!({"type":"auth","token":token}))["ok"],false);
}


#[test]
fn browser_pairing_replacement_revokes_only_the_proven_old_browser(){
    let storage=Arc::new(Memory::default());let sink=Arc::new(Sink::default());
    let hub=Hub::with_storage(storage.clone(),sink.clone()).unwrap();
    let offer=operator(&hub,json!({"command":"companion_pair_offer","kind":"browser","autoApprove":true}));
    let pending=wire(&hub,"old-link",json!({"type":"pair","secret":offer["secret"],"companionId":"old-browser","name":"Old browser"}));
    let old=wire(&hub,"old-link",json!({"type":"pair-status","requestId":pending["requestId"],"secret":offer["secret"]}));
    let offer=operator(&hub,json!({"command":"companion_pair_offer","kind":"browser","autoApprove":true}));
    let invalid=wire(&hub,"bad-link",json!({"type":"pair","secret":"bad","companionId":"new-browser","name":"New browser","replaceToken":old["token"]}));
    assert_eq!(invalid["ok"],false);
    assert_eq!(wire(&hub,"old-session",json!({"type":"auth","token":old["token"]}))["ok"],true);
    let pending=wire(&hub,"new-link",json!({"type":"pair","secret":offer["secret"],"companionId":"new-browser","name":"New browser","replaceToken":old["token"]}));
    assert_eq!(pending["ok"],true);
    let fresh=wire(&hub,"new-link",json!({"type":"pair-status","requestId":pending["requestId"],"secret":offer["secret"]}));
    assert_eq!(wire(&hub,"revoked-old",json!({"type":"auth","token":old["token"]}))["ok"],false);
    assert_eq!(wire(&hub,"fresh-session",json!({"type":"auth","token":fresh["token"]}))["ok"],true);
    let state=operator(&hub,json!({"command":"companion_status"}));
    assert_eq!(state["clients"].as_array().unwrap().len(),1);
    assert_eq!(state["clients"][0]["companionId"],"new-browser");
    drop(hub);
    let hub=Hub::with_storage(storage,sink).unwrap();
    assert_eq!(wire(&hub,"restart-old",json!({"type":"auth","token":old["token"]}))["ok"],false);
    assert_eq!(wire(&hub,"restart-new",json!({"type":"auth","token":fresh["token"]}))["ok"],true);
}

#[test]
fn panel_error_keeps_connection_alive_without_arming_inputs(){
    let h=Hub::new("host".into(),Arc::new(Sink::default()));
    let token=paired(&h,"screen");let p=publish(&h,fixture());assign(&h,"screen","panel",&p.publication_hash);
    let opened=connect(&h,"connection",&token,"panel");let mut heartbeat=frame(&opened,"heartbeat","1");heartbeat["panelError"]=json!(true);
    assert_eq!(wire(&h,"connection",heartbeat)["ok"],true);
    let status=h.command(&json!({"command":"companion_status"})).unwrap();
    assert_eq!(status["clients"][0]["connected"],true);assert!(status["clients"][0]["panelError"].is_string());
    assert_eq!(baseline(&h,"connection",&opened,&p)["ok"],false);
}

#[test]
fn device_images_are_external_sized_and_bound_to_the_authenticated_session(){
    let bitmap=image::RgbaImage::from_pixel(4,4,image::Rgba([0,255,0,255]));let mut output=std::io::Cursor::new(Vec::new());bitmap.write_to(&mut output,image::ImageFormat::Png).unwrap();let bytes=output.into_inner();
    let mut upload=mutated(|doc|{
        doc["ui"]["assets"]=json!([{"id":"picture","mime":"image/png","sha256":hash(&bytes),"sizeBytes":bytes.len()}]);
        let page=doc["ui"]["pages"][0]["id"].clone();
        doc["presentation"]["widgets"].as_array_mut().unwrap().push(json!({"id":"picture-widget","kind":"image","pageId":page,"label":"Image","bounds":{"x":0,"y":0,"width":1,"height":1},"controlIds":[],"feedbackIds":[],"config":{"assetId":"picture"}}));
    });
    upload.assets=vec![AssetPayload{asset_id:"picture".into(),base64:base64::engine::general_purpose::STANDARD.encode(bytes)}];
    let sink=Arc::new(Sink::default());let h=Hub::new("host".into(),sink);let token=paired(&h,"screen");let p=publish(&h,upload);assign(&h,"screen","instance",&p.publication_hash);
    assert_eq!(wire(&h,"screen-link",json!({"type":"auth","token":token}))["ok"],true);
    let open=wire(&h,"screen-link",json!({"type":"open","instanceId":"instance","externalImages":true}));assert_eq!(open["ok"],true,"{open}");assert_eq!(open["publication"]["assets"],json!([]));
    let mut request=frame(&open,"image-chunk","0");request.as_object_mut().unwrap().remove("sequence");request["widgetId"]=json!("picture-widget");request["width"]=json!(2);request["height"]=json!(3);request["offset"]=json!(0);
    let reply=wire(&h,"screen-link",request.clone());assert_eq!(reply["ok"],true,"{reply}");assert_eq!(reply["total"],18);
    assert_eq!(base64::engine::general_purpose::STANDARD.decode(reply["base64"].as_str().unwrap()).unwrap(),vec![224,7,255].repeat(6));
    assert_eq!(wire(&h,"other-link",request.clone())["ok"],false);
    request["generation"]=json!("2");assert_eq!(wire(&h,"screen-link",request)["ok"],false);
}
