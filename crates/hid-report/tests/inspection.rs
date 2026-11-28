use osg_hid_report::{decode_report, parse_descriptor, Framing, ReportKind};
use serde_json::to_value;
mod support;

#[test]
fn full_size_opaque_field_uses_the_wire_budget() {
    // Report Size 524,288 bits, Count 1. Both padding and Buffered Bytes are opaque.
    for flags in [0x0001u16, 0x0102] {
        let [low, high] = flags.to_le_bytes();
        let descriptor = parse_descriptor(&[0x77, 0, 0, 8, 0, 0x95, 1, 0x82, low, high]).unwrap();
        assert!(descriptor.diagnostics.is_empty());
        let wire = vec![0x5a; osg_hid_report::MAX_REPORT_BYTES];
        let report =
            decode_report(&descriptor, ReportKind::Input, Framing::Unnumbered, &wire).unwrap();
        assert_eq!(report.values[0].raw_bits, wire);
    }
}

#[test]
fn exported_evidence_replays_every_golden_report() {
    for fixture in support::corpus("authored") {
        let descriptor =
            parse_descriptor(&support::hex(fixture["descriptorHex"].as_str().unwrap())).unwrap();
        let exported = to_value(&descriptor).unwrap();
        let replay = parse_descriptor(&support::hex(exported["raw"].as_str().unwrap())).unwrap();
        assert_eq!(descriptor, replay);
        for sample in fixture["samples"].as_array().unwrap() {
            let kind = match sample["kind"].as_str().unwrap() {
                "input" => ReportKind::Input,
                "output" => ReportKind::Output,
                "feature" => ReportKind::Feature,
                other => panic!("Unexpected report kind {other}"),
            };
            let framing = if sample["reportId"].is_null() {
                Framing::Unnumbered
            } else {
                Framing::IdPrefix
            };
            let wire = support::hex(sample["wireHex"].as_str().unwrap());
            let first = decode_report(&descriptor, kind, framing, &wire).unwrap();
            let json = to_value(&first).unwrap();
            let restored = support::hex(json["raw"].as_str().unwrap());
            assert_eq!(
                first,
                decode_report(&replay, kind, framing, &restored).unwrap()
            );
        }
    }
}

#[test]
fn flags_and_wire_evidence_are_inspectable() {
    let descriptor = parse_descriptor(&[
        0x15, 0xff, 0x25, 0x7f, 0x75, 8, 0x95, 1, 0x82, 0x7f, 0x01, 0x92, 0xff, 0x01, 0xb1, 0,
        0xb1, 0x82,
    ])
    .unwrap();
    assert!(descriptor.diagnostics.is_empty());
    let input = &descriptor.fields[0];
    let a = input.attributes();
    assert!(a.constant && a.variable && a.relative && a.wrap && a.nonlinear);
    assert!(a.no_preferred_state && a.null_state && a.buffered_bytes);
    assert_eq!(a.volatile, None);
    assert_eq!(input.flags, 0x17f);
    assert_eq!(descriptor.fields[1].attributes().volatile, Some(true));
    assert_eq!(descriptor.fields[2].attributes().volatile, Some(false));
    assert_eq!(descriptor.fields[3].attributes().volatile, Some(true));
    let reserved = parse_descriptor(&[0x75, 8, 0x95, 1, 0x81, 0x82]).unwrap();
    assert_eq!(reserved.fields[0].flags, 0x82);
    assert!(!reserved.diagnostics.is_empty());
    let evidence = to_value(&descriptor).unwrap();
    assert_eq!(evidence["raw"], "15ff257f75089501827f0192ff01b100b182");
    assert_eq!(evidence["fields"][0]["globals"]["logical_min"], "-1");
    assert!(evidence["fields"][0]["globals"]["physical_min"].is_null());
    let report =
        decode_report(&descriptor, ReportKind::Input, Framing::Unnumbered, &[0xad]).unwrap();
    let wire = to_value(report).unwrap();
    assert_eq!(wire["raw"], "ad");
    assert_eq!(wire["values"][0]["raw_bits"], "ad");
    assert_eq!(wire["values"][0]["sample"]["Bytes"], "ad");
}
