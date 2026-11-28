mod support;
use osg_hid_report::*;
use support::{decimal, put_bits, Generator};

fn global(descriptor: &mut Vec<u8>, tag: u8, value: u32) {
    descriptor.push((tag << 4) | 0x07);
    descriptor.extend(value.to_le_bytes());
}
fn scalar_descriptor(width: usize, count: usize, padding: usize, signed: bool) -> Vec<u8> {
    let mut descriptor = vec![0x06, 0, 0xff, 0x09, 1, 0xa1, 1];
    let logical_width = width.clamp(1, 32);
    let minimum = if signed {
        -(1_i64 << (logical_width - 1))
    } else {
        0
    };
    let maximum = (1_i64 << (logical_width - usize::from(signed))) - 1;
    global(&mut descriptor, 1, minimum as u32);
    global(&mut descriptor, 2, maximum as u32);
    if padding > 0 {
        global(&mut descriptor, 7, padding as u32);
        global(&mut descriptor, 9, 1);
        descriptor.extend([0x81, 1]);
    }
    descriptor.extend([0x09, 2]);
    global(&mut descriptor, 7, width as u32);
    global(&mut descriptor, 9, count as u32);
    descriptor.extend([0x81, 2, 0xc0]);
    descriptor
}

#[test]
fn deterministic_generated_unaligned_scalars_match_independent_decimal_oracle() {
    let mut generator = Generator(0x0017_9178_4096);
    let widths = [
        1, 2, 3, 7, 8, 9, 13, 16, 24, 31, 32, 33, 63, 64, 65, 127, 128, 255, 1023, 4096,
    ];
    for case in 0..160 {
        let width = widths[case % widths.len()];
        let padding = generator.next() as usize % 8;
        let count = (generator.next() % 3 + 1) as usize;
        let signed = generator.next() & 1 != 0;
        let raw = scalar_descriptor(width, count, padding, signed);
        let descriptor = parse_descriptor(&raw).unwrap();
        assert!(
            descriptor.diagnostics.is_empty(),
            "case {case}: {:?}",
            descriptor.diagnostics
        );
        let mut payload = vec![0_u8; (padding + width * count).div_ceil(8)];
        let mut expected = Vec::new();
        for element in 0..count {
            let mut bytes = generator.bytes(width.div_ceil(8));
            if width % 8 != 0 {
                *bytes.last_mut().unwrap() &= (1 << (width % 8)) - 1;
            }
            put_bits(&mut payload, padding + width * element, width, &bytes);
            expected.push(decimal(&bytes, 0, width, signed));
        }
        let first = decode_report(
            &descriptor,
            ReportKind::Input,
            Framing::Unnumbered,
            &payload,
        )
        .unwrap();
        assert!(
            first.diagnostics.is_empty(),
            "case {case}: {:?}",
            first.diagnostics
        );
        let field = usize::from(padding != 0);
        for (element, expected) in expected.iter().enumerate() {
            let value = first
                .values
                .iter()
                .find(|value| value.field == field && value.element == element)
                .unwrap();
            assert_eq!(
                decimal(&value.raw_bits, 0, width, signed),
                *expected,
                "case {case}, element {element}"
            );
            let encoded = match &value.sample {
                Sample::Integer(value) | Sample::Invalid(value) | Sample::Null(value) => value,
                other => panic!("case {case}: scalar sample {other:?}"),
            };
            assert_eq!(encoded, expected);
            assert_eq!(value.raw_bits.len(), width.div_ceil(8));
        }
        assert_eq!(
            first,
            decode_report(
                &descriptor,
                ReportKind::Input,
                Framing::Unnumbered,
                &payload
            )
            .unwrap()
        );
        assert_eq!(descriptor, parse_descriptor(&raw).unwrap());
        assert_eq!(first.raw, payload);
    }
}

#[test]
fn maximum_scalar_preserves_positive_negative_and_sparse_low_bits_exactly() {
    for signed in [false, true] {
        let descriptor =
            parse_descriptor(&scalar_descriptor(MAX_SCALAR_BITS, 1, 0, signed)).unwrap();
        for mut payload in [vec![0; 512], vec![0xff; 512], vec![0x80; 512]] {
            payload[0] |= 1;
            let expected = decimal(&payload, 0, 4096, signed);
            let decoded = decode_report(
                &descriptor,
                ReportKind::Input,
                Framing::Unnumbered,
                &payload,
            )
            .unwrap();
            assert!(decoded.diagnostics.is_empty());
            let value = &decoded.values[0];
            assert_eq!(value.raw_bits, payload);
            let actual = match &value.sample {
                Sample::Integer(value) | Sample::Invalid(value) => value,
                other => panic!("wide scalar {other:?}"),
            };
            assert_eq!(actual, &expected);
            assert!(actual.len() > 1_000 || actual == "1" || actual == "-1");
            let exported = serde_json::to_value(&decoded).unwrap();
            assert!(exported.to_string().contains(actual));
        }
    }
}

#[test]
fn framing_kind_lengths_and_unknown_ids_never_reuse_a_prior_report() {
    let raw = [
        0x06, 0, 0xff, 0x09, 1, 0xa1, 1, 0x85, 7, 0x15, 0, 0x25, 0x7f, 0x75, 8, 0x95, 1, 0x81, 2,
        0xc0,
    ];
    let descriptor = parse_descriptor(&raw).unwrap();
    let good = decode_report(&descriptor, ReportKind::Input, Framing::IdPrefix, &[7, 42]).unwrap();
    assert_eq!(good.values.len(), 1);
    let out_of_band = decode_report(
        &descriptor,
        ReportKind::Input,
        Framing::OutOfBand(Some(7)),
        &[42],
    )
    .unwrap();
    assert_eq!(good.values, out_of_band.values);
    for (kind, framing, wire) in [
        (ReportKind::Input, Framing::IdPrefix, vec![]),
        (ReportKind::Input, Framing::IdPrefix, vec![7]),
        (ReportKind::Input, Framing::IdPrefix, vec![8, 42]),
        (ReportKind::Feature, Framing::IdPrefix, vec![7, 42]),
        (ReportKind::Input, Framing::Unnumbered, vec![42]),
    ] {
        let decoded = decode_report(&descriptor, kind, framing, &wire).unwrap();
        assert_eq!(decoded.raw, wire);
        assert!(decoded.values.is_empty() && !decoded.diagnostics.is_empty());
    }
    let padded = decode_report(
        &descriptor,
        ReportKind::Input,
        Framing::IdPrefix,
        &[7, 42, 0],
    )
    .unwrap();
    assert_eq!(padded.raw, [7, 42, 0]);
    assert_eq!(
        padded.values, good.values,
        "trailing delivery padding is not another declared value"
    );
    assert!(
        !padded.diagnostics.is_empty(),
        "extra delivery bytes are explicit"
    );
    let unnumbered = parse_descriptor(&scalar_descriptor(8, 1, 0, false)).unwrap();
    let plain = decode_report(&unnumbered, ReportKind::Input, Framing::Unnumbered, &[42]).unwrap();
    let os_zero = decode_report(
        &unnumbered,
        ReportKind::Input,
        Framing::OsZeroPrefix,
        &[0, 42],
    )
    .unwrap();
    assert_eq!(plain.values, os_zero.values);
    assert_eq!(os_zero.raw, [0, 42]);
    assert_eq!(plain.report_id, None);
}

#[test]
fn declared_budgets_are_rejected_before_expansion() {
    assert!(parse_descriptor(&vec![0; MAX_DESCRIPTOR_BYTES + 1]).is_err());
    assert!(parse_descriptor(&scalar_descriptor(MAX_SCALAR_BITS + 1, 1, 0, false)).is_err());
    assert!(parse_descriptor(&scalar_descriptor(1, MAX_FIELDS + 1, 0, false)).is_err());
    assert!(
        parse_descriptor(&scalar_descriptor(4096, 129, 0, false)).is_err(),
        "aggregate layout exceeds report budget"
    );
    let descriptor = parse_descriptor(&scalar_descriptor(8, 1, 0, false)).unwrap();
    assert!(decode_report(
        &descriptor,
        ReportKind::Input,
        Framing::Unnumbered,
        &vec![0; MAX_REPORT_BYTES + 1]
    )
    .is_err());
    for prefix in [vec![0xa4; MAX_DEPTH + 1], [0xa1, 1].repeat(MAX_DEPTH + 1)] {
        assert!(
            parse_descriptor(&prefix).is_err(),
            "bounded global/collection stack"
        );
    }
}

#[test]
fn exact_scalar_count_and_payload_boundaries_remain_usable() {
    let dense = parse_descriptor(&scalar_descriptor(1, MAX_FIELDS, 0, false)).unwrap();
    assert!(dense.diagnostics.is_empty());
    let decoded = decode_report(
        &dense,
        ReportKind::Input,
        Framing::Unnumbered,
        &vec![0; MAX_FIELDS / 8],
    )
    .unwrap();
    assert!(decoded.diagnostics.is_empty());
    assert_eq!(decoded.values.len(), MAX_FIELDS);
    let wide = parse_descriptor(&scalar_descriptor(MAX_SCALAR_BITS, 128, 0, false)).unwrap();
    assert!(wide.diagnostics.is_empty());
    assert_eq!(wide.layouts[0].payload_bits, MAX_REPORT_BYTES * 8);
    let decoded = decode_report(
        &wide,
        ReportKind::Input,
        Framing::Unnumbered,
        &vec![0; MAX_REPORT_BYTES],
    )
    .unwrap();
    assert!(decoded.diagnostics.is_empty());
    assert_eq!(decoded.values.len(), 128);
    assert!(decoded
        .values
        .iter()
        .all(|value| value.raw_bits.len() == 512 && value.sample == Sample::Integer("0".into())));
}

#[test]
fn malformed_item_streams_preserve_bytes_and_never_panic() {
    let examples: &[&[u8]] = &[
        &[0x27, 0, 0],
        &[0xfe],
        &[0xfe, 3],
        &[0xfe, 3, 0, 0xaa],
        &[0xb4],
        &[0xc0],
        &[0x85, 0],
        &[0xa9, 1, 0xa9, 1],
    ];
    for raw in examples {
        let descriptor = parse_descriptor(raw).unwrap();
        assert_eq!(descriptor.raw, *raw);
        assert!(!descriptor.diagnostics.is_empty(), "{raw:02x?}");
    }
    let valid = scalar_descriptor(13, 1, 3, true);
    for end in 0..valid.len() {
        let truncated = &valid[..end];
        let result = std::panic::catch_unwind(|| parse_descriptor(truncated));
        assert!(result.is_ok(), "prefix length {end}");
        if let Ok(descriptor) = result.unwrap() {
            assert_eq!(descriptor.raw, truncated);
        }
    }
    let mut generator = Generator(0x179_bad);
    for length in 0..256 {
        let bytes = generator.bytes(length);
        let result = std::panic::catch_unwind(|| parse_descriptor(&bytes));
        assert!(result.is_ok(), "generated stream length {length}");
        if let Ok(descriptor) = result.unwrap() {
            assert_eq!(descriptor.raw, bytes);
            let decoded = std::panic::catch_unwind(|| {
                decode_report(&descriptor, ReportKind::Input, Framing::Unnumbered, &[])
            });
            assert!(
                decoded.is_ok(),
                "generated descriptor/report must not panic"
            );
            if let Ok(report) = decoded.unwrap() {
                assert!(report.raw.is_empty());
            }
        }
    }
}
