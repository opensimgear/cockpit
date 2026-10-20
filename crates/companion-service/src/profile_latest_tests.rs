use super::*;
use crate::profile::AssignmentRestore;

fn capture_latest(hub: &Hub, panel: &Publication, previous: Option<&profile::AssignmentReceipt>) -> profile::AssignmentReceipt {
    let proof = hub.prepare_profile_assignment("instance", &panel.publication_hash).unwrap();
    hub.capture_profile_assignment(&proof, &uuid::Uuid::new_v4().to_string(), previous).unwrap()
}
fn updated(hub: &Hub, title: &str) -> Publication {
    publish(hub, mutated(|document| document["ui"]["title"] = json!(title)))
}

#[test]
fn restore_follows_latest_saved_original_panel_without_reusing_a_live_session() {
    let sink = Arc::new(Sink::default()); let hub = Hub::new("host".into(), sink.clone());
    let secret = paired(&hub, "screen"); let original = publish(&hub, fixture());
    let desired = updated(&hub, "Profile panel"); let latest = updated(&hub, "Latest saved original panel");
    assign(&hub, "screen", "instance", &original.publication_hash);
    let first = capture_latest(&hub, &desired, None); hub.acquire_profile_assignment(&first).unwrap();
    let second = capture_latest(&hub, &desired, Some(&first)); hub.acquire_profile_assignment(&second).unwrap();
    let open = connect(&hub, "active", &secret, "instance"); assert_eq!(baseline(&hub, "active", &open, &desired)["ok"], true);
    let heads = BTreeMap::from([(original.ui_id.clone(), latest.publication_hash.clone())]);
    assert_eq!(hub.restore_profile_assignment_latest(&first, &heads).unwrap(), AssignmentRestore::OwnershipLost);
    assert_eq!(hub.assigned_publication("instance").unwrap().publication_hash, desired.publication_hash);
    assert_eq!(hub.restore_profile_assignment_latest(&second, &heads).unwrap(), AssignmentRestore::Restored);
    assert_eq!(hub.assigned_publication("instance").unwrap().publication_hash, latest.publication_hash);
    assert!(sink.owners.lock().unwrap().is_empty());
    assert_eq!(sink.defs.lock().unwrap()["instance"], latest.definition("instance"));
    assert_eq!(wire(&hub, "active", frame(&open, "heartbeat", "1"))["ok"], false);
    assert_eq!(hub.restore_profile_assignment_latest(&second, &heads).unwrap(), AssignmentRestore::OwnershipLost);
    let fresh = connect(&hub, "fresh", &secret, "instance"); assert_ne!(fresh["sessionId"], open["sessionId"]);
    assert_eq!(fresh["publicationHash"], latest.publication_hash);
    assert!(sink.owners.lock().unwrap().is_empty());
    assert_eq!(baseline(&hub, "fresh", &fresh, &latest)["ok"], true);
}

#[test]
fn latest_restore_checks_original_receipt_before_head_lookup_and_rejects_other_panel_identity() {
    let sink = Arc::new(Sink::default()); let hub = Hub::new("host".into(), sink.clone());
    paired(&hub, "screen"); let original = publish(&hub, fixture()); let desired = updated(&hub, "Profile panel");
    let different = publish(&hub, mutated(|document| {
        document["ui"]["id"] = json!("another-stable-panel"); document["ui"]["title"] = json!("Unrelated panel");
    }));
    assign(&hub, "screen", "instance", &original.publication_hash);
    let receipt = capture_latest(&hub, &desired, None); hub.acquire_profile_assignment(&receipt).unwrap();
    let wrong = BTreeMap::from([(original.ui_id.clone(), different.publication_hash.clone())]);
    assert!(hub.restore_profile_assignment_latest(&receipt, &wrong).unwrap_err().contains("different stable identity"));
    hub.validate_profile_assignment(&receipt).unwrap();
    let missing = BTreeMap::from([(original.ui_id.clone(), format!("sha256:{}", "f".repeat(64)))]);
    assert!(hub.restore_profile_assignment_latest(&receipt, &missing).unwrap_err().contains("latest saved companion panel"));
    let mut tampered = serde_json::to_value(&receipt).unwrap(); tampered["baseline"]["publicationHash"] = json!(different.publication_hash);
    let tampered = serde_json::from_value(tampered).unwrap();
    assert!(hub.restore_profile_assignment_latest(&tampered, &wrong).is_err());
    assign(&hub, "screen", "instance", &different.publication_hash); // A newer operator edit supersedes the protected owner.
    assert_eq!(hub.restore_profile_assignment_latest(&receipt, &missing).unwrap(), AssignmentRestore::OwnershipLost);
    assert_eq!(hub.assigned_publication("instance").unwrap().publication_hash, different.publication_hash);
    assert!(sink.owners.lock().unwrap().is_empty());
}

#[test]
fn guarded_unowned_assignment_cannot_replace_changed_or_owned_assignment() {
    let sink = Arc::new(Sink::default()); let hub = Hub::new("host".into(), sink.clone());
    let secret = paired(&hub, "screen"); let original = publish(&hub, fixture()); let latest = updated(&hub, "Latest saved panel");
    let guarded = |expected: &str| json!({"command":"companion_assign", "instanceId":"instance", "companionId":"screen",
        "publicationHash":latest.publication_hash, "expectedUnownedPublicationHash":expected});
    assert!(hub.command(&guarded(&original.publication_hash)).is_err()); // No original assignment to compare.
    assign(&hub, "screen", "instance", &original.publication_hash);
    assert!(hub.command(&guarded("bad")).is_err());
    let open = connect(&hub, "original", &secret, "instance"); assert_eq!(baseline(&hub, "original", &open, &original)["ok"], true);
    let receipt = capture_latest(&hub, &original, None); hub.acquire_profile_assignment(&receipt).unwrap();
    let owned = connect(&hub, "owned", &secret, "instance"); assert_eq!(baseline(&hub, "owned", &owned, &original)["ok"], true);
    let before = operator(&hub, json!({"command":"companion_status"}));
    assert!(hub.command(&guarded(&original.publication_hash)).unwrap_err().contains("owned by a profile"));
    hub.validate_profile_assignment(&receipt).unwrap();
    assert_eq!(operator(&hub, json!({"command":"companion_status"})), before);
    assert!(sink.owners.lock().unwrap().contains_key("instance"));
    hub.restore_profile_assignment(&receipt).unwrap();
    assert_eq!(hub.command(&guarded(&original.publication_hash)).unwrap()["assigned"], true);
    assert_eq!(hub.assigned_publication("instance").unwrap().publication_hash, latest.publication_hash);
    assert!(hub.command(&guarded(&original.publication_hash)).is_err()); // A delayed save cannot replace a newer hash.
    assert_eq!(hub.assigned_publication("instance").unwrap().publication_hash, latest.publication_hash);
}
