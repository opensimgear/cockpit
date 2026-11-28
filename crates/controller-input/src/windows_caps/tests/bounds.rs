use super::*;

#[test]
fn missing_and_inconsistent_link_metadata_remain_inspectable() {
    setup();
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.caps.NumberLinkCollectionNodes = 2;
        s.links = vec![
            HIDP_LINK_COLLECTION_NODE::default(),
            HIDP_LINK_COLLECTION_NODE {
                Parent: 7,
                ..Default::default()
            },
        ];
    });
    let p = unsafe { snapshot_with(&api(), 1) };
    assert_eq!(p.links.len(), 2);
    assert!(p.failures.iter().any(|f| f.stage == "link-indices"));
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.links[1].Parent = 0;
        s.links[0].NumberOfChildren = 1;
    });
    assert!(unsafe { snapshot_with(&api(), 1) }
        .failures
        .iter()
        .any(|f| f.stage == "link-children"));
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.caps.NumberLinkCollectionNodes = MAX_LINKS as u16 + 1;
        s.calls.clear();
    });
    let p = unsafe { snapshot_with(&api(), 1) };
    assert!(!p.complete);
    assert!(!STATE.with(|s| s.borrow().calls.contains(&"links".into())));
    assert!(!Snapshot::unavailable("not exposed").available);
    let mut p = Snapshot::unavailable("not exposed");
    for _ in 0..100 {
        p.fail("test", None, None, "bounded");
    }
    assert_eq!(p.failures.len(), MAX_FAILURES);
}

#[test]
fn range_signedness_report_length_and_partial_counts_are_explicit() {
    basic();
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.values[0][0].LogicalMin = -10;
        s.values[0][0].LogicalMax = -1;
        s.values[0][0].PhysicalMin = -20;
        s.values[0][0].PhysicalMax = -2;
    });
    let p = unsafe { snapshot_with(&api(), 1) };
    assert_eq!(p.reports[0].value_caps[0].logical_max, -1);
    assert_eq!(p.reports[0].value_caps[0].physical_max, -2);
    STATE.with(|s| s.borrow_mut().caps.InputReportByteLength = 0);
    let p = unsafe { snapshot_with(&api(), 1) };
    assert!(!p.reports[0].complete);
    assert_eq!(p.reports[0].value_caps.len(), 1);
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.caps.InputReportByteLength = 4;
        s.values[0].clear();
    });
    let p = unsafe { snapshot_with(&api(), 1) };
    assert!(!p.reports[0].complete);
    assert!(p.reports[0].report_ids.is_empty());
    let mut v = value(0);
    v.IsRange = true;
    v.Anonymous.Range = HIDP_VALUE_CAPS_0_0 {
        UsageMin: 2,
        UsageMax: 1,
        ..Default::default()
    };
    assert!(!projection::valid(&projection::value(&v).common));
}

#[test]
fn sample_unknown_wide_and_invalid_values_preserve_raw_union_data() {
    let p = basic();
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.data = vec![HIDP_DATA {
            DataIndex: 7,
            Anonymous: HIDP_DATA_0 { RawValue: 99 },
            ..Default::default()
        }];
    });
    let read = unsafe { samples::samples_with(&api(), 1, &p, ReportKind::Input, &[1, 0, 0, 0]) };
    assert_eq!(read.data[0].raw_value, 99);
    assert_eq!(read.data[0].quality, "unmapped-data-index");
    assert!(read.data[0].values.is_empty());
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.data[0].DataIndex = 0;
        s.values[0][0].LogicalMax = 1;
    });
    let p = unsafe { snapshot_with(&api(), 1) };
    let read = unsafe { samples::samples_with(&api(), 1, &p, ReportKind::Input, &[1, 0, 0, 0]) };
    assert_eq!(read.data[0].values[0].quality, "invalid");
    STATE.with(|s| s.borrow_mut().values[0][0].BitSize = 33);
    let p = unsafe { snapshot_with(&api(), 1) };
    let read = unsafe { samples::samples_with(&api(), 1, &p, ReportKind::Input, &[1, 0, 0, 0]) };
    assert_eq!(read.data[0].values[0].quality, "unsupported-width");
    assert_eq!(read.data[0].values[0].native, None);
    let read = unsafe {
        samples::samples_with(
            &api(),
            1,
            &p,
            ReportKind::Input,
            &vec![0; MAX_SAMPLE_BYTES + 1],
        )
    };
    assert!(read.raw.is_empty());
    assert!(!read.failures.is_empty());
}

#[test]
fn usage_value_array_capacity_and_kind_absence_do_not_trigger_hidp_calls() {
    basic();
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.values[0][0].BitSize = u16::MAX;
        s.values[0][0].ReportCount = u16::MAX;
        s.max_data = 0;
        s.calls.clear();
    });
    let p = unsafe { snapshot_with(&api(), 1) };
    let read = unsafe { samples::samples_with(&api(), 1, &p, ReportKind::Input, &[1, 0, 0, 0]) };
    assert!(read.value_arrays.is_empty());
    assert!(read
        .failures
        .iter()
        .any(|f| f.stage == "value-array-capacity"));
    assert!(!STATE.with(|s| s.borrow().calls.contains(&"array".into())));
    let read = unsafe {
        samples::samples_with(
            &api(),
            1,
            &Snapshot::unavailable("missing"),
            ReportKind::Feature,
            &[0],
        )
    };
    assert!(read.value_arrays.is_empty());
    assert!(read.failures.iter().any(|f| f.stage == "capabilities"));
}
