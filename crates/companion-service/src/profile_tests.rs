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
