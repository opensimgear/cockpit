mod support;
use osg_hid_report::*;
use serde_json::Value as Json;
use support::{corpus, decimal, hex};

fn kind(value: &Json) -> ReportKind {
    match value.as_str().unwrap() {
        "input" => ReportKind::Input,
        "output" => ReportKind::Output,
        "feature" => ReportKind::Feature,
        other => panic!("unknown golden kind {other}"),
    }
}
fn report_id(value: &Json) -> Option<u8> {
    value.as_u64().map(|value| u8::try_from(value).unwrap())
}

fn element_at(
    descriptor: &Descriptor,
    layout: &Layout,
    offset: usize,
    width: usize,
) -> (usize, usize) {
    let matches: Vec<_> = layout
        .fields
        .iter()
        .flat_map(|&index| {
            let field = &descriptor.fields[index];
            (0..field.globals.count).filter_map(move |element| {
                (field.globals.size == width && field.offset_bits + element * width == offset)
                    .then_some((index, element))
            })
        })
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "one physical element at payload bit {offset}, width {width}"
    );
    matches[0]
}

/// The authored usage labels are readable prose; keep the numeric golden oracle explicit.
fn expected_usages(label: &str, field: &Json, element: usize, encoded: &str) -> Vec<Usage> {
    let id = field["id"].as_str().unwrap();
    let usage = field["usage"].as_str().unwrap();
    let pair = if field["mode"] == "constant" {
        return vec![];
    } else if label == "metadata-preservation" && id == "vendor-byte" {
        return vec![
            Usage {
                page: 0xff00,
                id: 1,
            },
            Usage {
                page: 0xff00,
                id: 2,
            },
        ];
    } else if usage.starts_with("Vendor FF00:") {
        (0xff00, u16::from_str_radix(&usage[12..14], 16).unwrap())
    } else if usage.starts_with("Button ") {
        (9, 1 + element as u16)
    } else if usage.starts_with("Keyboard E0") {
        (7, 0xe0 + element as u16)
    } else if usage.starts_with("Keyboard 00") {
        (7, encoded.parse().unwrap())
    } else if usage.starts_with("LED ") {
        (8, 1 + element as u16)
    } else if usage.contains("HatSwitch") {
        (1, 0x39)
    } else if usage.contains("Wheel") {
        (1, 0x38)
    } else if usage == "GenericDesktop Y" {
        (1, 0x31)
    } else if usage == "GenericDesktop X,Y" {
        (1, 0x30 + element as u16)
    } else if usage.starts_with("GenericDesktop X") {
        (1, 0x30)
    } else {
        panic!("missing numeric usage oracle for {label}/{id}");
    };
    vec![Usage {
        page: pair.0,
        id: pair.1,
    }]
}

#[test]
fn every_h0_descriptor_layout_and_exact_golden_sample() {
    let fixtures = corpus("authored");
    assert_eq!(fixtures.len(), 7, "review new H0 fixtures explicitly");
    let mut layouts = 0;
    let mut reports = 0;
    for fixture in fixtures {
        let label = fixture["id"].as_str().unwrap();
        let raw = hex(fixture["descriptorHex"].as_str().unwrap());
        let descriptor = parse_descriptor(&raw).unwrap();
        assert_eq!(descriptor.raw, raw, "{label}: original descriptor");
        assert!(
            descriptor.diagnostics.is_empty(),
            "{label}: {:?}",
            descriptor.diagnostics
        );
        assert_eq!(
            descriptor.layouts.len(),
            fixture["reports"].as_array().unwrap().len(),
            "{label}"
        );
        for expected in fixture["reports"].as_array().unwrap() {
            layouts += 1;
            let layout = descriptor
                .layouts
                .iter()
                .find(|layout| {
                    layout.kind == kind(&expected["kind"])
                        && layout.report_id == report_id(&expected["reportId"])
                })
                .expect("every golden report type/ID exists");
            assert_eq!(
                layout.payload_bits as u64,
                expected["payloadBits"].as_u64().unwrap(),
                "{label}"
            );
            let expected_fields = expected["fields"].as_array().unwrap();
            assert_eq!(
                layout
                    .fields
                    .iter()
                    .map(|&index| descriptor.fields[index].globals.count)
                    .sum::<usize>(),
                expected_fields
                    .iter()
                    .map(|field| field["count"].as_u64().unwrap() as usize)
                    .sum::<usize>(),
                "{label}: same physical elements independent of Main grouping"
            );
            for expected_field in expected_fields {
                for expected_element in 0..expected_field["count"].as_u64().unwrap() as usize {
                    let width = expected_field["width"].as_u64().unwrap() as usize;
                    let offset = expected_field["offset"].as_u64().unwrap() as usize
                        + expected_element * width;
                    let (index, element) = element_at(&descriptor, layout, offset, width);
                    let field = &descriptor.fields[index];
                    assert_eq!(field.offset_bits + element * width, offset, "{label}");
                    assert_eq!(
                        field.globals.size as u64,
                        expected_field["width"].as_u64().unwrap(),
                        "{label}"
                    );
                    assert_eq!(
                        field.globals.logical_min.unwrap().to_string(),
                        expected_field["min"].as_str().unwrap(),
                        "{label}"
                    );
                    assert_eq!(
                        field.globals.logical_max.unwrap().to_string(),
                        expected_field["max"].as_str().unwrap(),
                        "{label}"
                    );
                    let constant = expected_field["mode"] == "constant";
                    assert_eq!(field.flags & 1 != 0, constant, "{label}");
                    if !constant {
                        assert_eq!(
                            field.flags & 2 == 0,
                            expected_field["mode"] == "array",
                            "{label}"
                        );
                    }
                    if let Some(relative) = expected_field["relative"].as_bool() {
                        assert_eq!(field.flags & 4 != 0, relative, "{label}");
                    }
                    if let Some(unit) = expected_field["unitCode"].as_str() {
                        assert_eq!(field.globals.unit.unwrap().to_string(), unit, "{label}");
                    }
                    if let Some(exponent) = expected_field["unitExponent"].as_i64() {
                        assert_eq!(
                            field.globals.exponent.map(i64::from),
                            Some(exponent),
                            "{label}"
                        );
                    }
                    if let Some(bounds) = expected_field["physical"].as_object() {
                        assert_eq!(
                            field.globals.physical_min.unwrap().to_string(),
                            bounds["min"].as_str().unwrap(),
                            "{label}"
                        );
                        assert_eq!(
                            field.globals.physical_max.unwrap().to_string(),
                            bounds["max"].as_str().unwrap(),
                            "{label}"
                        );
                    }
                }
            }
        }
        for sample in fixture["samples"].as_array().unwrap() {
            reports += 1;
            let expected = fixture["reports"]
                .as_array()
                .unwrap()
                .iter()
                .find(|report| {
                    report["kind"] == sample["kind"] && report["reportId"] == sample["reportId"]
                })
                .unwrap();
            let wire = hex(sample["wireHex"].as_str().unwrap());
            let framing = if sample["reportId"].is_null() {
                Framing::Unnumbered
            } else {
                Framing::IdPrefix
            };
            let decoded =
                decode_report(&descriptor, kind(&sample["kind"]), framing, &wire).unwrap();
            assert_eq!(decoded.raw, wire, "{label}: raw report retained");
            assert!(
                decoded.diagnostics.is_empty(),
                "{label}/{}: {:?}",
                sample["name"],
                decoded.diagnostics
            );
            let layout = descriptor
                .layouts
                .iter()
                .find(|layout| layout.kind == decoded.kind && layout.report_id == decoded.report_id)
                .unwrap();
            let expected_fields = expected["fields"].as_array().unwrap();
            let expected_count: usize = expected_fields
                .iter()
                .map(|field| field["count"].as_u64().unwrap() as usize)
                .sum();
            assert_eq!(
                decoded.values.len(),
                expected_count,
                "{label}: retain constants and each array selector"
            );
            for expected_field in expected_fields {
                let expected_values = &sample["values"][expected_field["id"].as_str().unwrap()];
                let width = expected_field["width"].as_u64().unwrap() as usize;
                for (expected_element, expected_value) in
                    expected_values.as_array().unwrap().iter().enumerate()
                {
                    let offset = expected_field["offset"].as_u64().unwrap() as usize
                        + expected_element * width;
                    let (index, element) = element_at(&descriptor, layout, offset, width);
                    let value = decoded
                        .values
                        .iter()
                        .find(|value| value.field == index && value.element == element)
                        .unwrap();
                    assert_eq!(
                        value.usages,
                        expected_usages(
                            label,
                            expected_field,
                            expected_element,
                            expected_value.as_str().unwrap()
                        ),
                        "{label}/{} usage attribution at bit {offset}",
                        sample["name"]
                    );
                    assert_eq!(
                        decimal(
                            &value.raw_bits,
                            0,
                            width,
                            expected_field["signed"].as_bool().unwrap()
                        ),
                        expected_value.as_str().unwrap(),
                        "{label}/{}",
                        sample["name"]
                    );
                    let field_id = expected_field["id"].as_str().unwrap();
                    let outside = sample["outsideLogicalRange"]
                        .as_array()
                        .is_some_and(|items| items.iter().any(|item| item == field_id));
                    let null = sample["nullFields"]
                        .as_array()
                        .is_some_and(|items| items.iter().any(|item| item == field_id));
                    if null {
                        assert!(
                            matches!(&value.sample, Sample::Null(encoded) if encoded == expected_value.as_str().unwrap()),
                            "{label}: null retains exact encoded value"
                        );
                    } else if outside {
                        assert!(
                            matches!(&value.sample, Sample::Invalid(encoded) if encoded == expected_value.as_str().unwrap()),
                            "{label}: logical overflow remains exact and invalid"
                        );
                    } else if expected_field["mode"] == "array" {
                        assert!(
                            matches!(&value.sample, Sample::Array{selector,..} if selector == expected_value.as_str().unwrap()),
                            "{label}: selector retained"
                        );
                    } else if expected_field["mode"] != "constant" {
                        assert!(
                            matches!(&value.sample, Sample::Integer(encoded) if encoded == expected_value.as_str().unwrap()),
                            "{label}: exact integer result"
                        );
                    }
                    if expected_field["relative"] == true {
                        assert_eq!(value.semantics, Semantics::Relative);
                    }
                    if let Some(bounds) = expected_field["physical"].as_object() {
                        if expected_field["mode"] == "variable" && !null && !outside {
                            let physical = value
                                .physical
                                .as_ref()
                                .expect("declared exact physical interpolation");
                            let parse = |text: &str| text.parse::<i128>().unwrap();
                            let low = parse(expected_field["min"].as_str().unwrap());
                            let high = parse(expected_field["max"].as_str().unwrap());
                            let plow = parse(bounds["min"].as_str().unwrap());
                            let phigh = parse(bounds["max"].as_str().unwrap());
                            let number = parse(expected_value.as_str().unwrap());
                            let denominator = parse(&physical.denominator);
                            assert!(denominator > 0);
                            assert_eq!(
                                parse(&physical.numerator) * (high - low),
                                ((number - low) * (phigh - plow) + plow * (high - low))
                                    * denominator
                            );
                        }
                    }
                }
            }
            assert_eq!(
                serde_json::to_value(&decoded).unwrap(),
                serde_json::to_value(
                    decode_report(&descriptor, kind(&sample["kind"]), framing, &wire).unwrap()
                )
                .unwrap(),
                "deterministic decode/export"
            );
        }
    }
    assert_eq!((layouts, reports), (11, 18));
}

#[test]
fn metadata_items_globals_collection_tree_and_local_reset_are_preserved() {
    let fixture = corpus("authored")
        .into_iter()
        .find(|fixture| fixture["id"] == "metadata-preservation")
        .unwrap();
    let raw = hex(fixture["descriptorHex"].as_str().unwrap());
    let descriptor = parse_descriptor(&raw).unwrap();
    assert_eq!(descriptor.collections.len(), 2);
    assert_eq!(descriptor.collections[0].parent, None);
    assert_eq!(descriptor.collections[1].parent, Some(0));
    assert_eq!(
        (
            descriptor.collections[0].type_code,
            descriptor.collections[1].type_code
        ),
        (1, 0)
    );
    assert_eq!(descriptor.fields.len(), 3);
    let vendor = &descriptor.fields[0];
    assert_eq!(
        (
            vendor.globals.usage_page,
            vendor.globals.size,
            vendor.offset_bits
        ),
        (0xff00, 8, 0)
    );
    assert_eq!(vendor.primary.strings, vec![LocalIndex::Single(3)]);
    assert_eq!(vendor.primary.designators, vec![LocalIndex::Single(2)]);
    assert_eq!(
        vendor.primary.usages,
        vec![LocalIndex::Single(Usage {
            page: 0xff00,
            id: 1
        })]
    );
    assert_eq!(vendor.alternatives.len(), 1);
    assert_eq!(
        vendor.alternatives[0].usages,
        vec![LocalIndex::Single(Usage {
            page: 0xff00,
            id: 2
        })]
    );
    let restored = &descriptor.fields[1];
    assert_eq!(
        (
            restored.globals.usage_page,
            restored.globals.size,
            restored.offset_bits
        ),
        (1, 4, 8)
    );
    assert!(
        restored.primary.strings.is_empty()
            && restored.primary.designators.is_empty()
            && restored.alternatives.is_empty()
    );
    assert!(descriptor.items.iter().any(|item| item.offset == 51
        && item.length == 6
        && item.long
        && item.disposition == Disposition::Preserved));
    assert!(descriptor.items.iter().any(|item| item.offset == 57
        && item.length == 1
        && item.type_code == 3
        && item.disposition == Disposition::Preserved));
    for item in &descriptor.items {
        assert!(item.offset + item.length <= raw.len());
    }
}

#[test]
fn malformed_h0_sample_retains_source_and_diagnostic_offset() {
    for fixture in corpus("malformed") {
        let raw = hex(fixture["descriptorHex"].as_str().unwrap());
        let descriptor = parse_descriptor(&raw).unwrap();
        assert_eq!(descriptor.raw, raw);
        assert!(
            descriptor
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.offset as u64
                    == fixture["offsetBytes"].as_u64().unwrap())
        );
        let report =
            decode_report(&descriptor, ReportKind::Input, Framing::Unnumbered, &[0]).unwrap();
        assert!(report.values.is_empty() && !report.diagnostics.is_empty());
    }
}
