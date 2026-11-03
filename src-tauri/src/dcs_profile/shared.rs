//! Catalogue-bound projection of the canonical source. No parser or I/O work.
use super::*;
use crate::dcs_bios::decoded::{Snapshot, Spec};
use crate::native_capture::{CapturedValues, ProjectionBudget};

impl ExportSession {
    pub(crate) fn shared_baseline_ready(&self) -> bool {
        self.shared.as_ref().is_some_and(|snapshot| snapshot.status == "waiting"
            && snapshot.aircraft.is_some() && snapshot.counter.is_some())
    }
    pub(crate) fn decode_spec(&self) -> Result<Spec, String> {
        Spec::from_catalogue(&self.catalogue, self.freshness_ms)
    }
    pub(crate) fn accept_shared(&mut self, snapshot: Snapshot) -> Result<(), String> {
        let replaced = self.shared.as_ref().is_none_or(|previous|
            previous.producer_id != snapshot.producer_id || previous.epoch != snapshot.epoch);
        if replaced {
            self.epoch = self.epoch.checked_add(1)
                .ok_or("DCS projection epoch exhausted; acquire a new projection.")?;
        }
        self.shared = Some(snapshot);
        Ok(())
    }
    pub(super) fn project_shared(&self, snapshot: &Snapshot, now: Instant) -> ExportSnapshot {
        let mission_active = snapshot.aircraft.as_ref()
            .is_some_and(|aircraft| self.catalogue.aircraft_names.contains(aircraft));
        let mut fresh = mission_active && snapshot.fresh_at(now);
        let allowed = self.export_aircraft_names.as_ref().is_none_or(|names|
            snapshot.aircraft.as_ref().is_some_and(|aircraft| names.contains(aircraft)));
        let values: CapturedValues = if fresh && allowed {
            if let Some(mut projection) = super::captured_values::Projection::new(snapshot, now) {
                let mut values = BTreeMap::new();
                let mut budget = ProjectionBudget::default();
                for (id, source) in self.outputs.iter().map(|output|
                    (output.binding.id.as_str(), &output.source))
                    .chain(self.extra_sources.iter().map(|(id, source)| (id.as_str(), source)))
                {
                    if let Some(value) = source.read_captured(&mut projection, now) {
                        if !budget.admit(&value) {
                            values.clear();
                            fresh = false;
                            break;
                        }
                        values.insert(id.to_owned(), value);
                    }
                }
                values
            } else {
                fresh = false;
                BTreeMap::new()
            }
        } else { BTreeMap::new() };
        ExportSnapshot { epoch: self.epoch, aircraft: snapshot.aircraft.clone(), mission_active,
            fresh, age_ms: snapshot.age_ms(now), counter: snapshot.counter, values,
            malformed_frames: snapshot.malformed_frames }
    }
}
