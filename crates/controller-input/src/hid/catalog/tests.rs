use super::*;
use serde_json::json;

fn windows() -> Value {
    json!({"interfaceId":"first", "product":"stick", "attributes":{"vendorId":1,"productId":2},
        "capabilities":{"provenance":"windows-hidp-preparsed-capabilities","available":true,
            "reports":[{"kind":"input","valueCaps":[{"usage":48}]}],"links":[{"index":0}]}})
}

#[test]
fn restart_retains_metadata_without_any_live_state() {
    let dir = crate::test_directory();
    let path = dir.path().join("controllers.json");
    let mut catalog = Catalog::new(Some(&path)).unwrap();
    let mut detail = windows();
    detail["samples"] = json!([{"value":123}]);
    detail["generation"] = json!("live-generation");
    let observed = catalog.observe("first", &detail).unwrap();
    assert_eq!(observed["availability"], "offline");
    assert!(observed["detail"].get("samples").is_none());
    assert!(observed["detail"].get("generation").is_none());
    let restarted = Catalog::new(Some(&path)).unwrap();
    assert_eq!(restarted.snapshot("first").unwrap(), observed);
    assert_eq!(
        restarted
            .snapshot(observed["snapshotId"].as_str().unwrap())
            .unwrap(),
        observed
    );
    assert!(restarted.snapshots()[0].get("detail").is_none());
    assert_eq!(restarted.snapshots()[0]["status"], "last-observed");
}

#[test]
fn replacement_is_whole_contract_and_siblings_remain_separate() {
    let mut catalog = Catalog::new(None).unwrap();
    let first = catalog.observe("first", &windows()).unwrap();
    let sibling = catalog.observe("sibling", &json!({"interfaceId":"sibling", "attributes":{"vendorId":1,"productId":2},
        "capabilities":{"provenance":"windows-hidp-preparsed-capabilities","reports":[{"kind":"feature"}]}})).unwrap();
    let changed = catalog
        .observe(
            "first",
            &json!({"interfaceId":"first", "capabilities":{
        "provenance":"windows-hidp-preparsed-capabilities","reports":[],"links":[]}}),
        )
        .unwrap();
    assert_ne!(first["contractFingerprint"], changed["contractFingerprint"]);
    assert_eq!(first["snapshotId"], changed["snapshotId"]);
    assert!(changed["detail"].get("attributes").is_none());
    assert!(changed["detail"].get("product").is_none());
    assert_eq!(changed["detail"]["capabilities"]["reports"], json!([]));
    assert_eq!(catalog.snapshot("sibling").unwrap(), sibling);
    assert_eq!(catalog.snapshots().len(), 2);
}

#[test]
fn exact_native_identity_replaces_generation_and_invalidates_old_handle() {
    let mut catalog = Catalog::new(None).unwrap();
    let metadata = json!({"interfaceId":"first","sysfsPath":"/sys/interface0/hidraw0",
        "hidParent":"/sys/interface0","path":"/dev/hidraw0","descriptorHex":"0501"});
    let initial = catalog.observe("first", &metadata).unwrap();
    let mut next = metadata;
    next["interfaceId"] = json!("second");
    next["descriptorHex"] = json!("05010904");
    let replacement = catalog.observe("second", &next).unwrap();
    assert_eq!(replacement["previousInterfaceId"], "first");
    assert_eq!(replacement["snapshotId"], initial["snapshotId"]);
    assert_ne!(
        replacement["contractFingerprint"],
        initial["contractFingerprint"]
    );
    assert!(catalog.snapshot("first").is_none());
    assert_eq!(catalog.snapshots().len(), 1);
}

#[test]
fn failed_database_commit_does_not_publish_new_metadata() {
    let dir = crate::test_directory();
    let path = dir.path().join("controllers.json");
    let mut first = Catalog::new(Some(&path)).unwrap();
    let mut stale = Catalog::new(Some(&path)).unwrap();
    first.observe("first", &windows()).unwrap();
    assert!(stale
        .observe("first", &windows())
        .unwrap_err()
        .contains("revision conflict"));
    assert!(stale.snapshots().is_empty());
    assert_eq!(Catalog::new(Some(&path)).unwrap().snapshots().len(), 1);
}

#[test]
fn descriptor_identity_ignores_labels_and_samples_but_preserves_provenance() {
    let mut catalog = Catalog::new(None).unwrap();
    let before = catalog.observe("first", &windows()).unwrap();
    let mut after = windows();
    after["product"] = json!("Renamed stick");
    after["samples"] = json!([1, 2]);
    let after = catalog.observe("first", &after).unwrap();
    assert_eq!(before["contractFingerprint"], after["contractFingerprint"]);
    assert_eq!(
        after["provenance"]["capabilities"],
        "windows-hidp-preparsed-capabilities"
    );
    assert!(after["detail"].get("descriptorHex").is_none());
}

#[test]
fn only_previously_observed_exact_identity_needs_refresh_on_handle_change() {
    let mut catalog = Catalog::new(None).unwrap();
    let detail =
        json!({"interfaceId":"first","pathFingerprint":"collection-one","capabilities":{}});
    assert!(!catalog.needs_refresh("first", &detail));
    catalog.observe("first", &detail).unwrap();
    assert!(!catalog.needs_refresh("first", &detail));
    assert!(catalog.needs_refresh(
        "new",
        &json!({"interfaceId":"new","pathFingerprint":"collection-one"})
    ));
    assert!(!catalog.needs_refresh(
        "sibling",
        &json!({"interfaceId":"sibling","pathFingerprint":"collection-two"})
    ));
}

#[test]
fn capacity_rejection_preserves_existing_records_and_allows_replacement() {
    let mut catalog = Catalog::new(None).unwrap();
    for index in 0..MAX_SNAPSHOTS {
        catalog
            .observe_at(
                &format!("interface-{index}"),
                &json!({"descriptorHex":"0501"}),
                "10".into(),
            )
            .unwrap();
    }
    assert!(catalog
        .observe("over-capacity", &json!({}))
        .unwrap_err()
        .contains("capacity"));
    assert_eq!(catalog.snapshots().len(), MAX_SNAPSHOTS);
    catalog
        .observe_at(
            "interface-0",
            &json!({"descriptorHex":"05010904"}),
            "20".into(),
        )
        .unwrap();
    assert_eq!(catalog.snapshot("interface-0").unwrap()["observedAt"], "20");
}

#[test]
fn invalid_and_oversized_metadata_cannot_replace_a_snapshot() {
    let mut catalog = Catalog::new(None).unwrap();
    let first = catalog.observe("first", &windows()).unwrap();
    for (id, detail) in [
        ("", json!({})),
        ("first", json!([])),
        ("first", json!({"interfaceId":"other"})),
        ("first", json!({"product":"x".repeat(MAX_DETAIL_BYTES)})),
    ] {
        assert!(catalog.observe(id, &detail).is_err());
    }
    assert_eq!(catalog.snapshot("first").unwrap(), first);
}

#[test]
fn malformed_retained_contract_is_rejected_without_overwriting_evidence() {
    let dir = crate::test_directory();
    let path = dir.path().join("controllers.json");
    let mut catalog = Catalog::new(Some(&path)).unwrap();
    catalog.observe("first", &windows()).unwrap();
    let db = Database::for_namespace(&path).unwrap();
    let record = db.read(DOMAIN, KEY).unwrap().unwrap();
    let mut invalid: Value = serde_json::from_slice(&record.payload).unwrap();
    invalid
        .as_object_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap()["contractFingerprint"] = json!("0".repeat(64));
    let bytes = serde_json::to_vec(&invalid).unwrap();
    db.commit(&[osg_persistence::Change {
        domain: DOMAIN,
        key: KEY,
        expected_revision: record.revision,
        payload: Some(&bytes),
    }])
    .unwrap();
    assert!(Catalog::new(Some(&path)).is_err());
    assert_eq!(db.read(DOMAIN, KEY).unwrap().unwrap().payload, bytes);
}

#[test]
fn macos_native_service_identity_never_merges_sibling_collections() {
    let mut catalog = Catalog::new(None).unwrap();
    let one = json!({"identity":{"registryEntryId":"101"},"vendorId":1,"productId":2,
        "descriptorHex":"05010904","elements":[{"cookie":1}],
        "descriptorProvenance":"IOHIDDeviceGetProperty(ReportDescriptor)",
        "elementsProvenance":"IOHIDDeviceCopyMatchingElements"});
    let first = catalog.observe("first", &one).unwrap();
    let mut sibling = one.clone();
    sibling["identity"]["registryEntryId"] = json!("102");
    catalog.observe("sibling", &sibling).unwrap();
    let mut replaced = one;
    replaced.as_object_mut().unwrap().remove("elements");
    replaced.as_object_mut().unwrap().remove("descriptorHex");
    let updated = catalog.observe("next", &replaced).unwrap();
    assert_eq!(updated["snapshotId"], first["snapshotId"]);
    assert_eq!(updated["previousInterfaceId"], "first");
    assert_eq!(updated["provenance"].get("descriptor"), None);
    assert!(updated["detail"].get("elements").is_none());
    assert_eq!(catalog.snapshots().len(), 2);
}

#[test]
fn durable_replacement_restarts_with_exactly_the_new_contract() {
    let dir = crate::test_directory();
    let path = dir.path().join("controllers.json");
    let mut catalog = Catalog::new(Some(&path)).unwrap();
    catalog
        .observe_at("first", &windows(), "10".into())
        .unwrap();
    let replacement = json!({"capabilities":{"provenance":"windows-hidp-preparsed-capabilities",
        "available":true,"reports":[{"kind":"feature","reportIds":[7],"valueCaps":[]}],"links":[]}});
    let expected = catalog
        .observe_at("first", &replacement, "20".into())
        .unwrap();
    drop(catalog);
    let restarted = Catalog::new(Some(&path)).unwrap();
    assert_eq!(restarted.snapshot("first").unwrap(), expected);
    assert_eq!(restarted.snapshots().len(), 1);
    assert!(restarted.snapshot("first").unwrap()["detail"]
        .get("attributes")
        .is_none());
}

#[test]
fn aggregate_byte_limit_rejects_new_entry_without_partial_publication() {
    let mut catalog = Catalog::new(None).unwrap();
    let detail = json!({"product":"x".repeat(MAX_DETAIL_BYTES / 2)});
    for index in 0..16 {
        let result = catalog.observe_at(&format!("interface-{index}"), &detail, "10".into());
        if index < 15 {
            result.unwrap();
        } else {
            assert!(result
                .unwrap_err()
                .contains("catalog exceeds its size bound"));
        }
    }
    assert_eq!(catalog.snapshots().len(), 15);
    assert!(catalog.snapshot("interface-15").is_none());
}
