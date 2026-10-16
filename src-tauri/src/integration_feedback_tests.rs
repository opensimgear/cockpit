use super::*;
fn source_only() -> Value {
    let mut c = config();
    c["document"]["schemaVersion"] = json!(2);
    c["document"]["roles"] = json!([]);
    c["document"]["outputs"] = json!([]);
    c["document"]["feedbackOutputs"] = json!([
        {"id":"visual-rpm","channel":"rpm"},{"id":"visual-gear","channel":"gear"}]);
    c["assignments"] = json!([]);
    c["connection"]["freshnessMs"] = json!(100);
    c
}
fn no_device_effects(f: &Fixture) {
    let fake = f.mock.fake.lock().unwrap();
    assert!(fake.calls.is_empty(), "{:?}", fake.calls);
    assert!(fake.writes.is_empty());
    assert!(fake.devices.values().all(|d| d.leases.is_empty()));
}
#[test]
fn source_only_synthetic_feedback_is_typed_fresh_then_stale_and_never_touches_gateways() {
    let f = Fixture::new();
    let c = source_only();
    f.request(
        "integration_preflight",
        json!({"integrationConfig":c,"gameDevices":[]}),
    )
    .unwrap();
    assert_eq!(f.runtime.status()["validation"]["valid"], true);
    no_device_effects(&f);
    f.save(c.clone());
    assert_eq!(f.runtime.feedback_snapshot()["sourceKind"], "synthetic");
    assert_eq!(f.runtime.feedback_snapshot()["running"], false);
    assert_eq!(f.runtime.feedback_snapshot()["values"], json!({}));
    f.start();
    f.fresh(1, 6300);
    wait("named raw feedback", || {
        f.runtime.feedback_snapshot()["values"]["visual-rpm"] == 6300
    });
    let sample = f.runtime.feedback_snapshot();
    assert_eq!(sample["sourceKind"], "synthetic");
    assert_eq!(sample["fresh"], true);
    assert!(sample["values"]["visual-gear"].is_number());
    assert_eq!(
        sample["revision"],
        feedback_revision(&integration_profile::validate(&c).unwrap())
    );
    wait("source becomes stale", || {
        f.runtime.feedback_snapshot()["fresh"] == false
    });
    assert_eq!(f.runtime.feedback_snapshot()["sourceKind"], "synthetic");
    f.request("integration_stop", json!({})).unwrap();
    let stopped = f.runtime.feedback_snapshot();
    assert_eq!(stopped["running"], false);
    assert_eq!(stopped["fresh"], false);
    assert_eq!(stopped["values"], json!({}));
    no_device_effects(&f);
}
#[test]
fn source_revision_pins_semantics_and_ignores_transient_process_identity() {
    let c = source_only();
    let parsed = integration_profile::validate(&c).unwrap();
    let stable = feedback_revision(&parsed);
    let mut changed = parsed.clone();
    changed.connection["processId"] = json!(42);
    changed.connection["processIdentity"] = json!({"pid":42,"path":"/runtime/game","epoch":9});
    assert_eq!(feedback_revision(&changed), stable);
    changed.connection["processId"] = json!(99);
    changed.connection["processIdentity"]["epoch"] = json!(10);
    assert_eq!(feedback_revision(&changed), stable);
    changed.document["feedbackOutputs"][0]["channel"] = json!("gear");
    assert_ne!(feedback_revision(&changed), stable);
    changed = parsed.clone();
    changed.connection["source"] = json!("shared-memory");
    assert_ne!(feedback_revision(&changed), stable);
    changed = parsed;
    changed.document["id"] = json!("different-provider-profile");
    assert_ne!(feedback_revision(&changed), stable);
}
struct CatalogueFixture(PathBuf);
impl CatalogueFixture {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.research/feedback-source-tests")
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&path).unwrap();
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dcs-bios");
        for name in [
            "A-10C.json",
            "MetadataStart.json",
            "MetadataEnd.json",
            "AircraftAliases.json",
        ] {
            fs::copy(source.join(name), path.join(name)).unwrap();
        }
        let this = Self(path);
        this.edit(|doc|doc["RawFeedback"]=json!({
            "RAW_TEXT":{"identifier":"RAW_TEXT","description":"Raw radio text","inputs":[],"outputs":[{"type":"unsupported"},{"type":"string","address":8208,"max_length":7}]},
            "RAW_NUMBER":{"identifier":"RAW_NUMBER","description":"Raw number","inputs":[],"outputs":[{"type":"integer","address":8192,"mask":65535,"shift_by":0,"max_value":65535}]},
            "RAW_BOOL":{"identifier":"RAW_BOOL","description":"Raw boolean","inputs":[],"outputs":[{"type":"integer","address":8194,"mask":1,"shift_by":0,"max_value":1}]}
        }));
        this
    }
    fn edit(&self, change: impl FnOnce(&mut Value)) {
        let path = self.0.join("A-10C.json");
        let mut doc = crate::dcs_bios::read_json(&path, crate::dcs_bios::MAX_DOCUMENT).unwrap();
        change(&mut doc);
        fs::write(path, doc.to_string()).unwrap();
    }
    fn config(&self) -> Value {
        let mut c = dcs_inputs();
        c["document"]["schemaVersion"] = json!(2);
        c["document"]["roles"] = json!([]);
        c["document"]["inputs"] = json!([]);
        c["document"]["outputs"] = json!([]);
        c["document"]["feedbackOutputs"] = json!([{"id":"radio-text","channel":"RAW_TEXT","output":1},{"id":"raw-number","channel":"RAW_NUMBER"},{"id":"raw-flag","channel":"RAW_BOOL"}]);
        c["assignments"] = json!([]);
        c["connection"]["cataloguePath"] = json!(self.0);
        c
    }
}
impl Drop for CatalogueFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn raw_frame(counter: u8) -> Vec<u8> {
    let mut bytes = vec![0x55; 4];
    let mut name = [0; 24];
    name[..5].copy_from_slice(b"A-10C");
    for (address, data) in [
        (0u16, name.as_slice()),
        (8192u16, &[0x34, 0x12]),
        (8194u16, &[1, 0]),
        (8208u16, b"251.025\0".as_slice()),
        (65534u16, &[counter, 0]),
    ] {
        bytes.extend_from_slice(&address.to_le_bytes());
        bytes.extend_from_slice(&(data.len() as u16).to_le_bytes());
        bytes.extend_from_slice(data);
    }
    bytes
}
#[test]
fn named_dcs_raw_sources_keep_catalogue_ordinals_and_decode_without_physical_outputs() {
    let fixture = CatalogueFixture::new();
    let c = fixture.config();
    let parsed = integration_profile::validate(&c).unwrap();
    let compiled = compile(&parsed).unwrap();
    assert!(compiled.inputs.is_empty());
    assert!(compiled.outputs.is_empty());
    assert!(
        Profile::parse(&compiled.profile.value()).is_err(),
        "Legacy parser must still require a physical binding"
    );
    let metadata = feedback_metadata(&parsed).unwrap();
    let text = metadata
        .iter()
        .find(|m| m["bindingId"] == "radio-text")
        .unwrap();
    assert_eq!(text["kind"], "string");
    assert_eq!(text["unit"], "text");
    let flag = metadata
        .iter()
        .find(|m| m["bindingId"] == "raw-flag")
        .unwrap();
    assert_eq!(flag["kind"], "boolean");
    assert_eq!(flag["unit"], "state");
    let decoded = crate::dcs_profile::telemetry_sources(
        &compiled,
        &integration_profile::feedback_outputs(&parsed.document).unwrap(),
    )
    .unwrap();
    let mut session = crate::dcs_profile::ExportSession::new(&compiled);
    session.extend_sources(decoded.clone());
    session.feed(&raw_frame(1), 1);
    session.feed(&raw_frame(2), 2);
    let sample = session.snapshot(2);
    assert!(sample.fresh);
    assert_eq!(sample.values["radio-text"], "251.025");
    assert_eq!(sample.values["raw-number"], 0x1234);
    assert_eq!(sample.values["raw-flag"], 1);
    let f = Fixture::new();
    let observed = Arc::new(Mutex::new(None::<ExportSnapshot>));
    let copy = observed.clone();
    let mut control = Controls::default();
    control.feedback_sources = decoded;
    control.source_observer = Some(Arc::new(move |sample| {
        *copy.lock().unwrap() = Some(sample.clone())
    }));
    let cancel = Arc::new(AtomicBool::new(false));
    let snapshot = sample.clone();
    let worker = crate::dcs_profile_runtime::start_source(
        compiled,
        f.mock.panel(),
        Arc::new(Mutex::new(json!({}))),
        cancel.clone(),
        Arc::new(control),
        Some(Box::new(move |_| snapshot.clone())),
    )
    .unwrap();
    wait("raw source observer", || observed.lock().unwrap().is_some());
    assert_eq!(
        observed.lock().unwrap().as_ref().unwrap().values,
        sample.values
    );
    cancel.store(true, Ordering::Release);
    worker.join().unwrap();
    no_device_effects(&f);
}
#[test]
fn malformed_or_reserved_dcs_sources_and_wrong_ordinals_fail_before_gateway_reads() {
    let fixture = CatalogueFixture::new();
    let valid = fixture.config();
    let f = Fixture::new();
    let mut bad = valid.clone();
    bad["document"]["feedbackOutputs"][0]["output"] = json!(0);
    assert!(compile(&integration_profile::validate(&bad).unwrap()).is_err());
    for row in [
        json!({"type":"integer","address":0,"mask":65535,"shift_by":0,"max_value":65535}),
        json!({"type":"integer","address":65534,"mask":255,"shift_by":0,"max_value":255}),
        json!({"type":"string","address":3,"max_length":7}),
        json!({"type":"string","address":8208,"max_length":1025}),
        json!({"type":"string","address":65532,"max_length":7}),
    ] {
        fixture.edit(|doc| doc["RawFeedback"]["RAW_TEXT"]["outputs"][1] = row);
        assert!(compile(&integration_profile::validate(&valid).unwrap()).is_err());
        f.request(
            "integration_preflight",
            json!({"integrationConfig":valid,"gameDevices":[]}),
        )
        .unwrap();
        assert_eq!(f.runtime.status()["validation"]["valid"], false);
    }
    no_device_effects(&f);
}

#[test]
fn source_only_dcs_udp_observer_converts_boolean_metadata_and_clears_on_stop() {
    use std::net::UdpSocket;
    let catalogue = CatalogueFixture::new();
    let f = Fixture::new();
    let reservation = UdpSocket::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let mut c = catalogue.config();
    c["connection"]["listenPort"] = json!(address.port());
    c["connection"]["freshnessMs"] = json!(250);
    f.save(c);
    f.start();
    let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
    sender.send_to(&raw_frame(1), address).unwrap();
    sender.send_to(&raw_frame(2), address).unwrap();
    wait("typed DCS raw feedback from UDP observer", || {
        f.runtime.feedback_snapshot()["values"]["raw-flag"] == true
    });
    let sample = f.runtime.feedback_snapshot();
    assert_eq!(sample["sourceKind"], "simulator");
    assert_eq!(sample["running"], true);
    assert_eq!(sample["fresh"], true);
    assert_eq!(sample["values"]["radio-text"], "251.025");
    assert_eq!(sample["values"]["raw-number"], 0x1234);
    assert!(sample["values"]["raw-flag"].is_boolean());
    no_device_effects(&f);
    wait("DCS observer becomes stale", || {
        f.runtime.feedback_snapshot()["fresh"] == false
    });
    f.request("integration_stop", json!({})).unwrap();
    let stopped = f.runtime.feedback_snapshot();
    assert_eq!(stopped["running"], false);
    assert_eq!(stopped["fresh"], false);
    assert_eq!(stopped["values"], json!({}));
    no_device_effects(&f);
}
