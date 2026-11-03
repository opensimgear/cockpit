use super::Snapshot;
use serde_json::Value;
use std::mem::size_of;

// Per-entry space covers the B-tree slot, links, allocator slack, and clone
// metadata. The per-map allowance covers its root/leaf allocation even when it
// contains only one key; per-entry space is intentionally conservative for
// larger maps with partially occupied nodes.
const MAP_NODE_ALLOWANCE: usize = 1024;
const ENTRY_NODE_ALLOWANCE: usize = 256;

fn add(total: &mut usize, bytes: usize) -> bool {
    match total.checked_add(bytes) {
        Some(value) => {
            *total = value;
            true
        }
        None => false,
    }
}

fn string_capacity(total: &mut usize, value: Option<&String>) -> bool {
    value.is_none_or(|value| add(total, value.capacity()))
}

impl osg_streams::RetainedBytes for Snapshot {
    fn retained_bytes(&self) -> usize {
        let mut total = size_of::<Self>();
        if !string_capacity(&mut total, Some(&self.producer_id))
            || !string_capacity(&mut total, self.session.as_ref())
            || !string_capacity(&mut total, self.aircraft.as_ref())
            || !string_capacity(&mut total, Some(&self.status))
            || !string_capacity(&mut total, self.error.as_ref())
        {
            return usize::MAX;
        }
        if !self.values.is_empty()
            && (!add(&mut total, MAP_NODE_ALLOWANCE)
                || !add(
                    &mut total,
                    self.values.len().saturating_mul(ENTRY_NODE_ALLOWANCE),
                ))
        {
            return usize::MAX;
        }
        for (key, value) in &self.values {
            if !add(
                &mut total,
                size_of::<(String, Value)>().saturating_add(key.capacity()),
            ) {
                return usize::MAX;
            }
            let capacity = match value {
                Value::Null | Value::Bool(_) | Value::Number(_) => 0,
                Value::String(value) => value.capacity(),
                Value::Array(_) | Value::Object(_) => return usize::MAX,
            };
            if !add(&mut total, capacity) {
                return usize::MAX;
            }
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dcs_physics::{catalogue, source::LIMITS};
    use osg_streams::{Delivery, NativeStream, RetainedBytes};
    use serde_json::json;
    use std::{collections::BTreeMap, time::Instant};

    fn maximum_text_snapshot() -> Snapshot {
        let values = catalogue()
            .into_iter()
            .map(|channel| {
                let id = channel["id"].as_str().unwrap().to_owned();
                let value = match channel["type"].as_str().unwrap() {
                    "number" => json!(1.0),
                    "integer" => json!(1),
                    "boolean" => json!(true),
                    "string" => json!("x".repeat(256)),
                    _ => unreachable!("catalogue has a supported scalar type"),
                };
                (id, value)
            })
            .collect::<BTreeMap<_, _>>();
        Snapshot {
            producer_id: "p".repeat(36),
            epoch: 7,
            session: Some("s".repeat(96)),
            aircraft: Some("a".repeat(256)),
            plane_id: Some(1),
            model_time: Some(1.0),
            sequence: Some(u32::MAX),
            capture_sequence: u64::MAX,
            fresh: true,
            status: "live".repeat(8),
            age_ms: Some(0),
            captured_at: Some(Instant::now()),
            values,
            malformed_frames: 0,
            error: Some("e".repeat(128)),
        }
    }

    #[test]
    fn maximum_catalogue_snapshot_is_bounded_and_native_clone_keeps_capture_witness() {
        let snapshot = maximum_text_snapshot();
        let bytes = snapshot.retained_bytes();
        assert!(bytes > size_of::<Snapshot>());
        assert!(
            bytes <= LIMITS.max_snapshot_bytes,
            "retained bytes: {bytes}"
        );
        assert!(snapshot.values.len() <= 128);

        let clone = snapshot.clone();
        assert_eq!(clone.capture_sequence, snapshot.capture_sequence);
        assert_eq!(clone.captured_at, snapshot.captured_at);
        assert!(clone.retained_bytes() <= LIMITS.max_snapshot_bytes);

        // Runtime capture fields remain internal and are not added to the JSON DTO.
        let dto = serde_json::to_value(&snapshot).unwrap();
        assert!(dto.get("captured_at").is_none());
        assert!(dto.get("capture_sequence").is_none());

        let journal = NativeStream::<Snapshot, Snapshot>::new(
            snapshot.epoch,
            snapshot.clone(),
            1,
            "dcs-ownship:test",
            LIMITS,
        )
        .unwrap();
        let mut reader = journal.subscribe().unwrap();
        let Delivery::Snapshot(delivery) = reader.try_recv().unwrap().unwrap() else {
            panic!("initial native baseline expected");
        };
        assert_eq!(delivery.value.capture_sequence, snapshot.capture_sequence);
        assert_eq!(delivery.value.captured_at, snapshot.captured_at);
    }

    #[test]
    fn unexpected_nested_values_are_rejected_instead_of_undercounted() {
        for value in [json!([1]), json!({"nested": true})] {
            let mut snapshot = maximum_text_snapshot();
            snapshot.values.insert("test.nested".into(), value);
            assert_eq!(snapshot.retained_bytes(), usize::MAX);
        }
    }
}
