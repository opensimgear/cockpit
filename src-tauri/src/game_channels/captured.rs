//! Canonical projection preserves and unions original native contributors.
use super::*;
use crate::native_capture::{CapturedValue, CapturedValues, ProjectionBudget};
use std::time::Instant;

pub(crate) fn captured_values(game: &str, values: &CapturedValues, now: Instant) -> CapturedValues {
    let mut result: CapturedValues = values
        .iter()
        .filter(|(_, value)| value.is_current(now))
        .map(|(id, value)| (id.clone(), value.clone()))
        .collect();
    for rule in projections(game) {
        if let Some(value) = values
            .get(rule.native)
            .filter(|value| value.is_current(now))
            .and_then(|value| value.with_value(convert(rule, value.value())?))
        {
            result.insert(rule.id.into(), value);
        }
    }
    if let Some(sources) = magnitude_sources(game, "vehicle.g_force.magnitude") {
        if let Some(value) = magnitude(&sources, values, now) {
            result.insert("vehicle.g_force.magnitude".into(), value);
        }
    }
    bounded(result)
}

pub(crate) fn captured_project_rows(
    game: &str,
    rows: &[Value],
    values: &mut CapturedValues,
    now: Instant,
) {
    values.retain(|_, value| value.is_current(now));
    for row in rows {
        let (Some(id), Some(channel)) = (row["id"].as_str(), row["channel"].as_str()) else {
            continue;
        };
        if magnitude_sources(game, channel).is_some() {
            let keys = [format!("{id}/g0"), format!("{id}/g1"), format!("{id}/g2")];
            let inputs = keys.each_ref().map(String::as_str);
            let value = magnitude(&inputs, values, now);
            values.remove(id);
            if let Some(value) = value {
                values.insert(id.into(), value);
            }
        } else if let Some(rule) = projection(game, channel) {
            let value = values
                .get(id)
                .and_then(|value| value.with_value(convert(rule, value.value())?));
            values.remove(id);
            if let Some(value) = value {
                values.insert(id.into(), value);
            }
        }
    }
    let mut budget = ProjectionBudget::default();
    if values.values().any(|value| !budget.admit(value)) {
        values.clear();
    }
}

fn magnitude(keys: &[&str; 3], values: &CapturedValues, now: Instant) -> Option<CapturedValue> {
    let inputs = keys
        .iter()
        .map(|key| values.get(*key).filter(|v| v.is_current(now)))
        .collect::<Option<Vec<_>>>()?;
    let mut result = 0_f64;
    for input in &inputs {
        result = result.hypot(input.value().as_f64()?);
    }
    result
        .is_finite()
        .then(|| CapturedValue::combine(json!(result), &inputs, now))
        .flatten()
}
fn bounded(values: CapturedValues) -> CapturedValues {
    let mut budget = ProjectionBudget::default();
    if values.values().any(|value| !budget.admit(value)) {
        BTreeMap::new()
    } else {
        values
    }
}

#[cfg(test)]
#[path = "captured/tests.rs"]
mod tests;
