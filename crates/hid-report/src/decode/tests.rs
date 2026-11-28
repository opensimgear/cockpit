use super::*;

fn field(size: usize, count: usize, flags: u32) -> Field {
    Field {
        item_index: 0,
        collection: None,
        kind: ReportKind::Input,
        offset_bits: 0,
        flags,
        globals: Globals {
            size,
            count,
            logical_min: Some(0),
            logical_max: Some(255),
            ..Default::default()
        },
        primary: Locals::default(),
        alternatives: vec![],
    }
}
fn descriptor(fields: Vec<Field>) -> Descriptor {
    let mut layouts: Vec<Layout> = vec![];
    for (index, field) in fields.iter().enumerate() {
        let position = layouts.iter().position(|layout| {
            layout.kind == field.kind && layout.report_id == field.globals.report_id
        });
        let layout = if let Some(position) = position {
            &mut layouts[position]
        } else {
            layouts.push(Layout {
                kind: field.kind,
                report_id: field.globals.report_id,
                payload_bits: 0,
                fields: vec![],
            });
            layouts.last_mut().unwrap()
        };
        layout.payload_bits = layout
            .payload_bits
            .max(field.offset_bits + field.globals.size * field.globals.count);
        layout.fields.push(index);
    }
    Descriptor(DescriptorData {
        fields,
        layouts,
        ..Default::default()
    })
}
fn decode(fields: Vec<Field>, wire: &[u8]) -> DecodedReport {
    decode_report(
        &descriptor(fields),
        ReportKind::Input,
        Framing::Unnumbered,
        wire,
    )
    .unwrap()
}
fn range(page: u16, low: u16, high: u16) -> Locals {
    Locals {
        usages: vec![LocalIndex::Range {
            minimum: Usage { page, id: low },
            maximum: Usage { page, id: high },
        }],
        ..Default::default()
    }
}

#[test]
fn framing_keeps_original_delivery_and_never_treats_id_as_payload() {
    let mut f = field(8, 1, 2);
    f.globals.report_id = Some(7);
    let descriptor = descriptor(vec![f]);
    for (framing, wire) in [
        (Framing::IdPrefix, vec![7, 42, 0xfe]),
        (Framing::OutOfBand(Some(7)), vec![42, 0xfe]),
    ] {
        let report = decode_report(&descriptor, ReportKind::Input, framing, &wire).unwrap();
        assert_eq!(report.raw, wire);
        assert_eq!(report.report_id, Some(7));
        assert_eq!(report.values[0].sample, Sample::Integer("42".into()));
        assert!(report.diagnostics[0].reason.contains("Trailing"));
    }
    let unnumbered = super::tests::descriptor(vec![field(8, 1, 2)]);
    for (framing, wire) in [
        (Framing::Unnumbered, vec![42]),
        (Framing::OsZeroPrefix, vec![0, 42]),
        (Framing::OutOfBand(None), vec![42]),
    ] {
        let report = decode_report(&unnumbered, ReportKind::Input, framing, &wire).unwrap();
        assert_eq!(report.raw, wire);
        assert_eq!(report.values[0].raw_bits, vec![42]);
    }
    for (framing, wire) in [
        (Framing::IdPrefix, vec![]),
        (Framing::IdPrefix, vec![0, 1]),
        (Framing::OsZeroPrefix, vec![3, 1]),
        (Framing::OutOfBand(Some(0)), vec![1]),
    ] {
        let report = decode_report(&descriptor, ReportKind::Input, framing, &wire).unwrap();
        assert_eq!(report.raw, wire);
        assert!(report.values.is_empty());
        assert!(!report.diagnostics.is_empty());
    }
}

#[test]
fn kinds_share_ids_without_sharing_layout_or_value_extent() {
    let mut input = field(8, 1, 2);
    input.globals.report_id = Some(5);
    let mut output = input.clone();
    output.kind = ReportKind::Output;
    output.globals.size = 16;
    let descriptor = descriptor(vec![input, output]);
    let decoded = decode_report(
        &descriptor,
        ReportKind::Output,
        Framing::IdPrefix,
        &[5, 0xff, 0xff],
    )
    .unwrap();
    assert_eq!(decoded.values[0].sample, Sample::Invalid("65535".into()));
    assert!(
        decode_report(&descriptor, ReportKind::Feature, Framing::IdPrefix, &[5, 0])
            .unwrap()
            .values
            .is_empty()
    );
}

#[test]
fn truncation_and_descriptor_failures_never_emit_partial_values() {
    let mut f = field(8, 2, 2);
    f.primary = range(1, 0x30, 0x31);
    let mut descriptor = descriptor(vec![f]);
    let short = decode_report(&descriptor, ReportKind::Input, Framing::Unnumbered, &[42]).unwrap();
    assert_eq!(short.raw, vec![42]);
    assert!(short.values.is_empty());
    descriptor
        .0
        .diagnostics
        .push(diagnostic("Malformed descriptor tail"));
    assert!(
        decode_report(&descriptor, ReportKind::Input, Framing::Unnumbered, &[1, 2])
            .unwrap()
            .values
            .is_empty()
    );
    descriptor.0.diagnostics.clear();
    descriptor.0.layouts[0].fields.push(99);
    assert!(
        decode_report(&descriptor, ReportKind::Input, Framing::Unnumbered, &[1, 2])
            .unwrap()
            .values
            .is_empty()
    );
    descriptor.0.layouts[0].fields = vec![0];
    descriptor.0.fields[0].globals.logical_min = Some(256);
    assert!(
        decode_report(&descriptor, ReportKind::Input, Framing::Unnumbered, &[1, 2])
            .unwrap()
            .values
            .is_empty()
    );
}

#[test]
fn cross_byte_signed_and_arbitrary_width_values_are_exact() {
    let mut f = field(12, 2, 2);
    f.globals.logical_min = Some(-2048);
    f.globals.logical_max = Some(2047);
    let report = decode(vec![f], &[0x85, 0xff, 0x3f]);
    assert_eq!(report.values[0].sample, Sample::Integer("-123".into()));
    assert_eq!(report.values[0].raw_bits, vec![0x85, 0x0f]);
    assert_eq!(report.values[1].sample, Sample::Integer("1023".into()));
    let wide = decode(vec![field(257, 1, 2)], &[0xff; 33]);
    assert_eq!(
        wide.values[0].sample,
        Sample::Invalid(((BigInt::from(1u8) << 257usize) - 1u8).to_string())
    );
    assert_eq!(wide.values[0].raw_bits[32], 1);
    let mut signed = field(64, 1, 2);
    signed.globals.logical_min = Some(i64::MIN);
    signed.globals.logical_max = Some(i64::MAX);
    let minimum = decode(vec![signed], &[0, 0, 0, 0, 0, 0, 0, 0x80]);
    assert_eq!(
        minimum.values[0].sample,
        Sample::Integer(i64::MIN.to_string())
    );
}

#[test]
fn arrays_use_logical_origin_and_compact_alternate_usage_branches() {
    let mut f = field(8, 3, 0);
    f.globals.logical_min = Some(1);
    f.globals.logical_max = Some(3);
    f.primary = range(7, 4, 6);
    f.alternatives = vec![range(0xff00, 100, 102), range(7, 4, 6)];
    let report = decode(vec![f], &[2, 2, 3]);
    let expected = vec![
        Usage { page: 7, id: 5 },
        Usage {
            page: 0xff00,
            id: 101,
        },
    ];
    assert_eq!(
        report.values[0].sample,
        Sample::Array {
            selector: "2".into(),
            usages: expected.clone()
        }
    );
    assert_eq!(report.values[0].usages, expected);
    assert_eq!(report.values[0].sample, report.values[1].sample);
    assert_eq!(report.values[1].element, 1);
    assert_eq!(report.values[2].usages[0].id, 6);
    assert_eq!(report.values[0].semantics, Semantics::Event);
}

#[test]
fn variable_fields_assign_usages_by_index_and_repeat_last_declared_usage() {
    let mut f = field(8, 4, 6);
    f.primary = range(1, 0x30, 0x31);
    f.alternatives = vec![Locals {
        usages: vec![LocalIndex::Single(Usage {
            page: 0xff00,
            id: 9,
        })],
        ..Default::default()
    }];
    let report = decode(vec![f], &[7, 7, 7, 7]);
    assert_eq!(
        report
            .values
            .iter()
            .map(|value| value.usages[0].id)
            .collect::<Vec<_>>(),
        vec![0x30, 0x31, 0x31, 0x31]
    );
    assert!(report
        .values
        .iter()
        .all(|value| value.usages[1].id == 9 && value.semantics == Semantics::Relative));
}

#[test]
fn constants_preserve_each_element_and_buffered_data_stays_grouped() {
    let mut constant = field(3, 2, 1);
    constant.globals.logical_max = Some(1);
    let report = decode(vec![constant], &[0x3d]);
    assert_eq!(report.values.len(), 2);
    assert_eq!(report.values[0].sample, Sample::Bytes(vec![5]));
    assert_eq!(report.values[1].sample, Sample::Bytes(vec![7]));
    assert_eq!(report.values[1].element, 1);
    assert_eq!(report.values[1].semantics, Semantics::Opaque);
    let buffered = decode(vec![field(8, 3, 0x102)], &[1, 2, 3]);
    assert_eq!(buffered.values.len(), 1);
    assert_eq!(buffered.values[0].sample, Sample::Bytes(vec![1, 2, 3]));
}

#[test]
fn maximum_opaque_buffers_and_constant_bits_never_require_decimal_conversion() {
    let wire: Vec<u8> = (0..MAX_REPORT_BYTES)
        .map(|index| (index % 251) as u8)
        .collect();
    for f in [
        field(8, MAX_REPORT_BYTES, 0x102),
        field(MAX_REPORT_BYTES * 8, 1, 1),
    ] {
        let report = decode(vec![f], &wire);
        assert_eq!(report.raw, wire);
        assert_eq!(report.values.len(), 1);
        assert_eq!(report.values[0].sample, Sample::Bytes(wire.clone()));
        assert_eq!(report.values[0].raw_bits, wire);
        assert_eq!(report.values[0].semantics, Semantics::Opaque);
    }
}

#[test]
fn null_invalid_and_unknown_keep_encoded_number_instead_of_inventing_value() {
    let mut hat = field(4, 1, 0x42);
    hat.globals.logical_max = Some(7);
    hat.primary = range(1, 0x39, 0x39);
    assert_eq!(
        decode(vec![hat.clone()], &[8]).values[0].usages,
        vec![Usage { page: 1, id: 0x39 }]
    );
    assert_eq!(
        decode(vec![hat.clone()], &[8]).values[0].sample,
        Sample::Null("8".into())
    );
    hat.flags = 2;
    assert_eq!(
        decode(vec![hat.clone()], &[8]).values[0].usages,
        vec![Usage { page: 1, id: 0x39 }]
    );
    assert_eq!(
        decode(vec![hat], &[8]).values[0].sample,
        Sample::Invalid("8".into())
    );
    let mut unknown = field(8, 1, 2);
    unknown.globals.logical_min = None;
    unknown.primary = range(1, 0x30, 0x30);
    assert_eq!(
        decode(vec![unknown.clone()], &[12]).values[0].usages,
        vec![Usage { page: 1, id: 0x30 }]
    );
    assert!(
        matches!(&decode(vec![unknown],&[12]).values[0].sample,Sample::Unknown {encoded:Some(value),..} if value=="12")
    );
    assert!(
        matches!(&decode(vec![field(8,1,0)],&[12]).values[0].sample,Sample::Unknown {encoded:Some(value),..} if value=="12")
    );
}

#[test]
fn physical_scaling_is_reduced_signed_rational_with_unit_metadata() {
    let mut f = field(8, 1, 2);
    f.globals.logical_max = Some(3);
    f.globals.physical_min = Some(-100);
    f.globals.physical_max = Some(100);
    f.globals.unit = Some(20);
    f.globals.exponent = Some(-2);
    let value = &decode(vec![f.clone()], &[1]).values[0];
    assert_eq!(
        value.physical,
        Some(Physical {
            numerator: "-100".into(),
            denominator: "3".into(),
            unit: 20,
            exponent: -2
        })
    );
    assert_eq!(
        decode(vec![f.clone()], &[0]).values[0]
            .physical
            .as_ref()
            .unwrap()
            .denominator,
        "1"
    );
    f.flags |= 0x10;
    assert!(decode(vec![f.clone()], &[1]).values[0].physical.is_none());
    f.flags &= !0x10;
    f.globals.physical_min = Some(0);
    f.globals.physical_max = Some(0);
    assert!(decode(vec![f], &[1]).values[0].physical.is_none());
}

#[test]
fn value_and_lookup_budgets_fail_closed_without_count_times_usage_expansion() {
    assert!(decode_report(
        &descriptor(vec![field(8, 1, 2)]),
        ReportKind::Input,
        Framing::Unnumbered,
        &vec![0; MAX_REPORT_BYTES + 1]
    )
    .is_err());
    let oversized = field(MAX_SCALAR_BITS + 1, 1, 2);
    assert!(decode_report(
        &descriptor(vec![oversized]),
        ReportKind::Input,
        Framing::Unnumbered,
        &vec![0; 513]
    )
    .is_err());
    let many = field(1, MAX_FIELDS + 1, 2);
    assert!(decode_report(
        &descriptor(vec![many]),
        ReportKind::Input,
        Framing::Unnumbered,
        &vec![0; (MAX_FIELDS + 8) / 8]
    )
    .is_err());
    let mut branches = field(8, 512, 0);
    branches.primary = range(1, 0, 255);
    branches.alternatives = vec![range(1, 0, 255); 512];
    assert!(decode_report(
        &descriptor(vec![branches]),
        ReportKind::Input,
        Framing::Unnumbered,
        &vec![0; 512]
    )
    .is_err());
    let mut compact = field(16, 1024, 0);
    compact.globals.logical_max = Some(65535);
    compact.primary = range(0xff00, 0, 65535);
    let report = decode(vec![compact], &vec![0xff; 2048]);
    assert_eq!(report.values.len(), 1024);
    assert_eq!(report.values[0].usages[0].id, 65535);
}

#[test]
fn malformed_usage_ranges_fence_already_prepared_values() {
    let first = field(8, 1, 2);
    let mut second = field(8, 1, 2);
    second.offset_bits = 8;
    second.primary = range(1, 5, 4);
    let report = decode(vec![first, second], &[1, 2]);
    assert!(report.values.is_empty());
    assert!(!report.diagnostics.is_empty());
}
