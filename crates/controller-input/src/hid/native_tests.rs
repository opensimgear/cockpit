use super::*;
#[test]
fn stream_distinguishes_timeouts_removal_and_access_failure() {
    assert_eq!(
        delivery(Receive::Report(vec![0, 12])).unwrap(),
        Some(vec![0, 12])
    );
    assert_eq!(delivery(Receive::Timeout).unwrap(), None);
    assert_eq!(delivery(Receive::Removed).unwrap_err()["kind"], "removed");
    let denied = windows_receive::ReceiveError {
        kind: windows_receive::ErrorKind::AccessDenied,
        code: Some(5),
        message: "Access denied".into(),
    };
    assert_eq!(delivery(Receive::Error(denied)).unwrap_err()["code"], 5);
}
use crate::windows_caps::{Report, Snapshot};
use windows_inventory::{Attributes, Inventory};
fn row(path: &str) -> Interface {
    let report = |kind, byte_length| Report {
        kind,
        byte_length,
        data_indices: 0,
        declared_button_caps: 0,
        declared_value_caps: 0,
        report_ids: vec![1],
        button_caps: vec![],
        value_caps: vec![],
        complete: true,
    };
    Interface {
        index: 0,
        path: Some(path.into()),
        attributes: Some(Attributes {
            vendor_id: 1,
            product_id: 2,
            version_number: 3,
        }),
        manufacturer: None,
        product: None,
        serial: None,
        metadata_opened: true,
        failures: vec![],
        caps: Some(Snapshot {
            api_version: crate::windows_caps::ApiVersion {
                number: Some(2),
                source: "fixture".into(),
                status: None,
            },
            provenance: "fixture".into(),
            available: true,
            complete: true,
            usage_page: Some(1),
            usage: Some(6),
            declared_link_count: 0,
            reports: vec![
                report(ReportKind::Input, 8),
                report(ReportKind::Feature, 16),
            ],
            links: vec![],
            failures: vec![],
        }),
    }
}
#[test]
fn catalogue_requests_accept_only_discovered_interface_ids() {
    let w = Windows {
        enumerate: || {
            Ok(Inventory {
                interfaces: vec![row("keyboard")],
                truncated: false,
                failures: vec![],
            })
        },
        inspect: |path| Ok(row(path)),
        ..Default::default()
    };
    assert!(w.inventory(Some("keyboard")).is_err());
    let list = w.inventory(None).unwrap();
    let selected = list["interfaces"][0]["interfaceId"].as_str().unwrap();
    assert_eq!(list["interfaces"][0]["access"], "metadata-only");
    let detail = w.inventory(Some(selected)).unwrap();
    assert_eq!(detail["capabilities"]["usage"], 6);
    assert!(w.open(selected).err().unwrap().contains("MetadataOnly"));
    let s = selection(&row("test")).unwrap();
    assert_eq!(s.input_report_bytes, 8);
    assert_eq!(s.feature_report_bytes, 16);
    let mut absent = row("offline");
    absent.path = None;
    absent.caps = None;
    absent.metadata_opened = false;
    assert_eq!(summary(&absent)["access"], "unknown");
    assert!(selection(&absent).is_err());
}

#[test]
fn changed_layout_retires_the_handle_even_at_the_same_path() {
    let windows = Windows {
        enumerate: || {
            Ok(Inventory {
                interfaces: vec![row("same")],
                truncated: false,
                failures: vec![],
            })
        },
        inspect: |path| {
            let mut replacement = row(path);
            replacement.caps.as_mut().unwrap().reports[0].report_ids = vec![2];
            Ok(replacement)
        },
        ..Default::default()
    };
    let list = windows.inventory(None).unwrap();
    let old = list["interfaces"][0]["interfaceId"].as_str().unwrap();
    assert!(windows
        .inventory(Some(old))
        .unwrap_err()
        .contains("changed"));
    assert!(windows.open(old).is_err());
}

fn deferred(path: &str) -> Interface {
    let mut item = row(path);
    item.caps = None;
    item.failures.push(windows_inventory::Failure {
        stage: "caps-capacity".into(),
        native_code: None,
        reason: "Inspect this interface separately".into(),
    });
    item
}

#[test]
fn budget_deferred_interface_can_be_inspected_and_open_uses_the_inspected_contract() {
    let windows = Windows {
        enumerate: || {
            Ok(Inventory {
                interfaces: vec![deferred("deferred")],
                truncated: true,
                failures: vec![],
            })
        },
        inspect: |path| Ok(row(path)),
        ..Default::default()
    };
    let list = windows.inventory(None).unwrap();
    let token = list["interfaces"][0]["interfaceId"].as_str().unwrap();
    let detail = windows.inventory(Some(token)).unwrap();
    assert_eq!(detail["interfaceId"], token);
    assert_eq!(
        detail["pathFingerprint"],
        list["interfaces"][0]["pathFingerprint"]
    );
    assert_eq!(detail["capabilities"]["reports"][0]["byteLength"], 8);
    // Metadata-only policy proves open reached selection after successful full
    // reinspection, without attempting a real native device open in this test.
    assert!(windows.open(token).err().unwrap().contains("MetadataOnly"));
    let refreshed = windows.inventory(None).unwrap();
    assert_eq!(refreshed["interfaces"][0]["interfaceId"], token);
    assert!(windows.inventory(Some(token)).is_ok());
}

#[test]
fn metadata_poor_inventory_can_bind_one_complete_inspection() {
    let windows = Windows {
        enumerate: || {
            let mut item = deferred("poor");
            item.attributes = None;
            item.serial = None;
            item.metadata_opened = false;
            Ok(Inventory {
                interfaces: vec![item],
                truncated: false,
                failures: vec![],
            })
        },
        inspect: |path| {
            let mut item = row(path);
            item.serial = Some("actual-serial".into());
            Ok(item)
        },
        ..Default::default()
    };
    let list = windows.inventory(None).unwrap();
    let token = list["interfaces"][0]["interfaceId"].as_str().unwrap();
    assert_eq!(
        windows.inventory(Some(token)).unwrap()["serial"],
        "actual-serial"
    );
    assert!(windows.inventory(Some(token)).is_ok());
}

#[test]
fn same_path_replacement_retires_old_handle_permanently() {
    let windows = Windows::default();
    let inventory = |item| Inventory {
        interfaces: vec![item],
        truncated: false,
        failures: vec![],
    };
    let first = windows.catalogue(inventory(row("same")));
    let old = first["interfaces"][0]["interfaceId"].as_str().unwrap();
    let mut replacement = row("same");
    replacement.caps.as_mut().unwrap().reports[0].report_ids = vec![7];
    let next = windows.catalogue(inventory(replacement));
    assert_ne!(next["interfaces"][0]["interfaceId"], old);
    assert!(windows.inventory(Some(old)).is_err());
    let returned = windows.catalogue(inventory(row("same")));
    assert_ne!(returned["interfaces"][0]["interfaceId"], old);
    assert_eq!(
        returned["interfaces"][0]["pathFingerprint"],
        first["interfaces"][0]["pathFingerprint"]
    );
}

#[test]
fn first_deferred_inspection_binds_full_contract_and_a_later_change_retires_it() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static INSPECTIONS: AtomicUsize = AtomicUsize::new(0);
    let windows = Windows {
        enumerate: || {
            Ok(Inventory {
                interfaces: vec![deferred("bound")],
                truncated: true,
                failures: vec![],
            })
        },
        inspect: |path| {
            let mut item = row(path);
            if INSPECTIONS.fetch_add(1, Ordering::SeqCst) == 1 {
                item.caps.as_mut().unwrap().reports[0].report_ids = vec![9];
            }
            Ok(item)
        },
        ..Default::default()
    };
    let list = windows.inventory(None).unwrap();
    let token = list["interfaces"][0]["interfaceId"].as_str().unwrap();
    assert!(windows.inventory(Some(token)).is_ok());
    assert!(windows
        .inventory(Some(token))
        .unwrap_err()
        .contains("changed"));
    // The path now exposes its original layout again. An old issued token
    // cannot revive just because the fingerprint happens to match again.
    assert!(windows
        .inventory(Some(token))
        .unwrap_err()
        .contains("Unknown"));
    assert!(windows.open(token).is_err());
}

#[test]
fn held_capabilities_must_match_every_inspected_field_even_when_lengths_match() {
    let inspected = row("held");
    let mut held = inspected.caps.clone().unwrap();
    assert!(verify_held(&inspected, &held).is_ok());
    held.reports[0].report_ids = vec![7];
    assert!(verify_held(&inspected, &held)
        .unwrap_err()
        .contains("Opened HID capabilities changed"));
}

#[test]
fn issued_inventory_is_bounded_and_disappearance_invalidates_old_tokens() {
    let windows = Windows::default();
    let interfaces = (0..=windows_inventory::MAX_INTERFACES)
        .map(|index| deferred(&format!("bounded-{index}")))
        .collect();
    let list = windows.catalogue(Inventory {
        interfaces,
        truncated: false,
        failures: vec![],
    });
    assert_eq!(list["interfaces"].as_array().unwrap().len(), 1024);
    assert_eq!(list["truncated"], true);
    assert_eq!(windows.paths.lock().unwrap().len(), 1024);
    let old = list["interfaces"][0]["interfaceId"].as_str().unwrap();
    windows.catalogue(Inventory {
        interfaces: vec![],
        truncated: false,
        failures: vec![],
    });
    assert!(windows
        .inventory(Some(old))
        .unwrap_err()
        .contains("Unknown"));
}

#[test]
fn duplicate_native_path_cannot_issue_two_selectable_handles() {
    let windows = Windows::default();
    let list = windows.catalogue(Inventory {
        interfaces: vec![row("duplicate"), row("duplicate")],
        truncated: true,
        failures: vec![],
    });
    assert!(list["interfaces"][0]["interfaceId"].is_string());
    assert!(list["interfaces"][1]["interfaceId"].is_null());
    assert_eq!(windows.paths.lock().unwrap().len(), 1);
}
