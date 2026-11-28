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
    let mut details = Vec::new();
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
            details.push(detail);
        }
    }
    if let Some(probe) = std::env::var_os("OSG_TEST_IOHID_METADATA_PROBE") {
        compare_independent_metadata(&probe, &details, response["interfaces"].as_array().unwrap());
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

fn probe_json(probe: &std::ffi::OsStr, args: &[&str]) -> Value {
    let output = std::process::Command::new(probe)
        .args(args)
        .output()
        .expect("Run the existing compiled metadata-only Swift probe");
    assert!(
        output.status.success(),
        "Independent IOHID metadata probe {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value =
        serde_json::from_slice(&output.stdout).expect("Swift probe emitted JSON metadata");
    assert_eq!(receipt["origin"], "independent-macos-iohid-elements");
    for flag in [
        "fixtureEvidence",
        "deviceOpened",
        "inputReads",
        "featureReads",
        "reportWrites",
    ] {
        assert_eq!(
            receipt[flag], false,
            "Metadata probe must not capture input: {flag}"
        );
    }
    receipt
}

fn compare_independent_metadata(probe: &std::ffi::OsStr, details: &[Value], native_rows: &[Value]) {
    let independent = probe_json(probe, &["--list"]);
    let devices = independent["devices"]
        .as_array()
        .expect("Swift device inventory");
    let ids: std::collections::HashSet<_> = devices
        .iter()
        .filter_map(|d| d["registryEntryId"].as_str())
        .collect();
    let native_count = native_rows.len();
    if !devices.is_empty() {
        assert!(
            native_count > 0,
            "Native IOHID inventory is empty despite independently observed devices"
        );
        for device in devices {
            let id = device["registryEntryId"]
                .as_str()
                .expect("Independent registry entry ID");
            let native = native_rows
                .iter()
                .find(|row| row["identity"]["registryEntryId"].as_str() == Some(id))
                .unwrap_or_else(|| {
                    panic!("Native IOHID inventory omitted independently observed registry {id}")
                });
            if device["usagePage"] == 1 && matches!(device["usage"].as_u64(), Some(6 | 7)) {
                assert_eq!(
                    native["restricted"], true,
                    "Keyboard registry {id} must remain visible as restricted"
                );
                assert_eq!(
                    native["access"]["policy"], "metadata-only",
                    "Keyboard registry {id} must remain metadata only"
                );
            }
        }
    }
    let matching: Vec<_> = details
        .iter()
        .filter(|d| {
            d["identity"]["registryEntryId"]
                .as_str()
                .is_some_and(|id| ids.contains(id))
        })
        .collect();
    let mut comparable = Vec::new();
    for detail in &matching {
        // The independent probe deliberately refuses an empty element list.
        // Restricted interfaces may legitimately expose no element metadata;
        // count this absence explicitly, without calling it a comparison pass.
        if detail["elements"].as_array().is_some_and(|e| !e.is_empty()) {
            comparable.push(*detail);
        } else {
            eprintln!("IOHID metadata comparison excluded registry {}: native element metadata absent/empty; independent probe refuses empty lists", detail["identity"]["registryEntryId"]);
        }
    }
    let mut inspected = 0;
    let mut descriptor_unavailable = 0;
    for detail in comparable.iter().take(2) {
        let registry = detail["identity"]["registryEntryId"].as_str().unwrap();
        let receipt = probe_json(probe, &["--registry-id", registry]);
        assert_eq!(receipt["device"]["registryEntryId"], registry);
        assert_eq!(
            receipt["descriptorProvenance"],
            detail["descriptorProvenance"]
        );
        assert_eq!(receipt["elementsProvenance"], detail["elementsProvenance"]);
        // Swift reports absent and zero-byte descriptors as unavailable. Compare
        // exact hex whenever either reader actually obtained descriptor bytes.
        let native_hex = detail["descriptorHex"].as_str().filter(|s| !s.is_empty());
        let probe_hex = receipt["descriptorHex"].as_str().filter(|s| !s.is_empty());
        assert_eq!(
            native_hex, probe_hex,
            "Descriptor bytes differ for registry {registry}"
        );
        if native_hex.is_none() {
            descriptor_unavailable += 1;
            eprintln!("IOHID registry {registry}: descriptor absent/empty in both readers; comparing native element metadata only");
        }
        // Both implementations sort by cookie. Compare the full real schema:
        // cookie/parent, native type/collection, report ID/size/count, usages and
        // ranges, logical/physical bounds, units, and every exposed flag.
        assert!(
            receipt["elements"]
                .as_array()
                .is_some_and(|e| !e.is_empty()),
            "Independent element metadata unavailable for native nonempty registry {registry}"
        );
        assert_eq!(
            receipt["elements"], detail["elements"],
            "Native element metadata differs for registry {registry}"
        );
        inspected += 1;
    }
    if !comparable.is_empty() {
        assert!(
            inspected > 0,
            "Populated comparable inventories require an actual registry comparison"
        );
    }
    eprintln!("IOHID metadata comparison: native={native_count} nativeInspectable={} independent={} matched={} comparable={} inspected={inspected} excludedEmpty={} descriptorUnavailable={descriptor_unavailable}; metadata only, no device opens/input or feature reads/writes; unselected/exclusive inventory IDs are not hardware validation",details.len(),devices.len(),matching.len(),comparable.len(),matching.len()-comparable.len());
}
