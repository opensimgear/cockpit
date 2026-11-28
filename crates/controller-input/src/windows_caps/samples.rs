use super::*;
const MAX_LOOKUP_WORK: usize = 262_144;
fn fail(samples: &mut Samples, stage: &str, status: Option<i32>, message: &str) {
    if samples.failures.len() < MAX_FAILURES {
        samples.failures.push(Failure {
            stage: stage.into(),
            kind: Some(samples.kind),
            status,
            message: message.into(),
        });
    }
}
fn scalar(index: usize, c: &Value, raw: u32) -> ScalarValue {
    let semantics = if c.common.bit_field & 2 == 0 {
        "array-selector"
    } else if c.common.is_absolute {
        "absolute"
    } else {
        "relative"
    };
    let (native, quality) = if c.bit_size == 0 || c.bit_size > 32 {
        (None, "unsupported-width")
    } else {
        let shift = 32 - c.bit_size;
        let value = if c.logical_min < 0 {
            i64::from(((raw << shift) as i32) >> shift)
        } else {
            i64::from(raw & (u32::MAX >> shift))
        };
        let quality = if value < c.logical_min || value > c.logical_max {
            if c.has_null {
                "null"
            } else {
                "invalid"
            }
        } else {
            "valid"
        };
        (Some(value), quality)
    };
    ScalarValue {
        capability_index: index,
        native,
        quality: quality.into(),
        semantics: semantics.into(),
    }
}
pub(super) unsafe fn samples_with(
    api: &Api,
    pp: isize,
    snapshot: &Snapshot,
    kind: ReportKind,
    wire: &[u8],
) -> Samples {
    let mut samples = Samples {
        provenance: "windows-hidp-report-projection".into(),
        kind,
        report_id: wire.first().copied(),
        raw: Vec::new(),
        data: Vec::new(),
        value_arrays: Vec::new(),
        failures: Vec::new(),
    };
    if wire.len() > MAX_SAMPLE_BYTES {
        fail(
            &mut samples,
            "report-length",
            None,
            "Report exceeds sample byte bound",
        );
        return samples;
    }
    samples.raw = wire.to_vec();
    let Some(report) = snapshot
        .report(kind)
        .filter(|r| snapshot.available && r.complete)
    else {
        fail(
            &mut samples,
            "capabilities",
            None,
            "Complete report-kind capabilities are unavailable",
        );
        return samples;
    };
    if pp == 0
        || wire.len() != report.byte_length as usize
        || wire.is_empty()
        || !report.report_ids.contains(&wire[0])
    {
        fail(
            &mut samples,
            "report-framing",
            None,
            "Report ID/length differs from this preparsed collection's advertised report kind",
        );
        return samples;
    }
    let count = (api.max_data)(kind.native(), pp) as usize;
    let cap_count = report.button_caps.len() + report.value_caps.len();
    if count > MAX_CAPS
        || count
            .checked_mul(cap_count)
            .is_none_or(|work| work > MAX_LOOKUP_WORK)
    {
        fail(
            &mut samples,
            "data-capacity",
            None,
            "Data-index matching exceeds bounded work budget",
        );
        return samples;
    }
    if count > 0 {
        // Zero-initialized storage matters: HidP may fill only the bool On union
        // member for a button. Never inherit bytes from an earlier scalar value.
        let mut data = vec![HIDP_DATA::default(); count];
        let mut returned = count as u32;
        let mut packet = wire.to_vec();
        let status = (api.data)(
            kind.native(),
            data.as_mut_ptr(),
            &mut returned,
            pp,
            packet.as_mut_ptr(),
            packet.len() as u32,
        );
        if status != HIDP_STATUS_SUCCESS || returned as usize > data.len() {
            fail(
                &mut samples,
                "data",
                Some(status),
                "HidP_GetData failed or returned a count larger than its allocation",
            );
        } else {
            for entry in &data[..returned as usize] {
                let matching = |c: &Capability| {
                    c.report_id == wire[0]
                        && entry.DataIndex >= c.data_indices.minimum
                        && entry.DataIndex <= c.data_indices.maximum
                };
                let buttons: Vec<_> = report
                    .button_caps
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| matching(&c.common))
                    .map(|(i, _)| i)
                    .collect();
                let values: Vec<_> = report
                    .value_caps
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| matching(&c.common))
                    .map(|(i, _)| i)
                    .collect();
                let raw = entry.Anonymous.RawValue;
                let projected = values
                    .iter()
                    .map(|index| scalar(*index, &report.value_caps[*index], raw))
                    .collect();
                samples.data.push(DataValue {
                    data_index: entry.DataIndex,
                    raw_value: raw,
                    quality: if buttons.is_empty() && values.is_empty() {
                        "unmapped-data-index"
                    } else {
                        "os-projected"
                    }
                    .into(),
                    matching_button_caps: buttons,
                    matching_value_caps: values,
                    values: projected,
                });
            }
        }
    }
    let mut retained = 0usize;
    let mut arrays = 0usize;
    for (index, c) in report.value_caps.iter().enumerate().filter(|(_, c)| {
        c.common.report_id == wire[0] && !c.common.usages.is_range && c.report_count > 1
    }) {
        arrays += 1;
        let length = usize::from(c.bit_size)
            .checked_mul(c.report_count as usize)
            .and_then(|bits| bits.checked_add(7))
            .map(|bits| bits / 8);
        let Some(length) = length.filter(|n| *n > 0 && *n <= u16::MAX as usize) else {
            fail(
                &mut samples,
                "value-array-capacity",
                None,
                "Usage-value-array extent is unavailable or too large for HidP",
            );
            continue;
        };
        if arrays > 128
            || retained
                .checked_add(length)
                .is_none_or(|bytes| bytes > MAX_SAMPLE_BYTES)
        {
            fail(
                &mut samples,
                "value-array-capacity",
                None,
                "Aggregate usage-value-array allocation exceeds inspection bound",
            );
            break;
        }
        retained += length;
        let mut bytes = vec![0u8; length];
        let status = (api.value_array)(
            kind.native(),
            c.common.usage_page,
            c.common.link_collection,
            c.common.usages.minimum,
            bytes.as_mut_ptr(),
            length as u16,
            pp,
            wire.as_ptr(),
            wire.len() as u32,
        );
        if status != HIDP_STATUS_SUCCESS {
            fail(
                &mut samples,
                "value-array",
                Some(status),
                "HidP_GetUsageValueArray failed",
            );
            continue;
        }
        // This packed buffer is an OS projection, not a slice at an invented
        // offset in raw. Array selectors/usage attribution remain explicit caps.
        samples.value_arrays.push(ValueArray {
            capability_index: index,
            usage_page: c.common.usage_page,
            usage: c.common.usages.minimum,
            link_collection: c.common.link_collection,
            bit_size: c.bit_size,
            report_count: c.report_count,
            bytes,
        });
    }
    samples
}
