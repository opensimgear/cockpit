use super::*;

fn setup() -> (Hub, Arc<Sink>, Publication, Publication) {
    let sink = Arc::new(Sink::default());
    let hub = Hub::new("profile-host".into(), sink.clone());
    paired(&hub, "screen");
    let current = publish(&hub, fixture());
    let desired = publish(
        &hub,
        mutated(|document| document["ui"]["title"] = json!("Desired panel")),
    );
    assign(&hub, "screen", "instance", &current.publication_hash);
    (hub, sink, current, desired)
}

#[test]
fn prospective_panel_metadata_does_not_assign_or_admit_input() {
    let (hub, sink, current, desired) = setup();
    let before = hub.assignment_snapshot(&["instance".into()]).unwrap();
    let proof = hub
        .prepare_profile_assignment("instance", &desired.publication_hash)
        .unwrap();
    assert_eq!(proof.instance_id(), "instance");
    assert_eq!(proof.companion_id(), "screen");
    assert_eq!(proof.publication_hash(), desired.publication_hash);
    assert_eq!(
        proof.capability_fingerprint(),
        desired.capability_fingerprint
    );
    assert!(proof.identity_fingerprint().starts_with("sha256:"));
    assert_eq!(proof.definition(), desired.definition("instance"));
    assert_eq!(proof.feedback().len(), desired.feedback.len());
    let mut copy = proof.definition();
    copy.controls.clear();
    assert!(!proof.definition().controls.is_empty());
    assert_eq!(
        hub.assignment_snapshot(&["instance".into()]).unwrap(),
        before
    );
    assert_eq!(
        hub.assigned_publication("instance")
            .unwrap()
            .publication_hash,
        current.publication_hash
    );
    assert!(sink.owners.lock().unwrap().is_empty());
    assert!(sink.events.lock().unwrap().is_empty());
    hub.validate_profile_preparation(&proof).unwrap();
    assert!(format!("{proof:?}").contains("CompanionPreparation"));
}

#[test]
fn assignment_aba_and_uncertain_native_edits_expire_preparation() {
    let (hub, _, current, desired) = setup();
    let proof = hub
        .prepare_profile_assignment("instance", &desired.publication_hash)
        .unwrap();
    assign(&hub, "screen", "instance", &desired.publication_hash);
    assign(&hub, "screen", "instance", &current.publication_hash);
    assert!(hub.validate_profile_preparation(&proof).is_err());

    let storage = Arc::new(Memory::default());
    let persistent = Hub::with_storage(storage.clone(), Arc::new(Sink::default())).unwrap();
    paired(&persistent, "screen");
    let panel = publish(&persistent, fixture());
    assign(&persistent, "screen", "instance", &panel.publication_hash);
    let proof = persistent
        .prepare_profile_assignment("instance", &panel.publication_hash)
        .unwrap();
    storage.fail.store(true, Ordering::SeqCst);
    assert!(persistent
        .command(&json!({"command":"companion_update","companionId":"screen","name":"Edit"}))
        .is_err());
    assert!(persistent.validate_profile_preparation(&proof).is_err());
}

#[test]
fn proof_is_scoped_to_current_authorization_retained_panel_and_native_hub() {
    let (hub, _, _, desired) = setup();
    let proof = hub
        .prepare_profile_assignment("instance", &desired.publication_hash)
        .unwrap();
    let (other, _, _, _) = setup();
    assert!(other.validate_profile_preparation(&proof).is_err());
    assert!(hub
        .prepare_profile_assignment("missing", &desired.publication_hash)
        .is_err());
    assert!(hub
        .prepare_profile_assignment("bad id", &desired.publication_hash)
        .is_err());
    assert!(hub
        .prepare_profile_assignment("instance", "bad hash")
        .is_err());
    assert!(hub
        .prepare_profile_assignment("instance", &format!("sha256:{}", "0".repeat(64)))
        .is_err());
    operator(
        &hub,
        json!({"command":"companion_prune","hashes":[],"panelIds":[desired.ui_id]}),
    );
    assert!(hub.validate_profile_preparation(&proof).is_err());
    let current = hub.assigned_publication("instance").unwrap();
    let proof = hub
        .prepare_profile_assignment("instance", &current.publication_hash)
        .unwrap();
    operator(
        &hub,
        json!({"command":"companion_revoke","companionId":"screen"}),
    );
    assert!(hub.validate_profile_preparation(&proof).is_err());
}

#[derive(Default)]
struct ExistingStore {
    value: Mutex<Option<Vec<u8>>>,
    writes: std::sync::atomic::AtomicUsize,
}
impl persistence::StateStorage for ExistingStore {
    fn read(&self) -> Result<Option<Vec<u8>>, String> {
        Ok(self.value.lock().unwrap().clone())
    }
    fn write(&self, bytes: &[u8]) -> Result<(), String> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        *self.value.lock().unwrap() = Some(bytes.to_vec());
        Ok(())
    }
}

#[test]
fn existing_metadata_read_never_creates_or_replaces_protected_state() {
    let storage = Arc::new(ExistingStore::default());
    let sink = Arc::new(Sink::default());
    assert!(Hub::with_existing_storage(storage.clone(), sink.clone())
        .unwrap()
        .is_none());
    assert_eq!(storage.writes.load(Ordering::SeqCst), 0);
    *storage.value.lock().unwrap() = Some(b"invalid retained state".to_vec());
    assert!(Hub::with_existing_storage(storage.clone(), sink.clone()).is_err());
    assert_eq!(storage.writes.load(Ordering::SeqCst), 0);
    assert_eq!(
        storage.value.lock().unwrap().as_deref(),
        Some(b"invalid retained state".as_slice())
    );
    *storage.value.lock().unwrap() = None;
    let hub = Hub::with_storage(storage.clone(), sink.clone()).unwrap();
    paired(&hub, "screen");
    let publication = publish(&hub, fixture());
    assign(&hub, "screen", "instance", &publication.publication_hash);
    let writes = storage.writes.load(Ordering::SeqCst);
    let restored = Hub::with_existing_storage(storage.clone(), sink.clone())
        .unwrap()
        .unwrap();
    assert_eq!(restored.host_id(), hub.host_id());
    assert_eq!(storage.writes.load(Ordering::SeqCst), writes);
    assert_eq!(
        sink.defs.lock().unwrap()["instance"],
        publication.definition("instance")
    );
    assert!(sink.owners.lock().unwrap().is_empty());
    assert!(sink.events.lock().unwrap().is_empty());
}

fn capture(
    hub: &Hub,
    desired: &Publication,
    previous: Option<&profile::AssignmentReceipt>,
) -> profile::AssignmentReceipt {
    let proof = hub
        .prepare_profile_assignment("instance", &desired.publication_hash)
        .unwrap();
    hub.capture_profile_assignment(&proof, &uuid::Uuid::new_v4().to_string(), previous)
        .unwrap()
}
fn feedback(publication: &Publication) -> (FeedbackBinding, FeedbackSample) {
    let descriptor = publication
        .feedback
        .iter()
        .find(|descriptor| descriptor.kind == FeedbackKind::Boolean)
        .unwrap();
    let binding = FeedbackBinding {
        channel_id: descriptor.channel_id.clone(),
        output_id: descriptor.output_ids[0].clone(),
    };
    let sample = FeedbackSample {
        channel_id: binding.channel_id.clone(),
        output_id: binding.output_id.clone(),
        value: Some(FeedbackValue::Boolean(true)),
        status: FeedbackStatus::Fresh,
        provenance: FeedbackProvenance::Local,
        source_kind: FeedbackSource::Host,
        source_age_ms: 0,
        unit: "boolean".into(),
        range: None,
    };
    (binding, sample)
}

#[test]
fn native_takeover_inherits_original_baseline_and_stale_same_panel_owner_cannot_restore() {
    use profile::AssignmentRestore;
    let (hub, _, original, desired) = setup();
    let first = capture(&hub, &desired, None);
    assert_eq!(first.instance_id(), "instance");
    assert_eq!(first.publication_hash(), desired.publication_hash);
    assert_eq!(
        first.capability_fingerprint(),
        desired.capability_fingerprint
    );
    assert_eq!(first.revision(), 1);
    assert!(uuid::Uuid::parse_str(first.owner()).is_ok());
    hub.acquire_profile_assignment(&first).unwrap();
    hub.acquire_profile_assignment(&first).unwrap(); // Journal retry is idempotent.
    hub.validate_profile_assignment(&first).unwrap();
    let second = capture(&hub, &desired, Some(&first));
    assert_eq!(second.revision(), 2);
    assert_ne!(second.owner(), first.owner());
    hub.acquire_profile_assignment(&second).unwrap();
    assert!(hub.validate_profile_assignment(&first).is_err());
    assert_eq!(
        hub.restore_profile_assignment(&first).unwrap(),
        AssignmentRestore::OwnershipLost
    );
    hub.validate_profile_assignment(&second).unwrap();
    assert_eq!(
        hub.restore_profile_assignment(&second).unwrap(),
        AssignmentRestore::Restored
    );
    assert_eq!(
        hub.assigned_publication("instance")
            .unwrap()
            .publication_hash,
        original.publication_hash
    );
    assert_eq!(
        hub.restore_profile_assignment(&first).unwrap(),
        AssignmentRestore::AlreadyBaseline
    );
    assert_eq!(
        hub.restore_profile_assignment(&second).unwrap(),
        AssignmentRestore::AlreadyBaseline
    );
}

#[test]
fn profile_feedback_fences_all_legacy_paths_old_cleanup_and_real_source_sessions() {
    let sink = Arc::new(Sink::default());
    let hub = Hub::new("host".into(), sink.clone());
    let secret = paired(&hub, "screen");
    let peer_secret = paired(&hub, "peer-screen");
    let panel = publish(&hub, fixture());
    assign(&hub, "screen", "instance", &panel.publication_hash);
    assign(&hub, "peer-screen", "peer", &panel.publication_hash);
    let opened = connect(&hub, "old-connection", &secret, "instance");
    baseline(&hub, "old-connection", &opened, &panel);
    let peer = connect(&hub, "peer-connection", &peer_secret, "peer");
    baseline(&hub, "peer-connection", &peer, &panel);
    let (binding, sample) = feedback(&panel);
    hub.configure_feedback("instance", &panel.publication_hash, &[binding.clone()])
        .unwrap();
    let legacy = hub.feedback_token("instance").unwrap();
    let first = capture(&hub, &panel, None);
    hub.acquire_profile_assignment(&first).unwrap();
    assert!(!sink.owners.lock().unwrap().contains_key("instance"));
    assert!(sink.owners.lock().unwrap().contains_key("peer"));
    assert!(hub
        .configure_feedback("instance", &panel.publication_hash, &[binding.clone()])
        .is_err());
    assert!(hub.feedback_token("instance").is_err());
    assert!(hub
        .publish_feedback(
            &legacy,
            &[sample.clone()],
            FeedbackMode::Snapshot,
            hub.now_ms()
        )
        .is_err());
    assert!(hub
        .clear_feedback("instance", &panel.publication_hash, hub.now_ms())
        .is_err());
    let snapshot = hub.assignment_snapshot(&["instance".into()]).unwrap();
    assert!(hub.compare_assignments(&snapshot, &snapshot).is_err());
    hub.configure_profile_feedback(&first, &[binding.clone()])
        .unwrap();
    assert!(hub.profile_feedback_token(&first).is_err());
    let opened = connect(&hub, "new-connection", &secret, "instance");
    assert!(hub.profile_feedback_token(&first).is_err());
    baseline(&hub, "new-connection", &opened, &panel);
    let token = hub.profile_feedback_token(&first).unwrap();
    hub.publish_profile_feedback(
        &first,
        &token,
        &[sample.clone()],
        FeedbackMode::Snapshot,
        hub.now_ms(),
    )
    .unwrap();
    let second = capture(&hub, &panel, Some(&first));
    hub.acquire_profile_assignment(&second).unwrap();
    hub.disconnect_connection("old-connection", hub.now_ms());
    assert!(hub.clear_profile_feedback(&first, hub.now_ms()).is_err());
    assert!(hub
        .configure_profile_feedback(&first, &[binding.clone()])
        .is_err());
    assert!(hub
        .publish_profile_feedback(
            &first,
            &token,
            &[sample.clone()],
            FeedbackMode::Snapshot,
            hub.now_ms()
        )
        .is_err());
    hub.configure_profile_feedback(&second, &[binding]).unwrap();
    let opened = connect(&hub, "third-connection", &secret, "instance");
    baseline(&hub, "third-connection", &opened, &panel);
    let incoming = hub.profile_feedback_token(&second).unwrap();
    hub.publish_profile_feedback(
        &second,
        &incoming,
        &[sample],
        FeedbackMode::Snapshot,
        hub.now_ms(),
    )
    .unwrap();
    assert!(hub
        .clear_feedback("instance", &panel.publication_hash, hub.now_ms())
        .is_err());
    assert_eq!(hub.profile_feedback_token(&second).unwrap(), incoming);
    hub.clear_profile_feedback(&second, hub.now_ms()).unwrap();
    assert_ne!(
        hub.profile_feedback_token(&second)
            .unwrap()
            .feedback_generation,
        incoming.feedback_generation
    );
    assert_eq!(
        wire(&hub, "peer-connection", input(&peer, &panel, "1"))["ok"],
        true
    );
}

#[test]
fn protected_owner_survives_restart_and_tampered_recovery_cannot_redirect_baseline() {
    use profile::AssignmentRestore;
    let storage = Arc::new(Memory::default());
    let sink = Arc::new(Sink::default());
    let hub = Hub::with_storage(storage.clone(), sink.clone()).unwrap();
    paired(&hub, "screen");
    let original = publish(&hub, fixture());
    let desired = publish(
        &hub,
        mutated(|document| document["ui"]["title"] = json!("Profile owned")),
    );
    assign(&hub, "screen", "instance", &original.publication_hash);
    let receipt = capture(&hub, &desired, None);
    hub.acquire_profile_assignment(&receipt).unwrap();
    let encoded = serde_json::to_value(&receipt).unwrap();
    let receipt: profile::AssignmentReceipt = serde_json::from_value(encoded.clone()).unwrap();
    assert!(serde_json::from_value::<profile::AssignmentReceipt>(json!({"unknown":true})).is_err());
    let restarted = Hub::with_existing_storage(storage, sink.clone())
        .unwrap()
        .unwrap();
    publish(&restarted, fixture());
    publish(
        &restarted,
        mutated(|document| document["ui"]["title"] = json!("Profile owned")),
    );
    restarted.validate_profile_assignment(&receipt).unwrap();
    let mut tampered = encoded.clone();
    tampered["baseline"]["publicationHash"] = json!(desired.publication_hash);
    let tampered: profile::AssignmentReceipt = serde_json::from_value(tampered).unwrap();
    assert!(restarted.restore_profile_assignment(&tampered).is_err());
    let mut tampered = encoded;
    tampered["desired"]["ownership"]["owner"] = json!(uuid::Uuid::new_v4().to_string());
    let tampered: profile::AssignmentReceipt = serde_json::from_value(tampered).unwrap();
    assert_eq!(
        restarted.restore_profile_assignment(&tampered).unwrap(),
        AssignmentRestore::OwnershipLost
    );
    restarted.validate_profile_assignment(&receipt).unwrap();
    assert_eq!(
        restarted.restore_profile_assignment(&receipt).unwrap(),
        AssignmentRestore::Restored
    );
    assert!(sink.owners.lock().unwrap().is_empty());
}

#[test]
fn failed_acquisition_preserves_durable_assignment_and_operator_edit_revokes_owner() {
    use profile::AssignmentRestore;
    let storage = Arc::new(Memory::default());
    let hub = Hub::with_storage(storage.clone(), Arc::new(Sink::default())).unwrap();
    paired(&hub, "screen");
    let panel = publish(&hub, fixture());
    assign(&hub, "screen", "instance", &panel.publication_hash);
    let receipt = capture(&hub, &panel, None);
    let old = storage.value.lock().unwrap().clone();
    storage.fail.store(true, Ordering::SeqCst);
    assert!(hub.acquire_profile_assignment(&receipt).is_err());
    assert_eq!(*storage.value.lock().unwrap(), old);
    assert_eq!(
        hub.restore_profile_assignment(&receipt).unwrap(),
        AssignmentRestore::AlreadyBaseline
    );
    storage.fail.store(false, Ordering::SeqCst);
    hub.acquire_profile_assignment(&receipt).unwrap();
    let proof = hub
        .prepare_profile_assignment("instance", &panel.publication_hash)
        .unwrap();
    assert!(hub
        .capture_profile_assignment(&proof, &uuid::Uuid::new_v4().to_string(), None)
        .is_err());
    assert!(hub
        .capture_profile_assignment(&proof, receipt.owner(), Some(&receipt))
        .is_err());
    assert!(hub
        .capture_profile_assignment(&proof, "browser-owner", None)
        .is_err());
    assign(&hub, "screen", "instance", &panel.publication_hash);
    assert!(hub.validate_profile_assignment(&receipt).is_err());
    assert_eq!(
        hub.restore_profile_assignment(&receipt).unwrap(),
        AssignmentRestore::AlreadyBaseline
    );
    hub.configure_feedback("instance", &panel.publication_hash, &[])
        .unwrap();
}

#[test]
fn maximum_native_panel_uses_compact_bounded_journal_references_and_keeps_baseline_retained() {
    let storage = Arc::new(Memory::default());
    let hub = Hub::with_storage(storage.clone(), Arc::new(Sink::default())).unwrap();
    let companion = "c".repeat(64);
    let instance = "i".repeat(64);
    paired(&hub, &companion);
    let large = |title: &str| {
        mutated(|document| {
            document["ui"]["title"] = json!(title);
            let mut controls = vec![];
            let mut widgets = vec![];
            let mut items = vec![];
            let mut bindings = vec![];
            for index in 0..128 {
                let id = format!("control-{index:03}-{}", "x".repeat(52));
                let bounds = json!({"x":0.0,"y":0.0,"width":0.1,"height":0.1});
                controls.push(json!({"id":id,"kind":"button","label":"L".repeat(120)}));
                widgets.push(json!({"id":format!("widget-{index:03}-{}","x".repeat(53)),"pageId":"flight-page-0",
                "kind":"button","label":"L".repeat(120),"bounds":bounds,"controlIds":[id],"feedbackIds":[],
                "config":{"buttonMode":"momentary","pulseMs":0,"repeatMs":0}}));
                items.push(json!({"kind":"control","refId":id,"bounds":bounds}));
                bindings.push(json!({"controlId":id,"targetId":"flight-target-0","kind":"button","index":index}));
            }
            document["ui"]["controls"] = json!(controls);
            document["ui"]["feedbackWidgets"] = json!([]);
            document["ui"]["pages"][0]["items"] = json!(items);
            document["presentation"]["widgets"] = json!(widgets);
            document["targets"][0]["buttons"] = json!(128);
            document["targets"][0]["axes"] = json!([]);
            document["targets"][0]["hats"] = json!(0);
            document["bindings"] = json!(bindings);
        })
    };
    let baseline_upload = large(&"B".repeat(120));
    let desired_upload = large(&"D".repeat(120));
    let baseline = publish(&hub, baseline_upload.clone());
    let desired = publish(&hub, desired_upload.clone());
    assert_eq!(desired.controls.len(), 128);
    assert!(
        serde_json::to_vec(&desired.definition(&instance))
            .unwrap()
            .len()
            > 8192
    );
    assign(&hub, &companion, &instance, &baseline.publication_hash);
    let proof = hub
        .prepare_profile_assignment(&instance, &desired.publication_hash)
        .unwrap();
    let first = hub
        .capture_profile_assignment(&proof, &uuid::Uuid::new_v4().to_string(), None)
        .unwrap();
    assert!(serde_json::to_vec(&first).unwrap().len() <= 8192);
    hub.acquire_profile_assignment(&first).unwrap();
    let proof = hub
        .prepare_profile_assignment(&instance, &desired.publication_hash)
        .unwrap();
    let second = hub
        .capture_profile_assignment(&proof, &uuid::Uuid::new_v4().to_string(), Some(&first))
        .unwrap();
    let encoded = serde_json::to_vec(&second).unwrap();
    assert!(encoded.len() <= 8192);
    assert!(!String::from_utf8(encoded.clone())
        .unwrap()
        .contains("definition"));
    hub.acquire_profile_assignment(&second).unwrap();
    operator(
        &hub,
        json!({"command":"companion_prune","hashes":[],"panelIds":[desired.ui_id]}),
    );
    assert!(hub
        .publication_uploads()
        .iter()
        .any(|panel| panel.publication_hash == baseline.publication_hash));
    let restarted = Hub::with_existing_storage(storage, Arc::new(Sink::default()))
        .unwrap()
        .unwrap();
    publish(&restarted, baseline_upload);
    publish(&restarted, desired_upload);
    let restored: profile::AssignmentReceipt = serde_json::from_slice(&encoded).unwrap();
    restarted.validate_profile_assignment(&restored).unwrap();
    assert_eq!(
        restarted.restore_profile_assignment(&restored).unwrap(),
        profile::AssignmentRestore::Restored
    );
    assert_eq!(
        restarted
            .assigned_publication(&instance)
            .unwrap()
            .publication_hash,
        baseline.publication_hash
    );
}

#[test]
fn missing_panel_cache_preserves_owned_recovery_intent_and_still_allows_neutralization() {
    let storage = Arc::new(Memory::default());
    let hub = Hub::with_storage(storage.clone(), Arc::new(Sink::default())).unwrap();
    paired(&hub, "screen");
    let original = publish(&hub, fixture());
    let desired = publish(
        &hub,
        mutated(|document| document["ui"]["title"] = json!("Owned revision")),
    );
    assign(&hub, "screen", "instance", &original.publication_hash);
    let receipt = capture(&hub, &desired, None);
    hub.acquire_profile_assignment(&receipt).unwrap();
    let restarted = Hub::with_existing_storage(storage.clone(), Arc::new(Sink::default()))
        .unwrap()
        .unwrap();
    let before = storage.value.lock().unwrap().clone();
    restarted
        .validate_profile_assignment_owner(&receipt)
        .unwrap();
    assert!(restarted.validate_profile_assignment(&receipt).is_err());
    assert!(
        restarted.restore_profile_assignment(&receipt).is_err(),
        "A missing panel must retain RecoveryNeeded rather than pretend ownership was lost"
    );
    assert_eq!(*storage.value.lock().unwrap(), before);
    restarted
        .clear_profile_feedback(&receipt, restarted.now_ms())
        .unwrap();
    assert!(restarted.configure_profile_feedback(&receipt, &[]).is_err());
    publish(&restarted, fixture());
    assert!(
        restarted.restore_profile_assignment(&receipt).is_err(),
        "Both current and original immutable descriptors must be verified"
    );
    publish(
        &restarted,
        mutated(|document| document["ui"]["title"] = json!("Owned revision")),
    );
    assert_eq!(
        restarted.restore_profile_assignment(&receipt).unwrap(),
        profile::AssignmentRestore::Restored
    );
    assert_eq!(
        restarted
            .assigned_publication("instance")
            .unwrap()
            .publication_hash,
        original.publication_hash
    );
}

#[derive(Default)]
struct RemovableSink {
    source: Sink,
    reject_removal: AtomicBool,
}
impl SourceSink for RemovableSink {
    fn register(&self, definition: SourceDefinition) -> Result<(), String> {
        self.source.register(definition)
    }
    fn unregister(&self, instance: &str) -> Result<(), String> {
        if self.reject_removal.load(Ordering::SeqCst) {
            return Err("Descriptor removal failed".into());
        }
        self.source.defs.lock().unwrap().remove(instance);
        self.source.owners.lock().unwrap().remove(instance);
        Ok(())
    }
    fn baseline(
        &self,
        token: &SourceToken,
        sequence: u64,
        changes: &[SourceChange],
    ) -> Result<(), String> {
        self.source.baseline(token, sequence, changes)
    }
    fn changes(
        &self,
        token: &SourceToken,
        sequence: u64,
        changes: &[SourceChange],
    ) -> Result<(), String> {
        self.source.changes(token, sequence, changes)
    }
    fn loss(&self, token: &SourceToken) -> Result<(), String> {
        self.source.loss(token)
    }
}

fn capture_first(
    hub: &Hub,
    panel: &Publication,
    previous: Option<&profile::AssignmentReceipt>,
) -> profile::AssignmentReceipt {
    let proof = hub
        .prepare_profile_assignment(
            &profile::profile_instance_id("screen"),
            &panel.publication_hash,
        )
        .unwrap();
    hub.capture_profile_assignment(&proof, &uuid::Uuid::new_v4().to_string(), previous)
        .unwrap()
}

#[test]
fn first_assignment_preparation_requires_an_unassigned_paired_installation_and_is_passive() {
    let sink = Arc::new(RemovableSink::default());
    let hub = Hub::new("profile-host".into(), sink.clone());
    let panel = publish(&hub, fixture());
    let instance = profile::profile_instance_id("screen");
    assert!(id(&instance));
    assert_ne!(instance, profile::profile_instance_id("peer"));
    assert!(hub
        .prepare_profile_assignment(&instance, &panel.publication_hash)
        .is_err());
    paired(&hub, "screen");
    let proof = hub
        .prepare_profile_assignment(&instance, &panel.publication_hash)
        .unwrap();
    let receipt = capture_first(&hub, &panel, None);
    let encoded = serde_json::to_value(receipt).unwrap();
    assert!(encoded["expected"].is_null());
    assert!(encoded["baseline"].is_null());
    assert_eq!(proof.companion_id(), "screen");
    assert_eq!(proof.definition(), panel.definition(&instance));
    assert!(hub.assigned_publication(&instance).is_err());
    assert!(hub.resource_instance_ids().is_empty());
    assert!(sink.source.defs.lock().unwrap().is_empty());
    assert!(sink.source.events.lock().unwrap().is_empty());
    assign(&hub, "screen", "operator-instance", &panel.publication_hash);
    assert!(hub.validate_profile_preparation(&proof).is_err());
    assert!(hub
        .prepare_profile_assignment(&instance, &panel.publication_hash)
        .is_err());
    assert!(hub
        .prepare_profile_assignment(
            &profile::profile_instance_id("not-paired"),
            &panel.publication_hash
        )
        .is_err());
}

#[test]
fn first_assignment_takeover_preserves_absence_and_removes_only_the_exact_owned_source() {
    use profile::AssignmentRestore;
    let sink = Arc::new(RemovableSink::default());
    let hub = Hub::new("profile-host".into(), sink.clone());
    let secret = paired(&hub, "screen");
    paired(&hub, "peer");
    let panel = publish(&hub, fixture());
    let other = publish(
        &hub,
        mutated(|document| document["ui"]["title"] = json!("Other panel")),
    );
    assign(&hub, "peer", "peer-instance", &panel.publication_hash);
    let first = capture_first(&hub, &panel, None);
    hub.acquire_profile_assignment(&first).unwrap();
    hub.acquire_profile_assignment(&first).unwrap();
    assert!(
        hub.profile_feedback_token(&first).is_err(),
        "Prospective assignment is not a live reader"
    );
    let opened = connect(&hub, "first-live", &secret, first.instance_id());
    assert_eq!(baseline(&hub, "first-live", &opened, &panel)["ok"], true);
    assert!(sink
        .source
        .owners
        .lock()
        .unwrap()
        .contains_key(first.instance_id()));
    let second = capture_first(&hub, &other, Some(&first));
    assert_eq!(second.revision(), 2);
    assert!(serde_json::to_value(&second).unwrap()["baseline"].is_null());
    hub.acquire_profile_assignment(&second).unwrap();
    assert!(!sink
        .source
        .owners
        .lock()
        .unwrap()
        .contains_key(first.instance_id()));
    assert!(hub.acquire_profile_assignment(&first).is_err());
    assert_eq!(
        hub.restore_profile_assignment(&first).unwrap(),
        AssignmentRestore::OwnershipLost
    );
    hub.validate_profile_assignment(&second).unwrap();
    assert_eq!(
        hub.restore_profile_assignment(&second).unwrap(),
        AssignmentRestore::Restored
    );
    assert!(hub.assigned_publication(second.instance_id()).is_err());
    assert!(!sink
        .source
        .defs
        .lock()
        .unwrap()
        .contains_key(second.instance_id()));
    assert!(sink
        .source
        .defs
        .lock()
        .unwrap()
        .contains_key("peer-instance"));
    assert_eq!(
        hub.assigned_publication("peer-instance")
            .unwrap()
            .publication_hash,
        panel.publication_hash
    );
    assert_eq!(
        hub.restore_profile_assignment(&second).unwrap(),
        AssignmentRestore::AlreadyBaseline
    );
    assert_eq!(
        hub.restore_profile_assignment(&first).unwrap(),
        AssignmentRestore::AlreadyBaseline
    );
    assert!(wire(&hub, "first-live", input(&opened, &panel, "1"))["ok"] != true);
    hub.prepare_profile_assignment(second.instance_id(), &panel.publication_hash)
        .unwrap();
}

#[test]
fn first_assignment_storage_and_descriptor_failures_preserve_durable_authority_for_retry() {
    use profile::AssignmentRestore;
    let storage = Arc::new(Memory::default());
    let sink = Arc::new(RemovableSink::default());
    let hub = Hub::with_storage(storage.clone(), sink.clone()).unwrap();
    let secret = paired(&hub, "screen");
    let panel = publish(&hub, fixture());
    let receipt = capture_first(&hub, &panel, None);
    let before = storage.value.lock().unwrap().clone();
    storage.fail.store(true, Ordering::SeqCst);
    assert!(hub.acquire_profile_assignment(&receipt).is_err());
    assert_eq!(*storage.value.lock().unwrap(), before);
    assert!(hub.assigned_publication(receipt.instance_id()).is_err());
    assert!(!sink
        .source
        .defs
        .lock()
        .unwrap()
        .contains_key(receipt.instance_id()));
    assert_eq!(
        hub.restore_profile_assignment(&receipt).unwrap(),
        AssignmentRestore::AlreadyBaseline
    );
    storage.fail.store(false, Ordering::SeqCst);
    hub.acquire_profile_assignment(&receipt).unwrap();
    let opened = connect(&hub, "live", &secret, receipt.instance_id());
    baseline(&hub, "live", &opened, &panel);
    let owned = storage.value.lock().unwrap().clone();
    storage.fail.store(true, Ordering::SeqCst);
    assert!(hub.restore_profile_assignment(&receipt).is_err());
    assert_eq!(*storage.value.lock().unwrap(), owned);
    assert!(!sink
        .source
        .owners
        .lock()
        .unwrap()
        .contains_key(receipt.instance_id()));
    assert!(sink
        .source
        .defs
        .lock()
        .unwrap()
        .contains_key(receipt.instance_id()));
    hub.validate_profile_assignment(&receipt).unwrap();
    storage.fail.store(false, Ordering::SeqCst);
    sink.reject_removal.store(true, Ordering::SeqCst);
    assert!(hub.restore_profile_assignment(&receipt).is_err());
    assert_eq!(*storage.value.lock().unwrap(), owned);
    hub.validate_profile_assignment(&receipt).unwrap();
    sink.reject_removal.store(false, Ordering::SeqCst);
    assert_eq!(
        hub.restore_profile_assignment(&receipt).unwrap(),
        AssignmentRestore::Restored
    );
    assert!(!sink
        .source
        .defs
        .lock()
        .unwrap()
        .contains_key(receipt.instance_id()));
}

#[test]
fn first_assignment_receipt_is_installation_scoped_and_restart_can_restore_absence_without_panel_cache(
) {
    use profile::AssignmentRestore;
    let storage = Arc::new(Memory::default());
    let hub = Hub::with_storage(storage.clone(), Arc::new(RemovableSink::default())).unwrap();
    paired(&hub, "screen");
    let panel = publish(&hub, fixture());
    let stale = capture_first(&hub, &panel, None);
    operator(
        &hub,
        json!({"command":"companion_revoke","companionId":"screen"}),
    );
    paired(&hub, "screen");
    assert!(hub.acquire_profile_assignment(&stale).is_err());
    let receipt = capture_first(&hub, &panel, None);
    let encoded = serde_json::to_value(&receipt).unwrap();
    hub.acquire_profile_assignment(&receipt).unwrap();
    let sink = Arc::new(RemovableSink::default());
    let restarted = Hub::with_existing_storage(storage, sink.clone())
        .unwrap()
        .unwrap();
    restarted
        .validate_profile_assignment_owner(&receipt)
        .unwrap();
    assert!(restarted.validate_profile_assignment(&receipt).is_err());
    let decoded: profile::AssignmentReceipt = serde_json::from_value(encoded.clone()).unwrap();
    let mut tampered = encoded;
    tampered["desired"]["ownership"]["baselineHash"] = json!(format!("sha256:{}", "0".repeat(64)));
    let tampered: profile::AssignmentReceipt = serde_json::from_value(tampered).unwrap();
    assert!(
        restarted.restore_profile_assignment(&tampered).unwrap()
            == AssignmentRestore::OwnershipLost
    );
    assert_eq!(
        restarted.restore_profile_assignment(&decoded).unwrap(),
        AssignmentRestore::Restored
    );
    assert!(restarted
        .assigned_publication(receipt.instance_id())
        .is_err());
    assert!(!sink
        .source
        .defs
        .lock()
        .unwrap()
        .contains_key(receipt.instance_id()));
    assert_eq!(
        restarted.restore_profile_assignment(&decoded).unwrap(),
        AssignmentRestore::AlreadyBaseline
    );
}

#[test]
fn first_assignment_cannot_overwrite_a_new_operator_assignment_captured_after_preparation() {
    let hub = Hub::new("profile-host".into(), Arc::new(RemovableSink::default()));
    paired(&hub, "screen");
    let panel = publish(&hub, fixture());
    let receipt = capture_first(&hub, &panel, None);
    assign(&hub, "screen", "operator-instance", &panel.publication_hash);
    assert!(hub.acquire_profile_assignment(&receipt).is_err());
    assert!(hub.assigned_publication(receipt.instance_id()).is_err());
    assert_eq!(
        hub.assigned_publication("operator-instance")
            .unwrap()
            .publication_hash,
        panel.publication_hash
    );
}
