use super::*;
use osg_companion_service::{NullSourceSink, PublicationUpload};
fn publication() -> Publication {
    let v: Value = serde_json::from_str(include_str!(
        "../../crates/companion-service/tests/fixtures/c1-flight-publication.json"
    ))
    .unwrap();
    Publication::validate(serde_json::from_value::<PublicationUpload>(v).unwrap()).unwrap()
}
fn route(p: &Publication, source: Value, output: usize) -> Route {
    serde_json::from_value(json!({"instanceId":"panel","publicationHash":p.publication_hash,"bindings":[{"channelId":format!("flight-channel-{output}"),"outputId":format!("flight-output-{output}"),"source":source}]})).unwrap()
}
#[test]
fn typed_binding_validation_rejects_aliases_units_ranges_and_unsupported_conversions() {
    let p = publication();
    let t = json!({"sourceKind":"simulator","sources":[{"bindingId":"rpm","kind":"number","unit":"%","minimum":0,"maximum":100}]});
    let mut r = route(&p, json!({"kind":"integration","bindingId":"rpm"}), 2);
    validate(&r, &p, &t).unwrap();
    r.bindings[0].output_id = "another-output".into();
    assert!(validate(&r, &p, &t).is_err());
    r.bindings[0].output_id = "flight-output-2".into();
    r.bindings[0].scale = Some(2.);
    assert!(validate(&r, &p, &t).is_err());
    let mut button = route(
        &p,
        json!({"kind":"requested","controlId":"flight-input-0"}),
        0,
    );
    validate(&button, &p, &t).unwrap();
    button.bindings[0].scale = Some(2.);
    assert!(validate(&button, &p, &t).is_err());
    button.bindings[0].scale = None;
    button.bindings[0].source = Source::HostAccepted {
        control_id: "unknown".into(),
    };
    assert!(validate(&button, &p, &t).is_err());
    assert!(schema(&route(
        &p,
        json!({"kind":"integration","bindingId":"x".repeat(32)}),
        2
    ))
    .is_err());
}
#[test]
fn truthful_samples_never_upgrade_synthetic_stale_or_missing_values_and_do_not_emit_input() {
    let p = publication();
    let r = route(&p, json!({"kind":"integration","bindingId":"rpm"}), 2);
    let owner = json!({"connected":true,"armed":true,"values":{"flight-input-0":true}});
    let mut t = json!({"running":true,"fresh":true,"sourceKind":"simulator","sourceAgeMs":10,"values":{"rpm":50}});
    let sample = samples(&r, &p, &owner, &t, &Value::Null, false)
        .unwrap()
        .remove(0);
    assert_eq!(sample.provenance, FeedbackProvenance::GameConfirmed);
    assert_eq!(sample.status, FeedbackStatus::Fresh);
    t["sourceKind"] = json!("synthetic");
    assert_eq!(
        samples(&r, &p, &owner, &t, &Value::Null, false).unwrap()[0].provenance,
        FeedbackProvenance::Local
    );
    t["sourceAgeMs"] = json!(1000);
    assert_eq!(
        samples(&r, &p, &owner, &t, &Value::Null, false).unwrap()[0].status,
        FeedbackStatus::Stale
    );
    t["values"]["rpm"] = json!(999);
    assert_eq!(
        samples(&r, &p, &owner, &t, &Value::Null, false).unwrap()[0].status,
        FeedbackStatus::Unknown
    );
    assert_eq!(
        samples(&r, &p, &json!({"connected":false}), &t, &Value::Null, false).unwrap()[0].status,
        FeedbackStatus::Disconnected
    );
    let requested = route(
        &p,
        json!({"kind":"requested","controlId":"flight-input-0"}),
        0,
    );
    assert_eq!(
        samples(&requested, &p, &owner, &t, &Value::Null, false).unwrap()[0].value,
        Some(FeedbackValue::Boolean(true))
    );
    let accepted = route(
        &p,
        json!({"kind":"host-accepted","controlId":"flight-input-0"}),
        0,
    );
    assert_eq!(
        samples(&accepted, &p, &owner, &t, &Value::Null, false).unwrap()[0].provenance,
        FeedbackProvenance::HostAccepted
    );
    let submitted = route(
        &p,
        json!({"kind":"host-submitted","controlId":"flight-input-0"}),
        0,
    );
    assert_ne!(
        samples(&submitted, &p, &owner, &t, &Value::Null, false).unwrap()[0].status,
        FeedbackStatus::Fresh
    );
    assert_eq!(
        samples(&requested, &p, &owner, &t, &Value::Null, true).unwrap()[0].status,
        FeedbackStatus::Unknown
    );
}
#[test]
fn stopped_runtime_and_corrupt_stores_are_preserved_and_bounded() {
    let path = std::env::temp_dir()
        .join(format!("osg-feedback-test-{}", uuid::Uuid::new_v4()))
        .join("routes.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"{broken").unwrap();
    assert!(load(&Some(path.clone())).corrupt.is_some());
    let hub = Hub::new("test-host".into(), Arc::new(NullSourceSink));
    let runtime = Runtime::new(
        Some(path.clone()),
        Arc::new(move || Ok(hub.clone())),
        Arc::new(|| Value::Null),
        Arc::new(|_| Ok(Value::Null)),
    );
    runtime.stop();
    assert!(runtime.request(&json!({"action":"inspect"})).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"{broken");
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(path.parent().unwrap()).unwrap();
}

#[derive(Default)]
struct InputCounter(std::sync::atomic::AtomicUsize);
impl osg_companion_service::SourceSink for InputCounter {
    fn register(&self, _: osg_companion_service::SourceDefinition) -> Result<(), String> {
        Ok(())
    }
    fn baseline(
        &self,
        _: &osg_companion_service::SourceToken,
        _: u64,
        _: &[osg_companion_service::SourceChange],
    ) -> Result<(), String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn changes(
        &self,
        _: &osg_companion_service::SourceToken,
        _: u64,
        _: &[osg_companion_service::SourceChange],
    ) -> Result<(), String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn loss(&self, _: &osg_companion_service::SourceToken) -> Result<(), String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
fn wire(h: &Hub, connection: &str, frame: Value) -> Value {
    let reply = h.handle_wire(frame, connection, h.now_ms());
    assert_eq!(reply["ok"], true, "{reply}");
    reply
}
fn paired(h: &Hub, id: &str) -> String {
    let offer = h
        .command(&json!({"command":"companion_pair_offer"}))
        .unwrap();
    let connection = format!("pair-{id}");
    let request = wire(
        h,
        &connection,
        json!({"type":"pair","secret":offer["secret"],"companionId":id,"name":id}),
    );
    h.command(&json!({"command":"companion_pair_approve","requestId":request["requestId"],"companionId":id})).unwrap();
    wire(
        h,
        &connection,
        json!({"type":"pair-status","secret":offer["secret"],"requestId":request["requestId"]}),
    )["token"]
        .as_str()
        .unwrap()
        .into()
}
fn publish(h: &Hub, racing: bool) -> Publication {
    let data = if racing {
        include_str!("../../crates/companion-service/tests/fixtures/c1-racing-publication.json")
    } else {
        include_str!("../../crates/companion-service/tests/fixtures/c1-flight-publication.json")
    };
    let upload: PublicationUpload = serde_json::from_str(data).unwrap();
    let p = Publication::validate(upload.clone()).unwrap();
    let mut command = serde_json::to_value(upload).unwrap();
    command["command"] = json!("companion_publish");
    h.command(&command).unwrap();
    p
}
fn assign(h: &Hub, id: &str, companion: &str, p: &Publication) {
    h.command(&json!({"command":"companion_assign","instanceId":id,"companionId":companion,"publicationHash":p.publication_hash})).unwrap();
}
fn setup() -> (Hub, Publication, String, Arc<InputCounter>) {
    let counter = Arc::new(InputCounter::default());
    let h = Hub::new("feedback-host".into(), counter.clone());
    let token = paired(&h, "installation");
    let p = publish(&h, false);
    assign(&h, "panel", "installation", &p);
    (h, p, token, counter)
}
fn armed(h: &Hub, id: &str, token: &str, p: &Publication) -> Value {
    wire(h, id, json!({"type":"auth","token":token}));
    let open = wire(h, id, json!({"type":"open","instanceId":id}));
    let values = p
        .controls
        .iter()
        .map(|c| (c.id.clone(), c.fallback()))
        .collect::<BTreeMap<_, _>>();
    wire(
        h,
        id,
        json!({"type":"baseline","version":1,"clientId":open["clientId"],"sessionId":open["sessionId"],"generation":open["generation"],"publicationHash":p.publication_hash,"sequence":"0","values":values}),
    );
    open
}
fn input(h: &Hub, id: &str, open: &Value, sequence: &str, value: bool) {
    wire(
        h,
        id,
        json!({"type":"input","version":1,"clientId":open["clientId"],"sessionId":open["sessionId"],"generation":open["generation"],"publicationHash":open["publicationHash"],"sequence":sequence,"changes":[{"controlId":"flight-input-0","value":value}]}),
    );
}
fn observe(h: &Hub, id: &str, open: &Value, sequence: &str) -> Value {
    wire(h,id,json!({"type":"heartbeat","version":1,"clientId":open["clientId"],"sessionId":open["sessionId"],"generation":open["generation"],"publicationHash":open["publicationHash"],"sequence":sequence}))["feedback"].clone()
}
fn telemetry() -> Arc<Mutex<Value>> {
    Arc::new(Mutex::new(
        json!({"revision":"integration-one","epoch":1,"running":true,"fresh":true,"sourceKind":"simulator","sourceAgeMs":10,"sources":[{"bindingId":"rpm","label":"RPM","kind":"number","unit":"%","minimum":0,"maximum":100},{"bindingId":"label","label":"Label","kind":"string","unit":"text"}],"values":{"rpm":50,"label":"READY"}}),
    ))
}
fn test_runtime(h: &Hub, t: &Arc<Mutex<Value>>, path: Option<PathBuf>) -> Runtime {
    let hub = h.clone();
    let state = t.clone();
    Runtime {
        shared: Arc::new(Shared {
            state: Mutex::new(load(&path)),
            operation: Mutex::new(()),
            hub: Arc::new(move || Ok(hub.clone())),
            telemetry: Arc::new(move || state.lock().unwrap().clone()),
            submission: Arc::new(|_| Ok(Value::Null)),
            path,
            stop: AtomicBool::new(false),
        }),
        worker: Mutex::new(None),
    }
}
fn save(runtime: &Runtime, id: &str, p: &Publication, source: Value, output: usize) -> Value {
    let mut r = route(p, source, output);
    r.instance_id = id.into();
    runtime.request(&json!({"action":"save","instanceId":id,"publicationHash":p.publication_hash,"bindings":r.bindings})).unwrap()
}
fn action(runtime: &Runtime, id: &str, p: &Publication, action: &str) -> Value {
    runtime
        .request(&json!({"action":action,"instanceId":id,"publicationHash":p.publication_hash}))
        .unwrap()
}
fn temp_path() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("osg-c5-feedback-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    dir.join("routes.json")
}
fn cleanup(path: &PathBuf) {
    if path.is_file() {
        std::fs::remove_file(path).unwrap();
    }
    std::fs::remove_dir(path.parent().unwrap()).unwrap();
}
#[test]
fn inspect_details_are_selected_bounded_and_do_not_arm_or_change_sources() {
    let (h, p, _, counter) = setup();
    assign(&h, "second", "installation", &p);
    let runtime = test_runtime(&h, &telemetry(), None);
    let before = h.command(&json!({"command":"companion_status"})).unwrap();
    let first = runtime.request(&json!({"action":"inspect"})).unwrap();
    assert_eq!(
        first["feedbackRoutes"]["instances"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(!first["feedbackRoutes"]["instances"][0]["controls"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(first["feedbackRoutes"]["instances"][1]["controls"]
        .as_array()
        .unwrap()
        .is_empty());
    let selected = runtime
        .request(&json!({"action":"inspect","instanceId":"second"}))
        .unwrap();
    assert!(selected["feedbackRoutes"]["instances"][0]["outputs"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!selected["feedbackRoutes"]["instances"][1]["outputs"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(counter.0.load(Ordering::SeqCst), 0);
    assert_eq!(
        h.command(&json!({"command":"companion_status"})).unwrap(),
        before
    );
}
#[test]
fn requested_and_host_accepted_worker_feedback_never_creates_input() {
    let (h, p, token, counter) = setup();
    let runtime = test_runtime(&h, &telemetry(), None);
    let open = armed(&h, "panel", &token, &p);
    input(&h, "panel", &open, "1", true);
    let before = counter.0.load(Ordering::SeqCst);
    save(
        &runtime,
        "panel",
        &p,
        json!({"kind":"requested","controlId":"flight-input-0"}),
        0,
    );
    runtime.shared.tick();
    let observed = observe(&h, "panel", &open, "1");
    assert_eq!(observed["values"][0]["value"], true);
    assert_eq!(observed["values"][0]["status"], "fresh");
    assert_eq!(observed["values"][0]["provenance"], "requested");
    save(
        &runtime,
        "panel",
        &p,
        json!({"kind":"host-accepted","controlId":"flight-input-0"}),
        0,
    );
    runtime.shared.tick();
    assert_eq!(
        observe(&h, "panel", &open, "2")["values"][0]["provenance"],
        "host-accepted"
    );
    assert_eq!(counter.0.load(Ordering::SeqCst), before);
}
#[test]
fn supported_telemetry_and_typed_text_are_confirmed_only_for_fresh_simulator_samples() {
    let (h, p, token, counter) = setup();
    let t = telemetry();
    let runtime = test_runtime(&h, &t, None);
    let open = armed(&h, "panel", &token, &p);
    let before = counter.0.load(Ordering::SeqCst);
    save(
        &runtime,
        "panel",
        &p,
        json!({"kind":"integration","bindingId":"rpm"}),
        2,
    );
    runtime.shared.tick();
    let first = observe(&h, "panel", &open, "1");
    assert_eq!(first["values"][0]["provenance"], "game-confirmed");
    assert_eq!(first["values"][0]["value"].as_f64(), Some(50.));
    assert_eq!(first["values"][0]["unit"], "%");
    save(
        &runtime,
        "panel",
        &p,
        json!({"kind":"integration","bindingId":"label"}),
        1,
    );
    runtime.shared.tick();
    assert_eq!(
        observe(&h, "panel", &open, "2")["values"][0]["value"],
        "READY"
    );
    t.lock().unwrap()["sourceKind"] = json!("synthetic");
    std::thread::sleep(Duration::from_millis(37));
    runtime.shared.tick();
    assert_eq!(
        observe(&h, "panel", &open, "3")["values"][0]["provenance"],
        "local"
    );
    t.lock().unwrap()["sourceAgeMs"] = json!(1000);
    std::thread::sleep(Duration::from_millis(37));
    runtime.shared.tick();
    assert_eq!(
        observe(&h, "panel", &open, "4")["values"][0]["status"],
        "stale"
    );
    assert_eq!(counter.0.load(Ordering::SeqCst), before);
}
#[test]
fn reset_resync_and_source_changes_preserve_armed_input_without_replay() {
    let (h, p, token, counter) = setup();
    let runtime = test_runtime(&h, &telemetry(), None);
    let open = armed(&h, "panel", &token, &p);
    input(&h, "panel", &open, "1", true);
    save(
        &runtime,
        "panel",
        &p,
        json!({"kind":"requested","controlId":"flight-input-0"}),
        0,
    );
    runtime.shared.tick();
    let before = counter.0.load(Ordering::SeqCst);
    action(&runtime, "panel", &p, "reset");
    runtime.shared.tick();
    assert_eq!(
        observe(&h, "panel", &open, "1")["values"][0]["status"],
        "unknown"
    );
    assert_eq!(counter.0.load(Ordering::SeqCst), before);
    action(&runtime, "panel", &p, "resync");
    runtime.shared.tick();
    assert_eq!(observe(&h, "panel", &open, "2")["values"][0]["value"], true);
    assert_eq!(counter.0.load(Ordering::SeqCst), before);
    action(&runtime, "panel", &p, "reset");
    input(&h, "panel", &open, "2", false);
    runtime.shared.tick();
    assert_eq!(
        observe(&h, "panel", &open, "3")["values"][0]["value"],
        false
    );
}
#[test]
fn independent_instances_keep_feedback_and_input_ownership_separate() {
    let (h, p, token, counter) = setup();
    assign(&h, "second", "installation", &p);
    let runtime = test_runtime(&h, &telemetry(), None);
    let a = armed(&h, "panel", &token, &p);
    let b = armed(&h, "second", &token, &p);
    input(&h, "panel", &a, "1", true);
    let before = counter.0.load(Ordering::SeqCst);
    save(
        &runtime,
        "panel",
        &p,
        json!({"kind":"requested","controlId":"flight-input-0"}),
        0,
    );
    save(
        &runtime,
        "second",
        &p,
        json!({"kind":"requested","controlId":"flight-input-0"}),
        0,
    );
    runtime.shared.tick();
    assert_eq!(observe(&h, "panel", &a, "1")["values"][0]["value"], true);
    assert_eq!(observe(&h, "second", &b, "1")["values"][0]["value"], false);
    action(&runtime, "panel", &p, "reset");
    std::thread::sleep(Duration::from_millis(37));
    runtime.shared.tick();
    assert_eq!(
        observe(&h, "panel", &a, "2")["values"][0]["status"],
        "unknown"
    );
    assert_eq!(
        observe(&h, "second", &b, "2")["values"][0]["status"],
        "fresh"
    );
    assert_eq!(counter.0.load(Ordering::SeqCst), before);
}
#[test]
fn durable_routes_reopen_pinned_and_never_activate_input_on_save_or_load() {
    let (h, p, _, counter) = setup();
    let path = temp_path();
    let runtime = test_runtime(&h, &telemetry(), Some(path.clone()));
    save(
        &runtime,
        "panel",
        &p,
        json!({"kind":"requested","controlId":"flight-input-0"}),
        0,
    );
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(load(&Some(path.clone())).routes.len(), 1);
    assert_eq!(counter.0.load(Ordering::SeqCst), 0);
    drop(runtime);
    let restored = test_runtime(&h, &telemetry(), Some(path.clone()));
    let inspected = restored
        .request(&json!({"action":"inspect","instanceId":"panel"}))
        .unwrap();
    assert_eq!(inspected["feedbackRoutes"]["instances"][0]["pinned"], true);
    restored.shared.tick();
    assert_eq!(counter.0.load(Ordering::SeqCst), 0);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    drop(restored);
    cleanup(&path);
}
#[test]
fn changed_integration_revision_clears_old_values_until_explicit_route_save() {
    let (h, p, token, _) = setup();
    let t = telemetry();
    let runtime = test_runtime(&h, &t, None);
    let open = armed(&h, "panel", &token, &p);
    save(
        &runtime,
        "panel",
        &p,
        json!({"kind":"integration","bindingId":"rpm"}),
        2,
    );
    runtime.shared.tick();
    assert_eq!(
        observe(&h, "panel", &open, "1")["values"][0]["status"],
        "fresh"
    );
    t.lock().unwrap()["revision"] = json!("integration-two");
    runtime.shared.tick();
    assert_eq!(
        observe(&h, "panel", &open, "2")["values"][0]["status"],
        "unknown"
    );
    assert!(runtime.shared.state.lock().unwrap().errors["panel"].contains("configuration changed"));
    save(
        &runtime,
        "panel",
        &p,
        json!({"kind":"integration","bindingId":"rpm"}),
        2,
    );
    runtime.shared.tick();
    assert_eq!(
        observe(&h, "panel", &open, "3")["values"][0]["status"],
        "fresh"
    );
}
#[test]
fn changed_publication_never_reinterprets_saved_bindings_or_overwrites_their_pin() {
    let (h, p, token, counter) = setup();
    let runtime = test_runtime(&h, &telemetry(), None);
    let _ = armed(&h, "panel", &token, &p);
    save(
        &runtime,
        "panel",
        &p,
        json!({"kind":"requested","controlId":"flight-input-0"}),
        0,
    );
    let saved =
        serde_json::to_value(&runtime.shared.state.lock().unwrap().routes["panel"]).unwrap();
    let other = publish(&h, true);
    assign(&h, "panel", "installation", &other);
    let before = counter.0.load(Ordering::SeqCst);
    runtime.shared.tick();
    assert!(runtime.shared.state.lock().unwrap().errors["panel"].contains("revision changed"));
    let view = runtime
        .request(&json!({"action":"inspect","instanceId":"panel"}))
        .unwrap();
    assert_eq!(view["feedbackRoutes"]["instances"][0]["pinned"], false);
    assert_eq!(
        serde_json::to_value(&runtime.shared.state.lock().unwrap().routes["panel"]).unwrap(),
        saved
    );
    assert_eq!(counter.0.load(Ordering::SeqCst), before);
}
#[test]
fn failed_and_malformed_route_requests_preserve_the_last_committed_configuration() {
    let (h, p, _, _) = setup();
    let path = temp_path();
    let runtime = test_runtime(&h, &telemetry(), Some(path.clone()));
    save(
        &runtime,
        "panel",
        &p,
        json!({"kind":"requested","controlId":"flight-input-0"}),
        0,
    );
    let before = std::fs::read(&path).unwrap();
    for payload in [
        json!({"action":"save"}),
        json!({"action":"unknown","instanceId":"panel","publicationHash":p.publication_hash}),
        json!({"action":"save","instanceId":"panel","publicationHash":"sha256:bad","bindings":[]}),
        json!({"action":"save","instanceId":"panel","publicationHash":p.publication_hash,"bindings":"bad"}),
        json!({"action":"save","instanceId":"panel","publicationHash":p.publication_hash,"bindings":[{"channelId":"unknown","outputId":"missing","source":{"kind":"requested","controlId":"flight-input-0"}}]}),
    ] {
        assert!(runtime.request(&payload).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    drop(runtime);
    cleanup(&path);
}
#[test]
fn malformed_duplicate_oversized_and_nonregular_stores_fail_closed_without_replacement() {
    let path = temp_path();
    let p = publication();
    let r = route(
        &p,
        json!({"kind":"requested","controlId":"flight-input-0"}),
        0,
    );
    for bytes in [
        b"{broken".to_vec(),
        serde_json::to_vec(&json!({"schema":2,"routes":[]})).unwrap(),
        serde_json::to_vec(&json!({"schema":1,"routes":[r.clone(),r.clone()]})).unwrap(),
        vec![b'x'; LIMIT + 1],
    ] {
        std::fs::write(&path, &bytes).unwrap();
        assert!(load(&Some(path.clone())).corrupt.is_some());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(load(&Some(path.clone())).corrupt.is_some());
    assert!(persist(&Some(path.clone()), &BTreeMap::from([("panel".into(), r)])).is_err());
    assert!(path.is_dir());
    std::fs::remove_dir(&path).unwrap();
    cleanup(&path);
}
#[test]
fn corrupted_configuration_and_unavailable_hub_are_actionable_and_do_not_erase_data() {
    let (h, p, _, _) = setup();
    let path = temp_path();
    std::fs::write(&path, b"{invalid").unwrap();
    let runtime = test_runtime(&h, &telemetry(), Some(path.clone()));
    let r = route(
        &p,
        json!({"kind":"requested","controlId":"flight-input-0"}),
        0,
    );
    assert!(runtime.request(&json!({"action":"save","instanceId":"panel","publicationHash":p.publication_hash,"bindings":r.bindings})).unwrap_err().contains("repair"));
    assert_eq!(std::fs::read(&path).unwrap(), b"{invalid");
    drop(runtime);
    cleanup(&path);
    let bad = Runtime {
        shared: Arc::new(Shared {
            state: Mutex::new(State::default()),
            operation: Mutex::new(()),
            hub: Arc::new(|| Err("Listener unavailable".into())),
            telemetry: Arc::new(|| Value::Null),
            submission: Arc::new(|_| Ok(Value::Null)),
            path: None,
            stop: AtomicBool::new(false),
        }),
        worker: Mutex::new(None),
    };
    assert!(bad
        .request(&json!({"action":"inspect"}))
        .unwrap_err()
        .contains("Listener unavailable"));
    bad.shared.tick();
}

#[test]
fn submitted_only_changes_unsuppress_reset_without_claiming_game_confirmation_or_sending_input() {
    let (h, p, token, counter) = setup();
    let mut runtime = test_runtime(&h, &telemetry(), None);
    let submitted = Arc::new(Mutex::new(
        json!({"confirmed":true,"publicationHash":p.publication_hash,"values":{"flight-input-0":true}}),
    ));
    let own = submitted.clone();
    Arc::get_mut(&mut runtime.shared).unwrap().submission =
        Arc::new(move |_| Ok(own.lock().unwrap().clone()));
    let open = armed(&h, "panel", &token, &p);
    let before = counter.0.load(Ordering::SeqCst);
    save(
        &runtime,
        "panel",
        &p,
        json!({"kind":"host-submitted","controlId":"flight-input-0"}),
        0,
    );
    runtime.shared.tick();
    let first = observe(&h, "panel", &open, "1");
    assert_eq!(first["values"][0]["provenance"], "host-submitted");
    assert_eq!(first["values"][0]["status"], "fresh");
    assert_eq!(first["values"][0]["value"], true);
    action(&runtime, "panel", &p, "reset");
    runtime.shared.tick();
    assert_eq!(
        observe(&h, "panel", &open, "2")["values"][0]["status"],
        "unknown"
    );
    submitted.lock().unwrap()["values"]["flight-input-0"] = json!(false);
    std::thread::sleep(Duration::from_millis(37));
    runtime.shared.tick();
    assert_eq!(
        observe(&h, "panel", &open, "3")["values"][0]["value"],
        false
    );
    assert_eq!(counter.0.load(Ordering::SeqCst), before);
}
#[test]
fn captured_game_feedback_preflight_preserves_immutable_route_pins_and_input_state() {
    let (h, p, _, counter) = setup();
    let runtime = test_runtime(&h, &telemetry(), None);
    save(
        &runtime,
        "panel",
        &p,
        json!({"kind":"requested","controlId":"flight-input-0"}),
        0,
    );
    let mut capture = json!({"schemaVersion":1,"assignments":[{"instanceId":"panel","companionId":"installation","publicationHash":p.publication_hash,"capabilityFingerprint":p.capability_fingerprint}]});
    assert!(runtime.preflight_assignments(&capture).is_ok());
    capture["assignments"][0]["publicationHash"] = json!(format!("sha256:{}", "b".repeat(64)));
    assert!(runtime.preflight_assignments(&capture).is_err());
    assert!(runtime
        .preflight_assignments(&json!({"schemaVersion":1,"assignments":[]}))
        .is_err());
    runtime.shared.state.lock().unwrap().corrupt = Some("corrupt".into());
    assert!(runtime
        .preflight_assignments(&capture)
        .unwrap_err()
        .contains("Repair"));
    assert_eq!(counter.0.load(Ordering::SeqCst), 0);
}
#[test]
fn failed_atomic_route_save_restores_prior_binding_ownership_and_preserves_saved_bytes() {
    let (h, p, token, _) = setup();
    let path = temp_path();
    let runtime = test_runtime(&h, &telemetry(), Some(path.clone()));
    let open = armed(&h, "panel", &token, &p);
    save(
        &runtime,
        "panel",
        &p,
        json!({"kind":"requested","controlId":"flight-input-0"}),
        0,
    );
    let original = std::fs::read(&path).unwrap();
    let backup = path.with_extension("saved");
    std::fs::rename(&path, &backup).unwrap();
    std::fs::create_dir(&path).unwrap();
    let r = route(
        &p,
        json!({"kind":"host-accepted","controlId":"flight-input-0"}),
        0,
    );
    assert!(runtime.request(&json!({"action":"save","instanceId":"panel","publicationHash":p.publication_hash,"bindings":r.bindings})).is_err());
    assert!(matches!(
        runtime.shared.state.lock().unwrap().routes["panel"].bindings[0].source,
        Source::Requested { .. }
    ));
    assert_eq!(std::fs::read(&backup).unwrap(), original);
    runtime.shared.tick();
    assert_eq!(
        observe(&h, "panel", &open, "1")["values"][0]["provenance"],
        "requested"
    );
    std::fs::remove_dir(&path).unwrap();
    std::fs::rename(&backup, &path).unwrap();
    drop(runtime);
    cleanup(&path);
}

#[test]
fn captured_integration_feedback_mismatch_rejects_before_input_or_route_side_effects() {
    let (h, p, _, counter) = setup();
    let t = telemetry();
    let runtime = test_runtime(&h, &t, None);
    save(
        &runtime,
        "panel",
        &p,
        json!({"kind":"integration","bindingId":"rpm"}),
        2,
    );
    let capture = json!({"schemaVersion":1,"assignments":[{"instanceId":"panel","companionId":"installation","publicationHash":p.publication_hash,"capabilityFingerprint":p.capability_fingerprint}]});
    let before =
        serde_json::to_value(&runtime.shared.state.lock().unwrap().routes["panel"]).unwrap();
    let state = h.command(&json!({"command":"companion_status"})).unwrap();
    assert!(runtime
        .preflight_integration(&capture, "integration-one")
        .is_ok());
    assert!(runtime
        .preflight_integration(&capture, "integration-two")
        .unwrap_err()
        .contains("Captured integration differs"));
    assert_eq!(counter.0.load(Ordering::SeqCst), 0);
    assert_eq!(
        h.command(&json!({"command":"companion_status"})).unwrap(),
        state
    );
    assert_eq!(
        serde_json::to_value(&runtime.shared.state.lock().unwrap().routes["panel"]).unwrap(),
        before
    );
    runtime.shared.state.lock().unwrap().corrupt = Some("unreadable".into());
    assert!(runtime
        .preflight_integration(&capture, "integration-one")
        .is_err());
}
#[test]
fn local_only_unconfigured_and_legacy_game_feedback_do_not_require_an_integration_pin() {
    let (h, p, _, counter) = setup();
    let runtime = test_runtime(&h, &telemetry(), None);
    let capture = json!({"schemaVersion":1,"assignments":[{"instanceId":"panel","companionId":"installation","publicationHash":p.publication_hash,"capabilityFingerprint":p.capability_fingerprint}]});
    assert!(runtime
        .preflight_integration(&capture, "different-integration")
        .is_ok());
    save(
        &runtime,
        "panel",
        &p,
        json!({"kind":"requested","controlId":"flight-input-0"}),
        0,
    );
    assert!(runtime
        .preflight_integration(&capture, "different-integration")
        .is_ok());
    assert!(runtime
        .preflight_integration(&Value::Null, "different-integration")
        .is_ok());
    assert!(runtime
        .preflight_integration(&json!({"schemaVersion":1,"assignments":[]}), "different")
        .is_err());
    assert_eq!(counter.0.load(Ordering::SeqCst), 0);
}

fn maximum_publication() -> PublicationUpload {
    let stable = |prefix: &str, index: usize| {
        format!("{:<64}", format!("{prefix}{index:03}")).replace(' ', "x")
    };
    let bounds = json!({"x":0,"y":0,"width":1,"height":1});
    let label = "😀".repeat(60);
    let unit = "😀".repeat(60);
    let controls=(0..128).map(|i|json!({"id":stable("control",i),"label":label,"kind":"axis","axisMode":"bipolar","fallback":0})).collect::<Vec<_>>();
    let outputs=(0..64).map(|i|json!({"id":stable("output",i),"channelId":stable("channel",i),"kind":"gauge","label":"Gauge"})).collect::<Vec<_>>();
    let mut widgets=(0..128).map(|i|json!({"id":stable("widgetinput",i),"pageId":"page","kind":"slider","label":label,"bounds":bounds,"controlIds":[stable("control",i)],"feedbackIds":[],"config":{"centering":"spring","deadzone":0,"inverted":false,"orientation":"horizontal"}})).collect::<Vec<_>>();
    widgets.extend((0..64).map(|i|json!({"id":stable("widgetoutput",i),"pageId":"page","kind":"gauge","label":"Gauge","bounds":bounds,"controlIds":[],"feedbackIds":[stable("output",i)],"config":{"min":-1000000000,"max":1000000000,"unit":unit,"precision":4}})));
    let mut items = (0..128)
        .map(|i| json!({"kind":"control","refId":stable("control",i),"bounds":bounds}))
        .collect::<Vec<_>>();
    items.extend(
        (0..64).map(|i| json!({"kind":"feedback","refId":stable("output",i),"bounds":bounds})),
    );
    let doc = json!({"schemaVersion":2,"ui":{"schemaVersion":1,"id":"max-ui","title":"Maximum feedback","controls":controls,"feedbackWidgets":outputs,"pages":[{"id":"page","title":"Page","items":items}],"assets":[]},"presentation":{"widgets":widgets,"groups":[],"viewports":[{"pageId":"page","width":1280,"height":800}],"grid":{"columns":24,"rows":16,"snap":true}},"targets":(0..8).map(|i|json!({"id":stable("target",i),"label":"Target","axes":vec!["bipolar";16],"buttons":0,"hats":0})).collect::<Vec<_>>(),"bindings":(0..128).map(|i|json!({"controlId":stable("control",i),"targetId":stable("target",i/16),"kind":"axis","index":i%16})).collect::<Vec<_>>()});
    let canonical_document = serde_json::to_string(&doc).unwrap();
    PublicationUpload {
        publication_hash: osg_companion_service::tls::sha256(canonical_document.as_bytes()),
        canonical_document,
        assets: vec![],
        capability_fingerprint: None,
    }
}
#[test]
fn maximum_selected_feedback_inspection_fits_existing_response_budget_without_losing_routing_metadata(
) {
    let upload = maximum_publication();
    let p = Publication::validate(upload.clone()).unwrap();
    let counter = Arc::new(InputCounter::default());
    let h = Hub::new("max-host".into(), counter.clone());
    paired(&h, "installation");
    let mut command = serde_json::to_value(upload).unwrap();
    command["command"] = json!("companion_publish");
    h.command(&command).unwrap();
    let stable = |prefix: &str, index: usize| {
        format!("{:<64}", format!("{prefix}{index:03}")).replace(' ', "x")
    };
    for i in 0..32 {
        assign(&h, &stable("instance", i), "installation", &p);
    }
    let t = telemetry();
    t.lock().unwrap()["sources"]=json!((0..16).map(|i|json!({"bindingId":format!("{:x<31}",format!("source{i}")),"label":"界".repeat(120),"channel":stable("telemetry",i),"kind":"number","unit":"😀".repeat(60),"minimum":-1000000000.0,"maximum":1000000000.0})).collect::<Vec<_>>());
    let runtime = test_runtime(&h, &t, None);
    for i in 1..32 {
        runtime
            .shared
            .state
            .lock()
            .unwrap()
            .errors
            .insert(stable("instance", i), "界".repeat(256));
    }
    let bindings=(0..64).map(|i|json!({"channelId":stable("channel",i),"outputId":stable("output",i),"source":{"kind":"host-submitted","controlId":stable("control",i)},"scale":999999.123456789,"offset":123456789.12345679})).collect::<Vec<_>>();
    let response=runtime.request(&json!({"action":"save","instanceId":stable("instance",0),"publicationHash":p.publication_hash,"bindings":bindings})).unwrap();
    let wrapped = json!({"ok":true,"state":{"companions":response}});
    let bytes = serde_json::to_vec(&wrapped).unwrap();
    assert!(bytes.len() > 65_536);
    assert!(bytes.len() <= 131_072, "{} bytes", bytes.len());
    let routes = &wrapped["state"]["companions"]["feedbackRoutes"];
    assert_eq!(routes["instances"].as_array().unwrap().len(), 32);
    assert_eq!(routes["sources"].as_array().unwrap().len(), 16);
    let selected = &routes["instances"][0];
    assert_eq!(selected["controls"].as_array().unwrap().len(), 128);
    assert_eq!(selected["outputs"].as_array().unwrap().len(), 64);
    assert_eq!(selected["bindings"], json!(bindings));
    assert_eq!(selected["controls"][0]["id"], stable("control", 0));
    assert_eq!(
        selected["outputs"][0]["unit"]
            .as_str()
            .unwrap()
            .encode_utf16()
            .count(),
        120
    );
    assert_eq!(counter.0.load(Ordering::SeqCst), 0);
}
