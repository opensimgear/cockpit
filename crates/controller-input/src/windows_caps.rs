//! Read-only HidP projections over caller-owned Windows preparsed data.
//! These are OS parser facts, never original report-descriptor bytes/offsets.
use windows_sys::Win32::Devices::HumanInterfaceDevice::*;
mod types;
pub use types::*;

pub(crate) struct Api {
    pub version: fn() -> ApiVersion,
    pub caps: unsafe extern "system" fn(isize, *mut HIDP_CAPS) -> i32,
    pub buttons: unsafe extern "system" fn(i32, *mut HIDP_BUTTON_CAPS, *mut u16, isize) -> i32,
    pub values: unsafe extern "system" fn(i32, *mut HIDP_VALUE_CAPS, *mut u16, isize) -> i32,
    pub links: unsafe extern "system" fn(*mut HIDP_LINK_COLLECTION_NODE, *mut u32, isize) -> i32,
    pub data: unsafe extern "system" fn(i32, *mut HIDP_DATA, *mut u32, isize, *mut u8, u32) -> i32,
    pub max_data: unsafe extern "system" fn(i32, isize) -> u32,
    pub value_array:
        unsafe extern "system" fn(i32, u16, u16, u16, *mut u8, u16, isize, *const u8, u32) -> i32,
}
impl Api {
    pub(crate) fn native() -> Self {
        Self {
            version: version::native,
            caps: HidP_GetCaps,
            buttons: HidP_GetButtonCaps,
            values: HidP_GetValueCaps,
            links: HidP_GetLinkCollectionNodes,
            data: HidP_GetData,
            max_data: HidP_MaxDataListLength,
            value_array: HidP_GetUsageValueArray,
        }
    }
}

/// # Safety
/// `preparsed` must point to live Windows preparsed data for the entire call.
/// No device handles are opened and no feature/output reports are requested.
pub unsafe fn snapshot(preparsed: isize) -> Snapshot {
    snapshot_with(&Api::native(), preparsed)
}

/// # Safety
/// Same lifetime requirement as `snapshot`; `wire` is an already-acquired report,
/// including Windows' report-ID byte (zero for an unnumbered report).
pub unsafe fn samples(
    preparsed: isize,
    snapshot: &Snapshot,
    kind: ReportKind,
    wire: &[u8],
) -> Samples {
    samples::samples_with(&Api::native(), preparsed, snapshot, kind, wire)
}

mod projection;
mod samples;
mod version;

pub(crate) unsafe fn snapshot_with(api: &Api, pp: isize) -> Snapshot {
    let mut snapshot = Snapshot {
        provenance: "windows-hidp-preparsed-capabilities".into(),
        available: false,
        complete: true,
        api_version: ApiVersion::unavailable(),
        usage_page: None,
        usage: None,
        declared_link_count: 0,
        reports: Vec::new(),
        links: Vec::new(),
        failures: Vec::new(),
    };
    if pp == 0 {
        snapshot.fail("preparsed", None, None, "Preparsed data is unavailable");
        return snapshot;
    }
    snapshot.api_version = (api.version)();
    if snapshot.api_version.number.is_none() {
        snapshot.fail(
            "api-version",
            None,
            snapshot.api_version.status,
            "HidP API version is unavailable; button ReportCount is not exposed",
        );
    }
    let button_count_supported = snapshot
        .api_version
        .number
        .is_some_and(|version| version >= 2);
    let mut caps = HIDP_CAPS::default();
    let status = (api.caps)(pp, &mut caps);
    if status != HIDP_STATUS_SUCCESS {
        snapshot.fail("caps", None, Some(status), "HidP_GetCaps failed");
        return snapshot;
    }
    snapshot.available = true;
    snapshot.usage_page = Some(caps.UsagePage);
    snapshot.usage = Some(caps.Usage);
    snapshot.declared_link_count = caps.NumberLinkCollectionNodes;
    let descriptions = [
        (
            ReportKind::Input,
            caps.InputReportByteLength,
            caps.NumberInputButtonCaps,
            caps.NumberInputValueCaps,
            caps.NumberInputDataIndices,
        ),
        (
            ReportKind::Output,
            caps.OutputReportByteLength,
            caps.NumberOutputButtonCaps,
            caps.NumberOutputValueCaps,
            caps.NumberOutputDataIndices,
        ),
        (
            ReportKind::Feature,
            caps.FeatureReportByteLength,
            caps.NumberFeatureButtonCaps,
            caps.NumberFeatureValueCaps,
            caps.NumberFeatureDataIndices,
        ),
    ];
    let mut retained = 0usize;
    for (kind, byte_length, buttons, values, data_indices) in descriptions {
        let mut report = Report {
            kind,
            byte_length,
            data_indices,
            declared_button_caps: buttons,
            declared_value_caps: values,
            report_ids: Vec::new(),
            button_caps: Vec::new(),
            value_caps: Vec::new(),
            complete: true,
        };
        for is_button in [true, false] {
            let expected = if is_button { buttons } else { values };
            let stage = if is_button {
                "button-caps"
            } else {
                "value-caps"
            };
            if retained
                .checked_add(expected as usize)
                .is_none_or(|n| n > MAX_CAPS)
            {
                report.complete = false;
                snapshot.fail(
                    stage,
                    Some(kind),
                    None,
                    "Aggregate capability count exceeds inspection bound",
                );
                continue;
            }
            retained += expected as usize;
            if expected == 0 {
                continue;
            }
            let mut returned = expected;
            if is_button {
                let mut rows = vec![HIDP_BUTTON_CAPS::default(); expected as usize];
                let status = (api.buttons)(kind.native(), rows.as_mut_ptr(), &mut returned, pp);
                if status == HIDP_STATUS_SUCCESS && returned <= expected {
                    report.button_caps = rows[..returned as usize]
                        .iter()
                        .map(|row| projection::button(row, button_count_supported))
                        .collect();
                }
                if status != HIDP_STATUS_SUCCESS || returned != expected {
                    report.complete = false;
                    snapshot.fail(
                        stage,
                        Some(kind),
                        Some(status),
                        "Button capability status/count differs from advertised allocation",
                    );
                }
            } else {
                let mut rows = vec![HIDP_VALUE_CAPS::default(); expected as usize];
                let status = (api.values)(kind.native(), rows.as_mut_ptr(), &mut returned, pp);
                if status == HIDP_STATUS_SUCCESS && returned <= expected {
                    report.value_caps = rows[..returned as usize]
                        .iter()
                        .map(projection::value)
                        .collect();
                }
                if status != HIDP_STATUS_SUCCESS || returned != expected {
                    report.complete = false;
                    snapshot.fail(
                        stage,
                        Some(kind),
                        Some(status),
                        "Value capability status/count differs from advertised allocation",
                    );
                }
            }
        }
        let mut ids = std::collections::BTreeSet::new();
        for common in report
            .button_caps
            .iter()
            .map(|row| &row.common)
            .chain(report.value_caps.iter().map(|row| &row.common))
        {
            ids.insert(common.report_id);
            if !projection::valid(common) {
                report.complete = false;
                snapshot.fail(
                    "capability-ranges",
                    Some(kind),
                    None,
                    "Invalid capability range or collection association",
                );
            }
        }
        report.report_ids = ids.into_iter().collect();
        if byte_length == 0 && (!report.button_caps.is_empty() || !report.value_caps.is_empty()) {
            report.complete = false;
            snapshot.fail(
                "report-length",
                Some(kind),
                None,
                "Capabilities exist for an advertised zero-length report",
            );
        }
        snapshot.reports.push(report);
    }
    let expected = caps.NumberLinkCollectionNodes as usize;
    if expected > MAX_LINKS {
        snapshot.fail(
            "links",
            None,
            None,
            "Collection-node count exceeds inspection bound",
        );
    } else if expected > 0 {
        let mut rows = vec![HIDP_LINK_COLLECTION_NODE::default(); expected];
        let mut count = expected as u32;
        let status = (api.links)(rows.as_mut_ptr(), &mut count, pp);
        if status == HIDP_STATUS_SUCCESS && count as usize <= expected {
            snapshot.links = rows[..count as usize]
                .iter()
                .enumerate()
                .map(|(index, row)| projection::link(index, row))
                .collect();
            projection::validate_links(&mut snapshot);
        }
        if status != HIDP_STATUS_SUCCESS || count as usize != expected {
            snapshot.fail(
                "links",
                None,
                Some(status),
                "Collection-node status/count differs from advertised allocation",
            );
        }
    }
    snapshot.complete &= snapshot.reports.iter().all(|r| r.complete);
    snapshot
}

#[cfg(test)]
mod tests;
