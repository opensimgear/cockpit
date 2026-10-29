use super::*;
use std::time::{Duration, Instant};

#[test]
fn live_owner_validation_and_deferred_clear_never_wait_for_the_hub_mutex() {
    let fixture = Fixture::new();
    let held = fixture.hub.inner.lock().unwrap();
    let started = Instant::now();
    for _ in 0..20 {
        assert_eq!(fixture.hub.try_validate_profile_assignment(&fixture.receipt).unwrap(), None);
        assert!(!fixture.hub.try_clear_profile_feedback(&fixture.receipt).unwrap());
    }
    assert!(started.elapsed() < Duration::from_millis(100));
    drop(held);
    assert_eq!(fixture.hub.profile_feedback_token(&fixture.receipt).unwrap(), fixture.token);
    assert_eq!(fixture.hub.try_validate_profile_assignment(&fixture.receipt).unwrap(), Some(()));
    assert!(fixture.hub.try_clear_profile_feedback(&fixture.receipt).unwrap());
    assert_ne!(fixture.hub.profile_feedback_token(&fixture.receipt).unwrap().feedback_generation, fixture.token.feedback_generation);
}

#[test]
fn deferred_clear_from_an_old_owner_cannot_reset_the_new_owner() {
    let fixture = Fixture::new();
    let held = fixture.hub.inner.lock().unwrap();
    assert!(!fixture.hub.try_clear_profile_feedback(&fixture.receipt).unwrap());
    drop(held);
    let proof = fixture.hub.prepare_profile_assignment("instance", &fixture.panel.publication_hash).unwrap();
    let incoming = fixture.hub.capture_profile_assignment(&proof, &uuid::Uuid::new_v4().to_string(), Some(&fixture.receipt)).unwrap();
    fixture.hub.acquire_profile_assignment(&incoming).unwrap();
    assert!(fixture.hub.try_clear_profile_feedback(&fixture.receipt).is_err());
    assert!(fixture.hub.try_validate_profile_assignment(&fixture.receipt).is_err());
    assert_eq!(fixture.hub.try_validate_profile_assignment(&incoming).unwrap(), Some(()));
}

struct Fixture {
    hub: Hub,
    receipt: profile::AssignmentReceipt,
    panel: Publication,
    secret: String,
    open: Value,
    token: FeedbackToken,
    sample: FeedbackSample,
}
impl Fixture {
    fn new() -> Self {
        let hub = Hub::new("async-feedback".into(), Arc::new(NullSourceSink));
        let offer = hub.command(&json!({"command":"companion_pair_offer"})).unwrap();
        let wire = |value| hub.handle_wire(value, "connection", hub.now_ms());
        let request = wire(json!({"type":"pair","secret":offer["secret"],"companionId":"screen","name":"Screen"}));
        hub.command(&json!({"command":"companion_pair_approve","requestId":request["requestId"],"companionId":"screen"})).unwrap();
        let secret = wire(json!({"type":"pair-status","secret":offer["secret"],"requestId":request["requestId"]}))["token"].as_str().unwrap().to_owned();
        let upload: PublicationUpload = serde_json::from_str(include_str!("../tests/fixtures/c1-flight-publication.json")).unwrap();
        let panel = Publication::validate_legacy_test_fixture(upload.clone()).unwrap();
        hub.retain_legacy_test_fixture(upload).unwrap();
        hub.command(&json!({"command":"companion_assign","companionId":"screen","instanceId":"instance","publicationHash":panel.publication_hash})).unwrap();
        let proof = hub.prepare_profile_assignment("instance", &panel.publication_hash).unwrap();
        let receipt = hub.capture_profile_assignment(&proof, &uuid::Uuid::new_v4().to_string(), None).unwrap();
        hub.acquire_profile_assignment(&receipt).unwrap();
        let descriptor = panel.feedback.iter().find(|descriptor| descriptor.kind == FeedbackKind::Boolean).unwrap();
        let binding = FeedbackBinding { channel_id: descriptor.channel_id.clone(), output_id: descriptor.output_ids[0].clone() };
        let sample = FeedbackSample { channel_id: binding.channel_id.clone(), output_id: binding.output_id.clone(),
            value: Some(FeedbackValue::Boolean(true)), status: FeedbackStatus::Fresh,
            provenance: FeedbackProvenance::Local, source_kind: FeedbackSource::Host, source_age_ms: 10,
            unit: "boolean".into(), range: None };
        hub.configure_profile_feedback(&receipt, &[binding]).unwrap();
        assert_eq!(wire(json!({"type":"auth","token":secret}))["ok"], true);
        let open = wire(json!({"type":"open","instanceId":"instance"}));
        assert_eq!(open["ok"], true, "{open}");
        let baseline = json!({"version":1,"type":"baseline","clientId":open["clientId"],"sessionId":open["sessionId"],
            "generation":open["generation"],"publicationHash":open["publicationHash"],"sequence":"0",
            "values":panel.controls.iter().map(|control| (control.id.clone(), control.fallback())).collect::<BTreeMap<_,_>>()});
        assert_eq!(wire(baseline)["ok"], true);
        let token = hub.profile_feedback_token(&receipt).unwrap();
        Self { hub, receipt, panel, secret, open, token, sample }
    }
    fn publish(&self, captured: Instant, current: impl FnOnce() -> bool) -> Result<Option<u64>, String> {
        self.hub.try_publish_profile_feedback(&self.receipt, &self.token, &[self.sample.clone()],
            FeedbackMode::Snapshot, captured, Duration::from_millis(250), current)
    }
    fn values(&self) -> Value {
        let response = self.hub.handle_wire(json!({"version":1,"type":"heartbeat","clientId":self.open["clientId"],
            "sessionId":self.open["sessionId"],"generation":self.open["generation"],
            "publicationHash":self.open["publicationHash"],"sequence":"1"}), "connection", self.hub.now_ms());
        assert_eq!(response["ok"], true, "{response}");
        response["feedback"]["values"].clone()
    }
}

#[test]
fn busy_expired_and_cancelled_feedback_never_waits_or_publishes() {
    let fixture = Fixture::new();
    let locked = fixture.hub.inner.lock().unwrap();
    let began = Instant::now();
    assert_eq!(fixture.publish(Instant::now(), || panic!("Busy publication evaluated its guard")).unwrap(), None);
    assert!(began.elapsed() < Duration::from_millis(100));
    drop(locked);
    assert_eq!(fixture.publish(Instant::now() - Duration::from_millis(251), || true).unwrap(), None);
    assert_eq!(fixture.publish(Instant::now(), || false).unwrap(), None);
    assert_ne!(fixture.values()[0]["status"], "fresh");
}

#[test]
fn source_age_is_added_under_the_lock_and_guard_observes_that_lock() {
    let fixture = Fixture::new();
    let captured = Instant::now() - Duration::from_millis(40);
    assert!(fixture.publish(captured, || {
        assert!(matches!(fixture.hub.inner.try_lock(), Err(std::sync::TryLockError::WouldBlock)));
        true
    }).unwrap().is_some());
    let values = fixture.values();
    assert_eq!(values[0]["value"], true);
    assert!(values[0]["sourceAgeMs"].as_u64().unwrap() >= 50);
}

#[test]
fn cleared_token_cannot_publish_over_a_new_binding_generation() {
    let fixture = Fixture::new();
    fixture.hub.clear_profile_feedback(&fixture.receipt, fixture.hub.now_ms()).unwrap();
    let current = fixture.hub.profile_feedback_token(&fixture.receipt).unwrap();
    assert_ne!(current.feedback_generation, fixture.token.feedback_generation);
    fixture.hub.try_publish_profile_feedback(&fixture.receipt, &current, &[fixture.sample.clone()],
        FeedbackMode::Snapshot, Instant::now(), Duration::from_millis(250), || true).unwrap().unwrap();
    assert!(fixture.publish(Instant::now(), || true).is_err());
    assert_eq!(fixture.hub.profile_feedback_token(&fixture.receipt).unwrap(), current);
    assert_eq!(fixture.values()[0]["value"], true);
}

#[test]
fn old_owner_and_session_cannot_replace_a_newly_acquired_peer() {
    let fixture = Fixture::new();
    let proof = fixture.hub.prepare_profile_assignment("instance", &fixture.panel.publication_hash).unwrap();
    let incoming = fixture.hub.capture_profile_assignment(&proof, &uuid::Uuid::new_v4().to_string(), Some(&fixture.receipt)).unwrap();
    fixture.hub.acquire_profile_assignment(&incoming).unwrap();
    assert!(fixture.publish(Instant::now(), || true).is_err());
    // The old worker has no clearing side effect and cannot reclaim the stamp.
    assert!(fixture.hub.validate_profile_assignment_owner(&incoming).is_ok());
    assert!(fixture.hub.validate_profile_assignment_owner(&fixture.receipt).is_err());
}

#[test]
fn a_reconnected_session_rejects_the_old_workers_captured_token() {
    let fixture = Fixture::new();
    fixture.hub.disconnect_connection("connection", fixture.hub.now_ms());
    let wire = |value| fixture.hub.handle_wire(value, "replacement", fixture.hub.now_ms());
    assert_eq!(wire(json!({"type":"auth","token":fixture.secret}))["ok"], true);
    let open = wire(json!({"type":"open","instanceId":"instance"}));
    assert_eq!(open["ok"], true);
    let baseline = json!({"version":1,"type":"baseline","clientId":open["clientId"],"sessionId":open["sessionId"],
        "generation":open["generation"],"publicationHash":open["publicationHash"],"sequence":"0",
        "values":fixture.panel.controls.iter().map(|control| (control.id.clone(), control.fallback())).collect::<BTreeMap<_,_>>()});
    assert_eq!(wire(baseline)["ok"], true);
    let current = fixture.hub.profile_feedback_token(&fixture.receipt).unwrap();
    assert_ne!(current.session_id, fixture.token.session_id);
    assert!(fixture.publish(Instant::now(), || true).is_err());
    assert_eq!(fixture.hub.profile_feedback_token(&fixture.receipt).unwrap(), current);
    assert!(fixture.hub.try_publish_profile_feedback(&fixture.receipt, &current, &[fixture.sample],
        FeedbackMode::Snapshot, Instant::now(), Duration::from_millis(250), || true).unwrap().is_some());
}
