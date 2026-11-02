use crate::{codec::encode_limited, Delivery, LossReason, PublishError};
use serde::Serialize;
use serde_json::Value;

pub const MAX_SNAPSHOT_BYTES: usize = 1024 * 1024;
pub const MAX_CHANGE_BYTES: usize = 64 * 1024;
// Four escaped 128-byte identities, decimal counters and fixed keys fit here.
pub(crate) const ENVELOPE_RESERVE: usize = 2048;

/// Observation evidence only: transport delivery never grants command authority.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireFrame {
    pub kind: String,
    pub topic: String,
    pub source: String,
    pub session: String,
    pub generation: String,
    pub revision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<String>,
    /// Capture evidence in its named clock domain. Loss has no capture evidence:
    /// its timestamp is "0" and clock is "none", never a freshness renewal.
    pub captured_us: String,
    pub clock_id: String,
    pub continuity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone)]
pub(crate) struct Identity {
    pub topic: String,
    pub source: String,
    pub session: String,
}

/// JSON bytes produced by the bounded wire validator, never caller-supplied bytes.
pub(crate) struct EncodedPayload(Vec<u8>);

impl EncodedPayload {
    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

impl Identity {
    pub(crate) fn frame(&self, delivery: Delivery<Value, Value>) -> WireFrame {
        let mut frame = WireFrame {
            kind: String::new(),
            topic: self.topic.clone(),
            source: self.source.clone(),
            session: self.session.clone(),
            generation: String::new(),
            revision: String::new(),
            sequence: None,
            captured_us: "0".into(),
            clock_id: "none".into(),
            continuity: String::new(),
            payload: None,
            reason: None,
        };
        match delivery {
            Delivery::Snapshot(snapshot) => {
                frame.kind = "snapshot".into();
                frame.generation = snapshot.generation.to_string();
                frame.revision = snapshot.revision.to_string();
                frame.captured_us = snapshot.captured_us.to_string();
                frame.clock_id = snapshot.clock_id;
                frame.continuity = "baseline".into();
                frame.payload = Some(snapshot.value);
            }
            Delivery::Event(event) => {
                frame.kind = "change".into();
                frame.generation = event.generation.to_string();
                frame.revision = event.revision.to_string();
                frame.sequence = Some(event.sequence.to_string());
                frame.captured_us = event.captured_us.to_string();
                frame.clock_id = event.clock_id;
                frame.continuity = "continuous".into();
                frame.payload = Some(event.value);
            }
            Delivery::Loss(loss) => {
                frame.kind = "loss".into();
                frame.generation = loss.generation.to_string();
                frame.revision = loss.revision.to_string();
                frame.continuity = "lost".into();
                frame.reason = Some(
                    match loss.reason {
                        LossReason::Overflow => "overflow",
                        LossReason::GenerationReplaced => "generation-replaced",
                        LossReason::BaselineReset => "baseline-reset",
                        LossReason::Closed => "closed",
                        LossReason::DecodeFailure => "decode-failure",
                    }
                    .into(),
                );
            }
        }
        frame
    }

    pub(crate) fn validate(
        &self,
        payload: &Value,
        captured_us: u64,
        clock: &str,
        change: bool,
    ) -> Result<(), PublishError> {
        self.encode(payload, captured_us, clock, change).map(|_| ())
    }

    pub(crate) fn encode(
        &self,
        payload: &Value,
        captured_us: u64,
        clock: &str,
        change: bool,
    ) -> Result<EncodedPayload, PublishError> {
        crate::journal::validate_clock(clock)?;
        let limit = if change {
            MAX_CHANGE_BYTES
        } else {
            MAX_SNAPSHOT_BYTES
        };
        let too_large = if change {
            PublishError::FrameTooLarge
        } else {
            PublishError::SnapshotTooLarge
        };
        let bytes = encode_limited(payload, limit - ENVELOPE_RESERVE)
            .map_err(|error| crate::codec::map_encode_error(error, too_large.clone()))?;
        // Validate a worst-counter envelope before entering the producer lock.
        let delivery = if change {
            Delivery::Event(crate::Event {
                generation: u64::MAX,
                sequence: u64::MAX,
                revision: u64::MAX,
                captured_us,
                clock_id: clock.into(),
                value: Value::Null,
            })
        } else {
            Delivery::Snapshot(crate::Snapshot {
                generation: u64::MAX,
                revision: u64::MAX,
                captured_us,
                clock_id: clock.into(),
                value: Value::Null,
            })
        };
        // Replacing the literal `null` with the encoded payload gives the exact
        // frame length, including escaped metadata and worst-case counters.
        let envelope = encode_limited(&self.frame(delivery), limit)
            .map_err(|error| crate::codec::map_encode_error(error, too_large.clone()))?;
        if envelope.len() - b"null".len() + bytes.len() > limit {
            return Err(too_large);
        }
        Ok(EncodedPayload(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn identity() -> Identity {
        Identity {
            topic: "\\\"".repeat(64),
            source: "\"\\".repeat(64),
            session: "\\".repeat(128),
        }
    }

    fn worst_delivery(
        payload: Value,
        captured_us: u64,
        clock: &str,
        change: bool,
    ) -> Delivery<Value, Value> {
        if change {
            Delivery::Event(crate::Event {
                generation: u64::MAX,
                sequence: u64::MAX,
                revision: u64::MAX,
                captured_us,
                clock_id: clock.into(),
                value: payload,
            })
        } else {
            Delivery::Snapshot(crate::Snapshot {
                generation: u64::MAX,
                revision: u64::MAX,
                captured_us,
                clock_id: clock.into(),
                value: payload,
            })
        }
    }

    #[test]
    fn encoded_payload_and_metadata_length_match_full_serialization() {
        let identity = identity();
        let clock = "\\\"".repeat(64);
        for captured_us in [0, 1, u64::MAX] {
            for change in [false, true] {
                for payload in [
                    Value::Null,
                    json!("\\\"\n☃"),
                    json!({"nested":[1,true,null,{"x":"y"}]}),
                ] {
                    let bytes = identity
                        .encode(&payload, captured_us, &clock, change)
                        .unwrap()
                        .into_bytes();
                    assert_eq!(bytes, serde_json::to_vec(&payload).unwrap());
                    let full = serde_json::to_vec(&identity.frame(worst_delivery(
                        payload,
                        captured_us,
                        &clock,
                        change,
                    )))
                    .unwrap();
                    let metadata = serde_json::to_vec(&identity.frame(worst_delivery(
                        Value::Null,
                        captured_us,
                        &clock,
                        change,
                    )))
                    .unwrap();
                    assert_eq!(metadata.len() - 4 + bytes.len(), full.len());
                    assert!(metadata.len() - 4 <= ENVELOPE_RESERVE);
                }
            }
        }
    }

    #[test]
    fn conservative_payload_thresholds_preserve_full_wire_bounds() {
        let identity = identity();
        let clock = "\\".repeat(128);
        for (change, limit, error) in [
            (false, MAX_SNAPSHOT_BYTES, PublishError::SnapshotTooLarge),
            (true, MAX_CHANGE_BYTES, PublishError::FrameTooLarge),
        ] {
            // A JSON string adds exactly two quote bytes to its ASCII contents.
            let payload = json!("x".repeat(limit - ENVELOPE_RESERVE - 2));
            let bytes = identity
                .encode(&payload, u64::MAX, &clock, change)
                .unwrap()
                .into_bytes();
            assert_eq!(bytes.len(), limit - ENVELOPE_RESERVE);
            let full = serde_json::to_vec(&identity.frame(worst_delivery(
                payload,
                u64::MAX,
                &clock,
                change,
            )))
            .unwrap();
            assert!(full.len() <= limit);
            let oversized = json!("x".repeat(limit - ENVELOPE_RESERVE - 1));
            assert_eq!(
                identity.encode(&oversized, u64::MAX, &clock, change).err(),
                Some(error)
            );
        }
        assert_eq!(
            identity.encode(&json!(0), 0, "bad clock", true).err(),
            Some(PublishError::InvalidClockId)
        );
    }
}
