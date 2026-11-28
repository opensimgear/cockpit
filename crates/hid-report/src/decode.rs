#[cfg(test)]
mod aliases_tests;
mod bits;
mod physical;
#[cfg(test)]
mod tests;
mod usages;

use crate::*;
use num_bigint::BigInt;
use std::collections::BTreeSet;

const MAX_LOOKUP_WORK: usize = 262_144;
fn diagnostic(reason: impl Into<String>) -> Diagnostic {
    Diagnostic {
        offset: 0,
        reason: reason.into(),
    }
}
fn capacity() -> Diagnostic {
    diagnostic("Report interpretation capacity exceeded")
}

/// Interpret one delivery without acquiring a device or changing device state.
/// Framing and all trailing bytes remain available in `raw` on every success.
/// Constants retain one opaque value per element; BufferedBytes stay grouped.
/// A selector without a declared usage mapping is unknown, not a guessed usage.
/// Physical scaling is exact when declared and is omitted for nonlinear fields.
pub fn decode_report(
    descriptor: &Descriptor,
    kind: ReportKind,
    framing: Framing,
    wire: &[u8],
) -> Result<DecodedReport, Diagnostic> {
    if wire.len() > MAX_REPORT_BYTES {
        return Err(capacity());
    }
    let mut report = DecodedReport {
        kind,
        framing,
        report_id: None,
        raw: wire.to_vec(),
        values: vec![],
        diagnostics: vec![],
    };
    let (id, payload) = match framing {
        Framing::Unnumbered => (None, wire),
        Framing::OutOfBand(id) if id != Some(0) => (id, wire),
        Framing::IdPrefix if wire.first().is_some_and(|id| *id != 0) => (Some(wire[0]), &wire[1..]),
        Framing::OsZeroPrefix if wire.first() == Some(&0) => (None, &wire[1..]),
        _ => {
            report
                .diagnostics
                .push(diagnostic("Missing or invalid report framing byte/ID"));
            return Ok(report);
        }
    };
    report.report_id = id;
    if !descriptor.diagnostics.is_empty() {
        report.diagnostics.push(diagnostic(
            "Descriptor has diagnostics; report values are unavailable",
        ));
        return Ok(report);
    }
    let Some(layout) = descriptor
        .layouts
        .iter()
        .find(|layout| layout.kind == kind && layout.report_id == id)
    else {
        report
            .diagnostics
            .push(diagnostic("Unknown report kind/ID layout"));
        return Ok(report);
    };
    let expected = layout.payload_bits.checked_add(7).ok_or_else(capacity)? / 8;
    if expected > MAX_REPORT_BYTES || layout.fields.len() > MAX_FIELDS {
        return Err(capacity());
    }
    if payload.len() < expected {
        report.diagnostics.push(diagnostic(
            "Truncated report payload; no partial values emitted",
        ));
        return Ok(report);
    }
    if payload.len() > expected {
        report.diagnostics.push(diagnostic(
            "Trailing delivery bytes preserved outside the declared payload",
        ));
    }
    let mut total_values = 0usize;
    let mut work = 0usize;
    for &index in &layout.fields {
        let Some(field) = descriptor.fields.get(index) else {
            report
                .diagnostics
                .push(diagnostic("Invalid descriptor field reference"));
            return Ok(report);
        };
        let g = &field.globals;
        let extent = g.size.checked_mul(g.count).ok_or_else(capacity)?;
        if g.size == 0
            || g.count == 0
            || field.kind != kind
            || g.report_id != id
            || field.offset_bits.checked_add(extent).ok_or_else(capacity)? > layout.payload_bits
            || matches!((g.logical_min,g.logical_max),(Some(low),Some(high)) if low > high)
        {
            report.diagnostics.push(diagnostic(
                "Invalid descriptor field extent or logical range",
            ));
            return Ok(report);
        }
        let attributes = field.attributes();
        let buffered = attributes.buffered_bytes;
        let constant = attributes.constant;
        if !buffered && !constant && g.size > MAX_SCALAR_BITS {
            return Err(capacity());
        }
        total_values = total_values
            .checked_add(if buffered { 1 } else { g.count })
            .ok_or_else(capacity)?;
        let branches = std::iter::once(&field.primary)
            .chain(field.alternatives.iter())
            .try_fold(0usize, |cost, locals| {
                cost.checked_add(usages::attribution_cost(locals).unwrap_or(1))
            })
            .ok_or_else(capacity)?;
        work = work
            .checked_add(if buffered || constant {
                1
            } else {
                g.count.checked_mul(branches).ok_or_else(capacity)?
            })
            .ok_or_else(capacity)?;
        work = work
            .checked_add(field.primary.usages.len())
            .ok_or_else(capacity)?;
        for locals in &field.alternatives {
            work = work.checked_add(locals.usages.len()).ok_or_else(capacity)?;
        }
        if total_values > MAX_FIELDS || work > MAX_LOOKUP_WORK {
            return Err(capacity());
        }
    }
    for &index in &layout.fields {
        let field = &descriptor.fields[index];
        match decode_field(index, field, payload) {
            Some(values) => report.values.extend(values),
            None => {
                report.values.clear();
                report.diagnostics.push(diagnostic(
                    "Invalid descriptor usage range or payload extent",
                ));
                break;
            }
        }
    }
    Ok(report)
}

fn decode_field(index: usize, field: &Field, payload: &[u8]) -> Option<Vec<Value>> {
    let g = &field.globals;
    let attributes = field.attributes();
    let constant = attributes.constant;
    let buffered = attributes.buffered_bytes;
    let variable = attributes.variable;
    let count = if buffered { 1 } else { g.count };
    let width = if buffered {
        g.size.checked_mul(g.count)?
    } else {
        g.size
    };
    let signed = g.logical_min.is_some_and(|min| min < 0);
    let indexes = if constant || buffered {
        None
    } else {
        Some(usages::Resolver::new(
            &field.primary,
            &field.alternatives,
            g.count,
        )?)
    };
    let mut values = Vec::with_capacity(count);
    for element in 0..count {
        let offset = field.offset_bits + element * width;
        if constant || buffered {
            let raw_bits = bits::extract_raw(payload, offset, width)?;
            values.push(Value {
                field: index,
                element,
                sample: Sample::Bytes(raw_bits.clone()),
                raw_bits,
                usages: vec![],
                semantics: Semantics::Opaque,
                physical: None,
            });
            continue;
        }
        let (number, raw_bits) = bits::extract(payload, offset, width, signed)?;
        let encoded = number.to_string();
        let mut attributed = if variable {
            attribute(indexes.as_ref()?, element, true)
        } else {
            vec![]
        };
        let semantics = if attributes.relative {
            Semantics::Relative
        } else if variable {
            Semantics::Absolute
        } else {
            Semantics::Event
        };
        let sample = if let (Some(low), Some(high)) = (g.logical_min, g.logical_max) {
            if number < BigInt::from(low) || number > BigInt::from(high) {
                if attributes.null_state {
                    Sample::Null(encoded)
                } else {
                    Sample::Invalid(encoded)
                }
            } else {
                if !variable {
                    if let Ok(selected) = usize::try_from(&number - low) {
                        attributed = attribute(indexes.as_ref()?, selected, false);
                    }
                }
                if variable {
                    Sample::Integer(encoded)
                } else if attributed.is_empty() {
                    Sample::Unknown {
                        reason: "Array selector has no declared usage mapping".into(),
                        encoded: Some(encoded),
                    }
                } else {
                    Sample::Array {
                        selector: encoded,
                        usages: attributed.clone(),
                    }
                }
            }
        } else {
            Sample::Unknown {
                reason: "Logical range unavailable".into(),
                encoded: Some(encoded),
            }
        };
        let physical = if !attributes.nonlinear && matches!(sample, Sample::Integer(_)) {
            physical::scale(&number, g)
        } else {
            None
        };
        values.push(Value {
            field: index,
            element,
            raw_bits,
            sample,
            usages: attributed,
            semantics,
            physical,
        });
    }
    Some(values)
}

fn attribute(indexes: &usages::Resolver, selected: usize, variable: bool) -> Vec<Usage> {
    let mut seen = BTreeSet::new();
    let mut attributed = vec![];
    indexes.visit(selected, variable, |usage| {
        if seen.insert((usage.page, usage.id)) {
            attributed.push(usage);
        }
    });
    attributed
}
