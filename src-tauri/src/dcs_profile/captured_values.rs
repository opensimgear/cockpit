//! DCS-BIOS values paired with the exact native byte captures they use.
use super::OutputSource;
use crate::{
    dcs_bios::decoded::Snapshot,
    native_capture::{CapturedValue, Contribution, InputId, Lease, Stamp, Validity},
};
use std::{collections::BTreeMap, sync::Arc, time::Instant};

/// A byte-witness cache and lease bound to exactly one immutable snapshot.
pub(crate) struct Projection<'a> {
    snapshot: &'a Snapshot,
    lease: Arc<Lease>,
    cache: BTreeMap<u16, Arc<Contribution>>,
}
impl<'a> Projection<'a> {
    pub(crate) fn new(snapshot: &'a Snapshot, now: Instant) -> Option<Self> {
        let observation = snapshot.counter_observation.as_ref()?;
        let valid_until = snapshot.deadline()?;
        if valid_until <= now || snapshot.epoch == 0 {
            return None;
        }
        Some(Self {
            snapshot,
            lease: Arc::new(Lease {
                domain_id: Arc::from(snapshot.producer_id.as_str()),
                generation: snapshot.epoch,
                observation: Stamp {
                    producer_id: Arc::from(observation.acquisition_id.as_str()),
                    generation: 1,
                    sequence: observation.position.report_sequence,
                    byte_offset: Some(observation.position.byte_offset),
                    captured_at: observation.captured_at,
                },
                valid_until,
            }),
            cache: BTreeMap::new(),
        })
    }
}

impl OutputSource {
    pub(crate) fn read_captured(
        &self,
        projection: &mut Projection<'_>,
        now: Instant,
    ) -> Option<CapturedValue> {
        let snapshot = projection.snapshot;
        let value = self.read(&snapshot.memory)?;
        let (address, length) = match self {
            Self::Integer(output) => (output.address, 2),
            Self::String { address, length } => (*address, *length),
        };
        let mut contributors = Vec::with_capacity(length);
        for offset in 0..length {
            let address = u16::try_from((address as usize).checked_add(offset)?).ok()?;
            let contribution = if let Some(contribution) = projection.cache.get(&address) {
                contribution.clone()
            } else {
                let capture = snapshot.memory.capture(address)?;
                let contribution = Arc::new(Contribution {
                    input: InputId::Byte(address),
                    capture: Stamp {
                        producer_id: Arc::from(snapshot.memory.acquisition_id()),
                        generation: 1,
                        sequence: capture.position.report_sequence,
                        byte_offset: Some(capture.position.byte_offset),
                        captured_at: capture.captured_at,
                    },
                    validity: Validity::Latched(projection.lease.clone()),
                });
                projection.cache.insert(address, contribution.clone());
                contribution
            };
            contributors.push(contribution);
        }
        CapturedValue::native(value, contributors, now)
    }
}

#[cfg(test)]
#[path = "captured_values_tests.rs"]
mod tests;
