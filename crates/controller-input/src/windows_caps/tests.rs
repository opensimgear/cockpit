use super::*;
use std::cell::RefCell;
mod bounds;
mod version;
mod wire;

#[derive(Clone)]
struct State {
    caps: HIDP_CAPS,
    buttons: [Vec<HIDP_BUTTON_CAPS>; 3],
    values: [Vec<HIDP_VALUE_CAPS>; 3],
    links: Vec<HIDP_LINK_COLLECTION_NODE>,
    data: Vec<HIDP_DATA>,
    array: Vec<u8>,
    caps_status: i32,
    version: Option<u32>,
    button_status: i32,
    value_status: i32,
    link_status: i32,
    data_status: i32,
    array_status: i32,
    oversize: bool,
    calls: Vec<String>,
    max_data: u32,
}
impl Default for State {
    fn default() -> Self {
        Self {
            caps: HIDP_CAPS::default(),
            buttons: Default::default(),
            values: Default::default(),
            links: Vec::new(),
            data: Vec::new(),
            array: Vec::new(),
            caps_status: HIDP_STATUS_SUCCESS,
            version: Some(2),
            button_status: HIDP_STATUS_SUCCESS,
            value_status: HIDP_STATUS_SUCCESS,
            link_status: HIDP_STATUS_SUCCESS,
            data_status: HIDP_STATUS_SUCCESS,
            array_status: HIDP_STATUS_SUCCESS,
            oversize: false,
            calls: Vec::new(),
            max_data: 1,
        }
    }
}
thread_local! {static STATE:RefCell<State>=RefCell::new(State::default());}
unsafe extern "system" fn caps(_: isize, out: *mut HIDP_CAPS) -> i32 {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.calls.push("caps".into());
        *out = s.caps;
        s.caps_status
    })
}
unsafe extern "system" fn buttons(
    kind: i32,
    out: *mut HIDP_BUTTON_CAPS,
    count: *mut u16,
    _: isize,
) -> i32 {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.calls.push(format!("buttons:{kind}"));
        let rows = &s.buttons[kind as usize];
        std::ptr::copy_nonoverlapping(rows.as_ptr(), out, rows.len().min(*count as usize));
        *count = if s.oversize {
            u16::MAX
        } else {
            rows.len() as u16
        };
        s.button_status
    })
}
unsafe extern "system" fn values(
    kind: i32,
    out: *mut HIDP_VALUE_CAPS,
    count: *mut u16,
    _: isize,
) -> i32 {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.calls.push(format!("values:{kind}"));
        let rows = &s.values[kind as usize];
        std::ptr::copy_nonoverlapping(rows.as_ptr(), out, rows.len().min(*count as usize));
        *count = if s.oversize {
            u16::MAX
        } else {
            rows.len() as u16
        };
        s.value_status
    })
}
unsafe extern "system" fn links(
    out: *mut HIDP_LINK_COLLECTION_NODE,
    count: *mut u32,
    _: isize,
) -> i32 {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.calls.push("links".into());
        std::ptr::copy_nonoverlapping(s.links.as_ptr(), out, s.links.len().min(*count as usize));
        *count = if s.oversize {
            u32::MAX
        } else {
            s.links.len() as u32
        };
        s.link_status
    })
}
unsafe extern "system" fn data(
    _: i32,
    out: *mut HIDP_DATA,
    count: *mut u32,
    _: isize,
    _: *mut u8,
    _: u32,
) -> i32 {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.calls.push("data".into());
        assert_eq!(
            (*out).Anonymous.RawValue,
            0,
            "union storage must be cleared"
        );
        std::ptr::copy_nonoverlapping(s.data.as_ptr(), out, s.data.len().min(*count as usize));
        *count = if s.oversize {
            u32::MAX
        } else {
            s.data.len() as u32
        };
        s.data_status
    })
}
unsafe extern "system" fn max_data(_: i32, _: isize) -> u32 {
    STATE.with(|s| s.borrow().max_data)
}
unsafe extern "system" fn array(
    _: i32,
    _: u16,
    _: u16,
    _: u16,
    out: *mut u8,
    length: u16,
    _: isize,
    _: *const u8,
    _: u32,
) -> i32 {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.calls.push("array".into());
        assert_eq!(length as usize, s.array.len());
        std::ptr::copy_nonoverlapping(s.array.as_ptr(), out, s.array.len());
        s.array_status
    })
}
fn api() -> Api {
    Api {
        version: || {
            STATE.with(|s| ApiVersion {
                number: s.borrow().version,
                source: "injected-test-version".into(),
                status: None,
            })
        },
        caps,
        buttons,
        values,
        links,
        data,
        max_data,
        value_array: array,
    }
}
fn setup() {
    STATE.with(|s| *s.borrow_mut() = State::default());
}
fn value(id: u8) -> HIDP_VALUE_CAPS {
    let mut row = HIDP_VALUE_CAPS {
        ReportID: id,
        UsagePage: 1,
        BitField: 2,
        BitSize: 8,
        ReportCount: 1,
        IsAbsolute: true,
        LogicalMax: 255,
        ..Default::default()
    };
    row.Anonymous.NotRange = HIDP_VALUE_CAPS_0_1 {
        Usage: 0x30,
        DataIndex: 0,
        ..Default::default()
    };
    row
}
fn basic() -> Snapshot {
    setup();
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.caps.InputReportByteLength = 4;
        s.caps.NumberInputValueCaps = 1;
        s.values[0] = vec![value(1)];
    });
    unsafe { snapshot_with(&api(), 1) }
}

#[test]
fn all_report_kinds_aliases_and_complete_numeric_metadata_are_retained() {
    setup();
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.caps.UsagePage = 1;
        s.caps.Usage = 4;
        s.caps.InputReportByteLength = 8;
        s.caps.OutputReportByteLength = 4;
        s.caps.FeatureReportByteLength = 16;
        s.caps.NumberInputButtonCaps = 1;
        s.caps.NumberInputValueCaps = 1;
        s.caps.NumberOutputValueCaps = 1;
        s.caps.NumberFeatureValueCaps = 1;
        let mut button = HIDP_BUTTON_CAPS {
            UsagePage: 9,
            ReportID: 1,
            IsAlias: true,
            IsRange: true,
            IsStringRange: true,
            IsDesignatorRange: true,
            BitField: 0x102,
            ReportCount: 3,
            ..Default::default()
        };
        button.Anonymous.Range = HIDP_BUTTON_CAPS_0_0 {
            UsageMin: 1,
            UsageMax: 3,
            DataIndexMin: 0,
            DataIndexMax: 2,
            StringMin: 4,
            StringMax: 5,
            DesignatorMin: 6,
            DesignatorMax: 7,
        };
        s.buttons[0] = vec![button];
        let mut v = value(1);
        v.IsAlias = true;
        v.IsAbsolute = false;
        v.HasNull = true;
        v.Units = 0x11;
        v.UnitsExp = 0xffff_fffe;
        v.LogicalMax = -1;
        v.PhysicalMax = -1;
        s.values[0] = vec![v];
        s.values[1] = vec![value(2)];
        s.values[2] = vec![value(3)];
    });
    let projection = unsafe { snapshot_with(&api(), 1) };
    assert!(
        projection.available && projection.complete,
        "{:?}",
        projection.failures
    );
    assert_eq!(projection.reports.len(), 3);
    assert_eq!(projection.reports[0].report_ids, vec![1]);
    assert_eq!(projection.reports[1].report_ids, vec![2]);
    assert_eq!(projection.reports[2].report_ids, vec![3]);
    let v = &projection.reports[0].value_caps[0];
    assert_eq!(v.logical_max, 4294967295);
    assert_eq!(v.physical_max, 4294967295);
    assert_eq!(v.unit_exponent, -2);
    assert_eq!(v.units_exp_raw, 0xffff_fffe);
    assert!(!v.common.is_absolute);
    assert!(v.common.is_alias && v.has_null);
    let b = &projection.reports[0].button_caps[0];
    assert!(b.common.is_alias);
    assert_eq!(b.common.usages.maximum, 3);
    assert_eq!(b.common.strings.minimum, 4);
    assert_eq!(b.common.designators.maximum, 7);
    let encoded = serde_json::to_value(projection).unwrap();
    assert_eq!(encoded["provenance"], "windows-hidp-preparsed-capabilities");
    assert!(encoded.get("rawDescriptor").is_none());
}

#[test]
fn string_and_designator_range_flags_are_independent_of_usage_range() {
    let mut v = value(0);
    v.IsStringRange = true;
    v.IsDesignatorRange = true;
    v.Anonymous.Range = HIDP_VALUE_CAPS_0_0 {
        UsageMin: 0x30,
        UsageMax: 99,
        StringMin: 1,
        StringMax: 4,
        DesignatorMin: 5,
        DesignatorMax: 8,
        DataIndexMin: 2,
        DataIndexMax: 99,
    };
    let v = projection::value(&v);
    assert!(!v.common.usages.is_range);
    assert_eq!(v.common.usages.maximum, 0x30);
    assert_eq!(v.common.data_indices.maximum, 2);
    assert_eq!(v.common.strings.maximum, 4);
    assert_eq!(v.common.designators.maximum, 8);
}

#[test]
fn hierarchy_and_alias_flags_are_preserved_and_cycles_are_explicit() {
    setup();
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.caps.NumberLinkCollectionNodes = 2;
        s.links = vec![
            HIDP_LINK_COLLECTION_NODE {
                FirstChild: 1,
                NumberOfChildren: 1,
                ..Default::default()
            },
            HIDP_LINK_COLLECTION_NODE {
                Parent: 0,
                LinkUsagePage: 1,
                LinkUsage: 0x30,
                _bitfield: 0x102,
                ..Default::default()
            },
        ];
    });
    let p = unsafe { snapshot_with(&api(), 1) };
    assert!(p.complete);
    assert!(p.links[1].is_alias);
    assert_eq!(p.links[1].collection_type, 2);
    assert_eq!(p.links[0].first_child, 1);
    STATE.with(|s| s.borrow_mut().links[1].Parent = 1);
    let p = unsafe { snapshot_with(&api(), 1) };
    assert!(!p.complete);
    assert_eq!(p.links.len(), 2);
    assert!(p.failures.iter().any(|f| f.stage == "link-cycle"));
}

#[test]
fn status_length_and_allocation_failures_never_read_beyond_returned_arrays() {
    let p = basic();
    assert!(p.complete);
    STATE.with(|s| s.borrow_mut().oversize = true);
    let p = unsafe { snapshot_with(&api(), 1) };
    assert!(p.available && !p.complete);
    assert!(p.reports[0].value_caps.is_empty());
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.oversize = false;
        s.value_status = HIDP_STATUS_INVALID_PREPARSED_DATA;
    });
    let p = unsafe { snapshot_with(&api(), 1) };
    assert_eq!(
        p.failures[0].status,
        Some(HIDP_STATUS_INVALID_PREPARSED_DATA)
    );
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.caps.NumberInputValueCaps = u16::MAX;
        s.calls.clear();
    });
    let p = unsafe { snapshot_with(&api(), 1) };
    assert!(!p.complete);
    assert!(!STATE.with(|s| s.borrow().calls.contains(&"values:0".into())));
    STATE.with(|s| s.borrow_mut().caps_status = HIDP_STATUS_INVALID_PREPARSED_DATA);
    assert!(!unsafe { snapshot_with(&api(), 1) }.available);
    STATE.with(|s| s.borrow_mut().calls.clear());
    assert!(!unsafe { snapshot_with(&api(), 0) }.available);
    assert!(STATE.with(|s| s.borrow().calls.is_empty()));
}

#[test]
fn actual_supplied_bytes_project_relative_null_and_usage_value_arrays() {
    basic();
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.values[0][0].LogicalMin = -128;
        s.values[0][0].LogicalMax = 127;
        s.values[0][0].IsAbsolute = false;
        s.data = vec![HIDP_DATA {
            DataIndex: 0,
            Anonymous: HIDP_DATA_0 { RawValue: 255 },
            ..Default::default()
        }];
    });
    let p = unsafe { snapshot_with(&api(), 1) };
    let read = unsafe { samples::samples_with(&api(), 1, &p, ReportKind::Input, &[1, 255, 0, 0]) };
    assert!(read.failures.is_empty());
    assert_eq!(read.raw, vec![1, 255, 0, 0]);
    assert_eq!(read.data[0].values[0].native, Some(-1));
    assert_eq!(read.data[0].values[0].semantics, "relative");
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.values[0][0].HasNull = true;
        s.values[0][0].LogicalMax = 0;
        s.data[0].Anonymous.RawValue = 1;
    });
    let p = unsafe { snapshot_with(&api(), 1) };
    assert_eq!(
        unsafe { samples::samples_with(&api(), 1, &p, ReportKind::Input, &[1, 1, 0, 0]) }.data[0]
            .values[0]
            .quality,
        "null"
    );
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.values[0][0].ReportCount = 3;
        s.array = vec![1, 7, 255];
        s.data.clear();
        s.max_data = 0;
    });
    let p = unsafe { snapshot_with(&api(), 1) };
    let read = unsafe { samples::samples_with(&api(), 1, &p, ReportKind::Input, &[1, 1, 7, 255]) };
    assert!(read.failures.is_empty());
    assert_eq!(read.value_arrays[0].bytes, vec![1, 7, 255]);
    assert_eq!(read.value_arrays[0].report_count, 3);
    assert!(
        read.data.is_empty(),
        "No fabricated data-index offsets for usage-value arrays"
    );
    assert!(p.complete);
}

#[test]
fn samples_refuse_invalid_framing_counts_extents_and_api_failures() {
    let p = basic();
    for wire in [&[][..], &[2, 0, 0, 0][..], &[1, 0][..]] {
        let read = unsafe { samples::samples_with(&api(), 1, &p, ReportKind::Input, wire) };
        assert!(read.data.is_empty() && !read.failures.is_empty());
        assert_eq!(read.raw, wire);
    }
    STATE.with(|s| s.borrow_mut().max_data = MAX_CAPS as u32 + 1);
    assert!(
        !unsafe { samples::samples_with(&api(), 1, &p, ReportKind::Input, &[1, 0, 0, 0]) }
            .failures
            .is_empty()
    );
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.max_data = 1;
        s.oversize = true;
    });
    assert!(
        unsafe { samples::samples_with(&api(), 1, &p, ReportKind::Input, &[1, 0, 0, 0]) }
            .data
            .is_empty()
    );
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.oversize = false;
        s.values[0][0].ReportCount = 3;
        s.array = vec![0; 3];
        s.array_status = HIDP_STATUS_NOT_VALUE_ARRAY;
    });
    let p = unsafe { snapshot_with(&api(), 1) };
    let read = unsafe { samples::samples_with(&api(), 1, &p, ReportKind::Input, &[1, 0, 0, 0]) };
    assert!(read.value_arrays.is_empty());
    assert!(read.failures.iter().any(|f| f.stage == "value-array"));
}
