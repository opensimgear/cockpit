//! Native ownship fields retain the packet's original host acquisition.
use super::{Snapshot, FRESHNESS_MS};
use crate::native_capture::{
    CapturedValue, CapturedValues, Contribution, InputId, ProjectionBudget, Stamp, Validity,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
impl Snapshot {
    pub(crate) fn captured_values(&self, now: Instant) -> CapturedValues {
        let Some(at) = self.captured_at else {
            return CapturedValues::new();
        };
        let Some(until) = at.checked_add(Duration::from_millis(FRESHNESS_MS + 1)) else {
            return CapturedValues::new();
        };
        if !self.fresh || now >= until {
            return CapturedValues::new();
        }
        let producer_id: Arc<str> = self.producer_id.as_str().into();
        let mut values = CapturedValues::new();
        let mut budget = ProjectionBudget::default();
        for (id, value) in &self.values {
            let contributor = Arc::new(Contribution {
                input: InputId::Channel(id.as_str().into()),
                capture: Stamp {
                    producer_id: producer_id.clone(),
                    generation: self.epoch,
                    sequence: self.capture_sequence,
                    byte_offset: None,
                    captured_at: at,
                },
                validity: Validity::Sampled { valid_until: until },
            });
            if let Some(value) = CapturedValue::native(value.clone(), vec![contributor], now) {
                if !budget.admit(&value) {
                    return CapturedValues::new();
                }
                values.insert(id.clone(), value);
            }
        }
        crate::game_channels::captured_values("dcs", &values, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn packet_fields_and_canonical_vectors_share_native_time_and_expire_without_reads() {
        let at = Instant::now();
        let snapshot = Snapshot {
            producer_id: "ownship".into(),
            epoch: 2,
            session: Some("mission".into()),
            aircraft: Some("A-10C".into()),
            plane_id: Some(1),
            model_time: Some(2.),
            sequence: Some(7),
            capture_sequence: 900,
            fresh: true,
            status: "live".into(),
            age_ms: Some(0),
            captured_at: Some(at),
            values: std::collections::BTreeMap::from([
                ("flight.acceleration.x".into(), json!(0)),
                ("flight.acceleration.y".into(), json!(3)),
                ("flight.acceleration.z".into(), json!(4)),
                ("flight.ownship.inAir".into(), json!(false)),
            ]),
            malformed_frames: 0,
            error: None,
        };
        let values = snapshot.captured_values(at + Duration::from_millis(200));
        assert_eq!(values["vehicle.g_force.magnitude"].value(), &json!(5.));
        assert_eq!(values["flight.ownship.inAir"].value(), &json!(false));
        for c in values["vehicle.g_force.magnitude"].contributors().unwrap() {
            assert_eq!(c.capture.sequence, 900);
            assert_eq!(c.capture.captured_at, at);
        }
        assert!(snapshot
            .captured_values(at + Duration::from_millis(FRESHNESS_MS + 1))
            .is_empty());
        let mut missing = snapshot.clone();
        missing.captured_at = None;
        assert!(missing.captured_values(at).is_empty());
        missing = snapshot;
        missing.capture_sequence = 0;
        assert!(missing.captured_values(at).is_empty());
    }
}
