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
            api_version:crate::windows_caps::ApiVersion{number:Some(2),source:"fixture".into(),status:None},
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
    let s = selection(row("test")).unwrap();
    assert_eq!(s.input_report_bytes, 8);
    assert_eq!(s.feature_report_bytes, 16);
    let mut absent = row("offline");
    absent.path = None;
    absent.caps = None;
    absent.metadata_opened = false;
    assert_eq!(summary(&absent)["access"], "unknown");
    assert!(selection(absent).is_err());
}
