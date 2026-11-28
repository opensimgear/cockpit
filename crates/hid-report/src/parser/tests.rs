use super::*;

#[test]
fn fields_offsets_kinds_padding_and_ranges_are_preserved() {
    let raw = [
        0x05, 1, 0x09, 4, 0xa1, 1, 0x15, 0, 0x25, 1, 0x75, 1, 0x95, 3, 0x05, 9, 0x19, 1, 0x29, 3,
        0x81, 2, 0x95, 5, 0x81, 1, 0x75, 8, 0x95, 1, 0x91, 2, 0xb1, 2, 0xc0,
    ];
    let d = parse_descriptor(&raw).unwrap();
    assert!(d.diagnostics.is_empty(), "{:?}", d.diagnostics);
    assert_eq!(d.raw, raw);
    assert_eq!(d.fields.len(), 4);
    assert_eq!(d.fields[1].offset_bits, 3);
    assert_eq!(d.fields[1].flags, 1);
    assert_eq!(
        d.fields[0].primary.usages,
        vec![LocalIndex::Range {
            minimum: Usage { page: 9, id: 1 },
            maximum: Usage { page: 9, id: 3 }
        }]
    );
    assert!(d.fields[1].primary.usages.is_empty());
    assert_eq!(
        d.layouts.iter().map(|l| l.payload_bits).collect::<Vec<_>>(),
        vec![8, 8, 8]
    );
    assert!(d.fields.iter().all(|f| f.collection == Some(0)));
}

#[test]
fn signed_bounds_unsigned_maxima_exponent_and_push_pop() {
    let raw = [
        0x15, 0x80, 0x25, 0x7f, 0x35, 0xff, 0x45, 0x7f, 0x55, 0x0e, 0x65, 0x11, 0x75, 8, 0x95, 1,
        0xa4, 0x15, 0, 0x26, 0xff, 0xff, 0x81, 2, 0xb4, 0x81, 2,
    ];
    let d = parse_descriptor(&raw).unwrap();
    assert!(d.diagnostics.is_empty(), "{:?}", d.diagnostics);
    assert_eq!(d.fields[0].globals.logical_max, Some(65535));
    assert_eq!(d.fields[1].globals.logical_min, Some(-128));
    assert_eq!(d.fields[1].globals.logical_max, Some(127));
    assert_eq!(d.fields[1].globals.physical_min, Some(-1));
    assert_eq!(d.fields[1].globals.exponent, Some(-2));
    assert_eq!(d.fields[1].globals.unit, Some(0x11));
}

#[test]
fn report_ids_have_independent_kind_and_id_offsets() {
    let d = parse_descriptor(&[
        0x75, 8, 0x95, 1, 0x85, 1, 0x81, 2, 0x85, 2, 0x81, 2, 0x91, 2, 0x85, 1, 0x81, 2,
    ])
    .unwrap();
    assert!(d.diagnostics.is_empty());
    assert_eq!(
        d.fields.iter().map(|f| f.offset_bits).collect::<Vec<_>>(),
        vec![0, 0, 0, 8]
    );
    assert_eq!(d.layouts.len(), 3);
}

#[test]
fn all_local_tags_and_delimiter_alternatives_are_kept_without_expansion() {
    let d = parse_descriptor(&[
        0x05, 9, 0x09, 1, 0xa9, 1, 0x09, 2, 0xa9, 0, 0xa9, 1, 0x0b, 3, 0, 0x0c, 0, 0xa9, 0, 0x39,
        4, 0x49, 5, 0x59, 9, 0x79, 2, 0x89, 3, 0x99, 8, 0x75, 1, 0x95, 1, 0x81, 2,
    ])
    .unwrap();
    assert!(d.diagnostics.is_empty(), "{:?}", d.diagnostics);
    let f = &d.fields[0];
    assert_eq!(f.primary.usages.len(), 1);
    assert_eq!(f.alternatives.len(), 2);
    assert_eq!(
        f.alternatives[1].usages,
        vec![LocalIndex::Single(Usage { page: 12, id: 3 })]
    );
    assert_eq!(
        f.primary.designators,
        vec![
            LocalIndex::Single(4),
            LocalIndex::Range {
                minimum: 5,
                maximum: 9
            }
        ]
    );
    assert_eq!(
        f.primary.strings,
        vec![
            LocalIndex::Single(2),
            LocalIndex::Range {
                minimum: 3,
                maximum: 8
            }
        ]
    );
    let huge = parse_descriptor(&[
        0x05, 9, 0x19, 0, 0x2a, 0xff, 0xff, 0x75, 1, 0x95, 1, 0x81, 2,
    ])
    .unwrap();
    assert_eq!(huge.fields[0].primary.usages.len(), 1);
}

#[test]
fn every_input_byte_is_covered_including_long_reserved_and_truncation() {
    for raw in [
        &[0xfe, 3, 0x77, 1, 2, 3, 0x0c][..],
        &[0xfe][..],
        &[0xfe, 2][..],
        &[0xfe, 3, 5, 9][..],
        &[0x77, 1, 2][..],
    ] {
        let d = parse_descriptor(raw).unwrap();
        assert_eq!(d.raw, raw);
        assert_eq!(
            d.items.iter().map(|item| item.length).sum::<usize>(),
            raw.len()
        );
        assert!(!d.diagnostics.is_empty());
    }
}

#[test]
fn malformed_structures_remain_inspectable_but_invalid() {
    for raw in [
        &[0xb4][..],
        &[0xa4][..],
        &[0xc0][..],
        &[0xa1, 1][..],
        &[0x85, 0][..],
        &[0xa9, 2][..],
        &[0xa9, 0][..],
        &[0xa9, 1, 0xa9, 1][..],
        &[0x19, 3, 0x29, 1][..],
        &[0x19, 1][..],
        &[0x81, 2][..],
        &[0x75, 8, 0x95, 1, 0x81, 2, 0x85, 1, 0x81, 2][..],
        &[0x15, 5, 0x25, 1, 0x75, 8, 0x95, 1, 0x81, 2][..],
    ] {
        let d = parse_descriptor(raw).unwrap();
        assert!(!d.diagnostics.is_empty(), "accepted malformed {raw:x?}");
        assert_eq!(d.raw, raw);
    }
}

#[test]
fn structural_and_expansion_budgets_fail_before_large_allocation() {
    assert!(parse_descriptor(&vec![0; MAX_DESCRIPTOR_BYTES + 1]).is_err());
    assert!(parse_descriptor(&[0xa4; MAX_DEPTH + 1]).is_err());
    let too_many = [0x75, 1, 0x97, 1, 0x40, 0, 0, 0x81, 2]; // 16385 elements.
    assert!(parse_descriptor(&too_many).is_err());
    assert!(parse_descriptor(&[0x77, 1, 0x10, 0, 0, 0x95, 1, 0x81, 2]).is_err());
    // Buffered bytes represent one grouped value rather than thousands of scalars.
    let buffered = parse_descriptor(&[0x75, 8, 0x97, 0, 0, 1, 0, 0x82, 0, 1]).unwrap();
    assert!(buffered.diagnostics.is_empty());
    assert_eq!(buffered.layouts[0].payload_bits, MAX_REPORT_BYTES * 8);
    assert!(parse_descriptor(&[0x75, 8, 0x97, 1, 0, 1, 0, 0x82, 0, 1]).is_err());
}

#[test]
fn diagnostics_are_bounded_and_empty_descriptor_is_invalid() {
    let d = parse_descriptor(&vec![0xc0; 4096]).unwrap();
    assert!(d.diagnostics.len() <= 64);
    assert!(!parse_descriptor(&[]).unwrap().diagnostics.is_empty());
}

#[test]
fn opaque_well_formed_items_do_not_block_known_fields() {
    let raw = [
        0xfe, 2, 0x72, 0xaa, 0xbb, 0x0c, 0xfc, 0xf4, 0xf8, 0x75, 8, 0x95, 1, 0x81, 2,
    ];
    let d = parse_descriptor(&raw).unwrap();
    assert!(d.diagnostics.is_empty(), "{:?}", d.diagnostics);
    assert_eq!(
        d.items
            .iter()
            .filter(|item| item.disposition == Disposition::Preserved)
            .count(),
        5
    );
    assert_eq!(d.fields[0].offset_bits, 0);
    assert_eq!(d.raw, raw);
}

#[test]
fn short_usages_resolve_at_main_and_delimiter_empties_survive() {
    let d = parse_descriptor(&[
        0x05, 1, 0x09, 0x30, 0x05, 9, 0x75, 1, 0x95, 1, 0x81, 2, 0xa9, 1, 0xa9, 0, 0xa9, 1, 0x09,
        3, 0xa9, 0, 0x81, 2,
    ])
    .unwrap();
    assert!(d.diagnostics.is_empty(), "{:?}", d.diagnostics);
    assert_eq!(
        d.fields[0].primary.usages,
        vec![LocalIndex::Single(Usage { page: 9, id: 0x30 })]
    );
    assert!(d.fields[1].primary.usages.is_empty());
    assert_eq!(d.fields[1].alternatives.len(), 1);
    assert_eq!(
        d.fields[1].alternatives[0].usages,
        vec![LocalIndex::Single(Usage { page: 9, id: 3 })]
    );
}

#[test]
fn first_delimited_preference_keeps_outside_string_metadata() {
    let d = parse_descriptor(&[
        0x79, 2, 0xa9, 1, 0x09, 7, 0xa9, 0, 0x75, 1, 0x95, 1, 0x81, 2,
    ])
    .unwrap();
    assert!(d.diagnostics.is_empty());
    assert_eq!(
        d.fields[0].primary.usages,
        vec![LocalIndex::Single(Usage { page: 0, id: 7 })]
    );
    assert_eq!(d.fields[0].primary.strings, vec![LocalIndex::Single(2)]);
    assert!(d.fields[0].alternatives.is_empty());
}

#[test]
fn total_elements_and_constants_share_one_budget_across_reports() {
    let mut raw = vec![0x75, 1, 0x96, 0, 0x40, 0x81, 1]; // 16384 constant elements.
    raw.extend([0x85, 2, 0x95, 1, 0x81, 2]);
    assert!(parse_descriptor(&raw).is_err());
    let mut collections = Vec::new();
    for _ in 0..MAX_DEPTH + 1 {
        collections.extend([0xa1, 0]);
    }
    assert!(parse_descriptor(&collections).is_err());
}

#[test]
fn forbidden_delimiter_contexts_are_invalid() {
    for raw in [
        &[0xa9, 1, 0x09, 1, 0xa9, 0, 0xa1, 1, 0xc0][..],
        &[0xa9, 1, 0x09, 1, 0xa9, 0, 0x75, 8, 0x95, 1, 0x81, 0][..],
        &[0xa9, 1, 0x39, 1, 0xa9, 0, 0x75, 8, 0x95, 1, 0x81, 2][..],
    ] {
        assert!(!parse_descriptor(raw).unwrap().diagnostics.is_empty());
    }
}

#[test]
fn deterministic_arbitrary_bytes_never_panic_or_lose_item_coverage() {
    let mut state = 0xa73e_8b21u32;
    for length in 0..512 {
        let raw: Vec<_> = (0..length)
            .map(|_| {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                (state >> 24) as u8
            })
            .collect();
        if let Ok(d) = parse_descriptor(&raw) {
            assert_eq!(d.raw, raw);
            assert!(d.diagnostics.len() <= 64);
            let mut next = 0;
            for item in &d.items {
                assert_eq!(item.offset, next);
                assert!(item.length > 0);
                next += item.length;
            }
            assert_eq!(next, raw.len());
            assert!(d
                .layouts
                .iter()
                .all(|layout| layout.payload_bits <= MAX_REPORT_BYTES * 8));
        }
    }
}

#[test]
fn aliases_in_one_delimiter_set_retain_distinct_group_provenance() {
    let raw = [
        0x05, 1, 0xa9, 1, 0x09, 0x30, 0x09, 0x31, 0xa9, 0, 0x75, 8, 0x95, 1, 0x81, 2,
    ];
    let d = parse_descriptor(&raw).unwrap();
    assert!(d.diagnostics.is_empty());
    assert!(d.fields[0].primary.delimited);
    assert_eq!(
        d.fields[0].primary.usages,
        vec![
            LocalIndex::Single(Usage { page: 1, id: 0x30 }),
            LocalIndex::Single(Usage { page: 1, id: 0x31 })
        ]
    );
    let sequential =
        parse_descriptor(&[0x05, 1, 0x09, 0x30, 0x09, 0x31, 0x75, 8, 0x95, 1, 0x81, 2]).unwrap();
    assert!(!sequential.fields[0].primary.delimited);
}

#[test]
fn ordered_groups_interleave_with_compact_ordinary_ranges() {
    let d = parse_descriptor(&[
        0x05, 9, 0x19, 1, 0x29, 3, 0xa9, 1, 0x09, 7, 0x09, 8, 0xa9, 0, 0x09, 4, 0xa9, 1, 0xa9, 0,
        0x75, 1, 0x95, 6, 0x81, 2,
    ])
    .unwrap();
    assert!(d.diagnostics.is_empty(), "{:?}", d.diagnostics);
    let f = &d.fields[0];
    assert!(!f.primary.delimited);
    assert_eq!(f.primary.control_index, None);
    assert_eq!(f.primary.usages.len(), 2);
    assert_eq!(f.alternatives[0].control_index, Some(3));
    assert_eq!(f.alternatives[1].control_index, Some(5));
    assert!(f.alternatives[1].usages.is_empty());
}

#[test]
fn distinct_alias_groups_occupy_distinct_slots_even_when_count_is_one() {
    for count in [1, 2] {
        let d = parse_descriptor(&[
            0xa9, 1, 0x09, 1, 0x09, 2, 0xa9, 0, 0xa9, 1, 0x09, 3, 0x09, 4, 0xa9, 0, 0x75, 8, 0x95,
            count, 0x81, 2,
        ])
        .unwrap();
        assert!(d.diagnostics.is_empty());
        assert_eq!(d.fields[0].primary.control_index, Some(0));
        assert_eq!(d.fields[0].alternatives[0].control_index, Some(1));
    }
}

#[test]
fn only_exact_h0_preferred_outside_form_overlays_one_alias_slot() {
    let d = parse_descriptor(&[
        0x09, 1, 0xa9, 1, 0x09, 7, 0x09, 8, 0xa9, 0, 0x75, 8, 0x95, 1, 0x81, 2,
    ])
    .unwrap();
    assert_eq!(d.fields[0].alternatives[0].control_index, Some(0));
    let multiple = parse_descriptor(&[
        0x09, 1, 0x09, 2, 0xa9, 1, 0x09, 7, 0xa9, 0, 0x75, 8, 0x95, 1, 0x81, 2,
    ])
    .unwrap();
    assert_eq!(multiple.fields[0].alternatives[0].control_index, Some(2));
}
