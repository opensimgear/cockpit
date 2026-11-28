use super::*;

#[test]
fn capability_bounds_are_exact_decimal_strings_without_changing_internal_numbers() {
    let mut snapshot = basic();
    let value = &mut snapshot.reports[0].value_caps[0];
    value.logical_min = i64::MIN;
    value.logical_max = i64::MAX;
    value.physical_min = -1;
    value.physical_max = 4294967295;
    value.logical_min_raw = i32::MIN;
    value.logical_max_raw = i32::MAX;
    value.physical_min_raw = 0;
    value.physical_max_raw = -1;
    value.units = 0xffff_ffff;
    value.units_exp_raw = 0xffff_fffe;
    assert_eq!(value.logical_min, i64::MIN);
    let json = serde_json::to_value(&snapshot).unwrap();
    let value = &json["reports"][0]["valueCaps"][0];
    for (key, expected) in [
        ("logicalMin", "-9223372036854775808"),
        ("logicalMax", "9223372036854775807"),
        ("physicalMin", "-1"),
        ("physicalMax", "4294967295"),
        ("logicalMinRaw", "-2147483648"),
        ("logicalMaxRaw", "2147483647"),
        ("physicalMinRaw", "0"),
        ("physicalMaxRaw", "-1"),
        ("units", "4294967295"),
        ("unitsExpRaw", "4294967294"),
    ] {
        assert_eq!(value[key].as_str(), Some(expected), "{key}");
    }
    assert_eq!(value["reportId"], 1);
    assert_eq!(value["bitSize"], 8);
    assert_eq!(json["reports"][0]["reportIds"], serde_json::json!([1]));
}

#[test]
fn samples_export_wire_order_lowercase_hex_and_exact_native_decimal() {
    let sample = Samples {
        provenance: "windows-hidp-report-projection".into(),
        kind: ReportKind::Input,
        report_id: Some(1),
        raw: vec![1, 0, 0xab, 0xff],
        data: vec![DataValue {
            data_index: 7,
            raw_value: u32::MAX,
            quality: "os-projected".into(),
            matching_button_caps: vec![],
            matching_value_caps: vec![0],
            values: vec![
                ScalarValue {
                    capability_index: 0,
                    native: Some(i64::MIN),
                    quality: "valid".into(),
                    semantics: "relative".into(),
                },
                ScalarValue {
                    capability_index: 1,
                    native: None,
                    quality: "unsupported-width".into(),
                    semantics: "absolute".into(),
                },
            ],
        }],
        value_arrays: vec![ValueArray {
            capability_index: 0,
            usage_page: 1,
            usage: 0x30,
            link_collection: 0,
            bit_size: 8,
            report_count: 3,
            bytes: vec![0, 0xab, 0xff],
        }],
        failures: vec![],
    };
    let json = serde_json::to_value(&sample).unwrap();
    assert_eq!(json["raw"], "0100abff");
    assert_eq!(json["valueArrays"][0]["bytes"], "00abff");
    assert_eq!(json["data"][0]["rawValue"], "4294967295");
    assert_eq!(
        json["data"][0]["values"][0]["native"],
        "-9223372036854775808"
    );
    assert!(json["data"][0]["values"][1]["native"].is_null());
    assert_eq!(sample.raw, vec![1, 0, 0xab, 0xff]);
    assert_eq!(sample.data[0].raw_value, u32::MAX);
    let mut empty = sample;
    empty.raw.clear();
    empty.value_arrays[0].bytes.clear();
    let json = serde_json::to_value(empty).unwrap();
    assert_eq!(json["raw"], "");
    assert_eq!(json["valueArrays"][0]["bytes"], "");
}
