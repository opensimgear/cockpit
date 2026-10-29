use super::*;
use std::time::{Duration, Instant};

fn fixture() -> (Hub, AssignmentReceipt, String) {
    let hub = Hub::new("confirmed-owner".into(), Arc::new(NullSourceSink));
    let offer = hub.command(&json!({"command":"companion_pair_offer"})).unwrap();
    let request = hub.handle_wire(json!({"type":"pair","secret":offer["secret"],
        "companionId":"screen","name":"Screen"}), "connection", hub.now_ms());
    hub.command(&json!({"command":"companion_approve","requestId":request["requestId"],"companionId":"screen"})).unwrap();
    let upload: PublicationUpload = serde_json::from_str(include_str!("../../tests/fixtures/c1-flight-publication.json")).unwrap();
    let hash = upload.publication_hash.clone();
    hub.retain_legacy_test_fixture(upload).unwrap();
    hub.command(&json!({"command":"companion_assign","instanceId":"instance",
        "companionId":"screen","publicationHash":hash})).unwrap();
    let proof = hub.prepare_profile_assignment("instance", &hash).unwrap();
    let receipt = hub.capture_profile_assignment(&proof, &uuid::Uuid::new_v4().to_string(), None).unwrap();
    hub.acquire_profile_assignment(&receipt).unwrap();
    (hub, receipt, hash)
}

#[test]
fn only_the_exact_strictly_confirmed_receipt_survives_a_busy_hub() {
    let (hub, receipt, _) = fixture();
    let held = hub.inner.lock().unwrap();
    assert_eq!(hub.try_validate_profile_assignment(&receipt).unwrap(), None);
    drop(held);
    hub.validate_profile_assignment(&receipt).unwrap();
    let held = hub.inner.lock().unwrap();
    let started = Instant::now();
    for _ in 0..100 { assert_eq!(hub.try_validate_profile_assignment(&receipt).unwrap(), Some(())); }
    assert!(started.elapsed() < Duration::from_millis(100));
    let mut modified = serde_json::to_value(&receipt).unwrap();
    modified["capabilityFingerprint"] = json!(format!("sha256:{}", "b".repeat(64)));
    let modified: AssignmentReceipt = serde_json::from_value(modified).unwrap();
    assert_eq!(hub.try_validate_profile_assignment(&modified).unwrap(), None);
    drop(held);
    assert!(hub.try_validate_profile_assignment(&modified).is_err());
}

#[test]
fn a_record_edit_requires_revalidation_before_the_busy_path_can_reuse_authority() {
    let (hub, receipt, _) = fixture();
    hub.validate_profile_assignment(&receipt).unwrap();
    hub.command(&json!({"command":"companion_update","companionId":"screen","name":"Renamed"})).unwrap();
    let held = hub.inner.lock().unwrap();
    assert_eq!(hub.try_validate_profile_assignment(&receipt).unwrap(), None);
    drop(held);
    assert_eq!(hub.try_validate_profile_assignment(&receipt).unwrap(), Some(()));
    let _held = hub.inner.lock().unwrap();
    assert_eq!(hub.try_validate_profile_assignment(&receipt).unwrap(), Some(()));
}

#[test]
fn replacement_and_revoke_never_follow_the_old_owners_confirmation() {
    let (hub, outgoing, hash) = fixture();
    hub.validate_profile_assignment(&outgoing).unwrap();
    let proof = hub.prepare_profile_assignment("instance", &hash).unwrap();
    let incoming = hub.capture_profile_assignment(&proof, &uuid::Uuid::new_v4().to_string(), Some(&outgoing)).unwrap();
    hub.acquire_profile_assignment(&incoming).unwrap();
    hub.validate_profile_assignment(&incoming).unwrap();
    let held = hub.inner.lock().unwrap();
    assert_eq!(hub.try_validate_profile_assignment(&outgoing).unwrap(), None);
    assert_eq!(hub.try_validate_profile_assignment(&incoming).unwrap(), Some(()));
    drop(held);
    assert!(hub.try_validate_profile_assignment(&outgoing).is_err());
    hub.command(&json!({"command":"companion_revoke","companionId":"screen"})).unwrap();
    let held = hub.inner.lock().unwrap();
    assert_eq!(hub.try_validate_profile_assignment(&incoming).unwrap(), None);
    drop(held);
    assert!(hub.try_validate_profile_assignment(&incoming).is_err());
}

#[test]
fn confirmed_cache_is_bounded_and_never_adds_a_lock_wait_to_live_validation() {
    let (hub, receipt, _) = fixture();
    {
        let revision = hub.record_revision.load(Ordering::Acquire);
        let mut cache = hub.confirmed_assignments.lock().unwrap();
        for index in 0..MAX_CONFIRMED { cache.insert(index.to_string(), revision); }
    }
    hub.validate_profile_assignment(&receipt).unwrap();
    assert_eq!(hub.confirmed_assignments.lock().unwrap().len(), 1);
    let cache = hub.confirmed_assignments.lock().unwrap();
    // Strict validation is still available; an optional cache insert never waits.
    assert_eq!(hub.try_validate_profile_assignment(&receipt).unwrap(), Some(()));
    let _hub = hub.inner.lock().unwrap();
    let started = Instant::now();
    assert_eq!(hub.try_validate_profile_assignment(&receipt).unwrap(), None);
    assert!(started.elapsed() < Duration::from_millis(100));
    drop(cache);
}
