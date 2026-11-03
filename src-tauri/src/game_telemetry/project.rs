//! Projection preserves original page captures and actual decoded dependencies.
use super::{schema, CaptureSet, ChannelStatus, Page, Snapshot};
use crate::native_capture::{
    CapturedValue, CapturedValues, Contribution, InputId, ProjectionBudget, Stamp, Validity,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::{Duration, Instant},
};

impl Snapshot {
    pub fn values(&self) -> CapturedValues {
        self.values_at(Instant::now())
    }

    pub fn values_at(&self, now: Instant) -> CapturedValues {
        if !self.fresh
            || self
                .capture
                .as_ref()
                .is_some_and(|capture| !capture.current_at(now, self.freshness_ms))
        {
            return BTreeMap::new();
        }
        let native_ids: BTreeSet<_> = schema::ids(self.game).collect();
        let mut contributors = BTreeMap::new();
        let mut values = BTreeMap::new();
        let mut budget = ProjectionBudget::default();
        for (id, channel) in &self.channels {
            if channel.status != ChannelStatus::Ready {
                continue;
            }
            let Some(value) = &channel.value else {
                continue;
            };
            let projected = if let Some(capture) = &self.capture {
                let dependencies = match id.as_str() {
                    "rpm" => vec!["physics.rpms"],
                    "gear" => vec!["physics.gear"],
                    "speed_kmh" => vec!["physics.speedKmh"],
                    "max_rpm" => self.maximum_input.into_iter().collect(),
                    "rev_ratio" => self
                        .maximum_input
                        .map(|maximum| vec!["physics.rpms", maximum])
                        .unwrap_or_default(),
                    native if native_ids.contains(native) => vec![native],
                    _ => continue,
                };
                let evidence = dependencies
                    .into_iter()
                    .map(|input| {
                        if let Some(existing) = contributors.get(input) {
                            return Some(Arc::clone(existing));
                        }
                        let contribution =
                            Arc::new(contribution(capture, input, self.freshness_ms)?);
                        contributors.insert(input.to_owned(), Arc::clone(&contribution));
                        Some(contribution)
                    })
                    .collect::<Option<Vec<_>>>();
                evidence.and_then(|evidence| CapturedValue::native(value.clone(), evidence, now))
            } else {
                Some(CapturedValue::fixture(value.clone()))
            };
            if let Some(projected) = projected {
                if !budget.admit(&projected) {
                    return BTreeMap::new();
                }
                values.insert(id.clone(), projected);
            }
        }
        let values = crate::game_channels::captured_values(self.game.id(), &values, now);
        // Canonical copies and derived unions also count toward the complete projection.
        let mut budget = ProjectionBudget::default();
        if values.values().all(|value| budget.admit(value)) {
            values
        } else {
            BTreeMap::new()
        }
    }
}

fn contribution(capture: &CaptureSet, input: &str, freshness_ms: u64) -> Option<Contribution> {
    let page = match input.split_once('.')?.0 {
        "physics" => Page::Physics,
        "graphics" => Page::Graphics,
        "static" => Page::Statics,
        _ => return None,
    };
    let page = capture.page(page);
    let valid_until = page
        .captured_at
        .checked_add(Duration::from_millis(freshness_ms))?;
    Some(Contribution {
        input: InputId::Channel(Arc::from(input)),
        capture: Stamp {
            producer_id: Arc::from(capture.producer_id.as_str()),
            generation: capture.generation,
            sequence: page.sequence,
            byte_offset: None,
            captured_at: page.captured_at,
        },
        validity: Validity::Sampled { valid_until },
    })
}

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;
