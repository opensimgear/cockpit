use super::*;

#[test]
fn identity_changes_for_registry_descriptor_and_element_changes() {
    let base = json!({"identity":{"registryEntryId":"100"},"descriptorHex":"0501","elements":[{"cookie":1} ]});
    for changed in [
        json!({"identity":{"registryEntryId":"101"},"descriptorHex":"0501","elements":[{"cookie":1} ]}),
        json!({"identity":{"registryEntryId":"100"},"descriptorHex":"0502","elements":[{"cookie":1} ]}),
        json!({"identity":{"registryEntryId":"100"},"descriptorHex":"0501","elements":[{"cookie":2} ]}),
    ] {
        assert_ne!(hash(&base), hash(&changed));
    }
}

#[test]
fn counts_reject_negative_and_over_budget_values() {
    assert_eq!(native::bounded(0, 1024, "test").unwrap(), 0);
    assert_eq!(native::bounded(1024, 1024, "test").unwrap(), 1024);
    assert!(native::bounded(-1, 1024, "test").is_err());
    assert!(native::bounded(1025, 1024, "test").is_err());
}

#[test]
fn arbitrary_ids_fail_before_native_discovery() {
    assert!(
        matches!(selected("unissued-id"), Err(e) if e.contains("Unknown HID interface generation"))
    );
}

#[test]
fn generic_inventory_is_reachable_through_service_without_opening_inputs() {
    let service = crate::ControllerService::new(None, true);
    let wake = std::sync::Arc::new(osg_streams::NativeWake::default());
    service.watch_resource_inventory(&wake).unwrap();
    let mapping_epoch = wake.observed();
    let response = service
        .request(&json!({"command":"controller_hid_list"}))
        .unwrap();
    assert!(response["interfaces"].is_array());
    assert!(response["truncated"].is_boolean());
    for row in response["interfaces"].as_array().unwrap() {
        assert!(row["descriptorHex"].is_null());
        assert!(row["elements"].is_null());
        if let Some(id) = row["interfaceId"].as_str() {
            // No input registration, device open, value/feature read, or writes.
            let detail = service
                .request(&json!({"command":"controller_hid_inspect","interfaceId":id}))
                .unwrap();
            assert_eq!(detail["interfaceId"], id);
            assert!(detail["identity"]["registryEntryId"].is_string());
        }
    }
    // Metadata-only IOHID discovery must never publish a mapping source.
    assert!(service
        .authoritative_resource_inventory()
        .unwrap()
        .sources
        .is_empty());
    assert_eq!(wake.observed(), mapping_epoch);
    service.shutdown();
    assert!(service
        .request(&json!({"command":"controller_hid_list"}))
        .is_err());
}
