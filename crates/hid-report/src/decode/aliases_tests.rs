use super::*;

#[test]
fn parsed_same_set_aliases_differ_from_sequential_usages() {
    let raw = [
        0x05, 1, 0x15, 0, 0x26, 0xff, 0, 0xa9, 1, 0x09, 0x30, 0x09, 0x31, 0xa9, 0, 0x75, 8, 0x95,
        1, 0x81, 2,
    ];
    let grouped = parse_descriptor(&raw).unwrap();
    assert!(grouped.diagnostics.is_empty(), "{:?}", grouped.diagnostics);
    let result = decode_report(&grouped, ReportKind::Input, Framing::Unnumbered, &[42]).unwrap();
    assert_eq!(result.values[0].sample, Sample::Integer("42".into()));
    assert_eq!(
        result.values[0].usages,
        vec![Usage { page: 1, id: 0x30 }, Usage { page: 1, id: 0x31 }]
    );
    let sequential = parse_descriptor(&[
        0x05, 1, 0x15, 0, 0x26, 0xff, 0, 0x09, 0x30, 0x09, 0x31, 0x75, 8, 0x95, 1, 0x81, 2,
    ])
    .unwrap();
    let result = decode_report(&sequential, ReportKind::Input, Framing::Unnumbered, &[42]).unwrap();
    assert_eq!(result.values[0].usages, vec![Usage { page: 1, id: 0x30 }]);
}

#[test]
fn grouped_range_aliases_repeat_with_a_bound_on_expanded_output() {
    let grouped = parse_descriptor(&[
        0x05, 1, 0x15, 0, 0x26, 0xff, 0, 0xa9, 1, 0x19, 0x30, 0x29, 0x32, 0xa9, 0, 0x75, 8, 0x95,
        2, 0x81, 2,
    ])
    .unwrap();
    let result = decode_report(&grouped, ReportKind::Input, Framing::Unnumbered, &[1, 2]).unwrap();
    assert_eq!(result.values.len(), 2);
    assert_eq!(
        result.values[0].usages,
        vec![
            Usage { page: 1, id: 0x30 },
            Usage { page: 1, id: 0x31 },
            Usage { page: 1, id: 0x32 }
        ]
    );
    assert_eq!(result.values[0].usages, result.values[1].usages);
    let oversized = parse_descriptor(&[
        0x05, 1, 0x15, 0, 0x26, 0xff, 0, 0xa9, 1, 0x19, 0, 0x2a, 0xff, 0xff, 0xa9, 0, 0x75, 8,
        0x95, 8, 0x81, 2,
    ])
    .unwrap();
    assert!(oversized.diagnostics.is_empty());
    assert!(decode_report(&oversized, ReportKind::Input, Framing::Unnumbered, &[0; 8]).is_err());
}

#[test]
fn parsed_array_delimiter_is_diagnosed_without_inventing_report_values() {
    let descriptor = parse_descriptor(&[
        0x05, 7, 0x15, 0, 0x25, 3, 0xa9, 1, 0x19, 0, 0x29, 3, 0xa9, 0, 0x75, 8, 0x95, 1, 0x81, 0,
    ])
    .unwrap();
    assert!(!descriptor.diagnostics.is_empty());
    let report = decode_report(&descriptor, ReportKind::Input, Framing::Unnumbered, &[1]).unwrap();
    assert!(report.values.is_empty());
    assert_eq!(report.raw, vec![1]);
}

#[test]
fn parsed_multiple_sets_address_successive_controls_and_repeat_only_the_final_set() {
    let descriptor = parse_descriptor(&[
        0x05, 1, 0x15, 0, 0x26, 0xff, 0, 0xa9, 1, 0x09, 0x30, 0x09, 0x31, 0xa9, 0, 0xa9, 1, 0x09,
        0x32, 0x09, 0x33, 0xa9, 0, 0x75, 8, 0x95, 3, 0x81, 2,
    ])
    .unwrap();
    let result = decode_report(
        &descriptor,
        ReportKind::Input,
        Framing::Unnumbered,
        &[1, 2, 3],
    )
    .unwrap();
    assert_eq!(
        result.values[0].usages,
        vec![Usage { page: 1, id: 0x30 }, Usage { page: 1, id: 0x31 }]
    );
    assert_eq!(
        result.values[1].usages,
        vec![Usage { page: 1, id: 0x32 }, Usage { page: 1, id: 0x33 }]
    );
    assert_eq!(result.values[2].usages, result.values[1].usages);
}

#[test]
fn parsed_interleaved_ordinary_controls_do_not_inherit_neighbor_alias_sets() {
    let descriptor = parse_descriptor(&[
        0x05, 1, 0x15, 0, 0x26, 0xff, 0, 0x09, 0x30, 0xa9, 1, 0x09, 0x31, 0x09, 0x32, 0xa9, 0,
        0x09, 0x33, 0x75, 8, 0x95, 4, 0x81, 2,
    ])
    .unwrap();
    let result = decode_report(
        &descriptor,
        ReportKind::Input,
        Framing::Unnumbered,
        &[1, 2, 3, 4],
    )
    .unwrap();
    assert_eq!(result.values[0].usages, vec![Usage { page: 1, id: 0x30 }]);
    assert_eq!(
        result.values[1].usages,
        vec![Usage { page: 1, id: 0x31 }, Usage { page: 1, id: 0x32 }]
    );
    assert_eq!(result.values[2].usages, vec![Usage { page: 1, id: 0x33 }]);
    assert_eq!(result.values[3].usages, result.values[2].usages);
}

#[test]
fn h0_single_preferred_plus_one_alias_group_is_narrowly_preserved() {
    let raw = [
        0x05, 1, 0x15, 0, 0x26, 0xff, 0, 0x09, 0x30, 0xa9, 1, 0x09, 0x31, 0xa9, 0, 0x75, 8, 0x95,
        1, 0x81, 2,
    ];
    let result = decode_report(
        &parse_descriptor(&raw).unwrap(),
        ReportKind::Input,
        Framing::Unnumbered,
        &[1],
    )
    .unwrap();
    assert_eq!(
        result.values[0].usages,
        vec![Usage { page: 1, id: 0x30 }, Usage { page: 1, id: 0x31 }]
    );
    let two_preferred = [
        0x05, 1, 0x15, 0, 0x26, 0xff, 0, 0x09, 0x30, 0x09, 0x31, 0xa9, 1, 0x09, 0x32, 0xa9, 0,
        0x75, 8, 0x95, 1, 0x81, 2,
    ];
    let result = decode_report(
        &parse_descriptor(&two_preferred).unwrap(),
        ReportKind::Input,
        Framing::Unnumbered,
        &[1],
    )
    .unwrap();
    assert_eq!(result.values[0].usages, vec![Usage { page: 1, id: 0x30 }]);
}

#[test]
fn empty_delimiter_groups_still_consume_lookup_work_budget() {
    let mut field = Field {
        item_index: 0,
        collection: None,
        kind: ReportKind::Input,
        offset_bits: 0,
        flags: 2,
        globals: Globals {
            size: 1,
            count: MAX_FIELDS,
            logical_min: Some(0),
            logical_max: Some(1),
            ..Default::default()
        },
        primary: Locals::default(),
        alternatives: vec![],
    };
    field.alternatives = (0..32)
        .map(|position| Locals {
            delimited: true,
            control_index: Some(position),
            ..Default::default()
        })
        .collect();
    let descriptor = Descriptor(DescriptorData {
        fields: vec![field],
        layouts: vec![Layout {
            kind: ReportKind::Input,
            report_id: None,
            payload_bits: MAX_FIELDS,
            fields: vec![0],
        }],
        ..Default::default()
    });
    assert!(decode_report(
        &descriptor,
        ReportKind::Input,
        Framing::Unnumbered,
        &vec![0; MAX_FIELDS / 8]
    )
    .is_err());
}
