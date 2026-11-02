use super::MAX_PACKET;
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) struct Packet {
    pub(super) session: String,
    pub(super) sequence: u32,
    pub(super) model_time: f64,
    pub(super) aircraft: Option<String>,
    pub(super) plane: Option<u64>,
    pub(super) allowed: bool,
    pub(super) paused: bool,
    pub(super) stopped: bool,
    pub(super) values: BTreeMap<String, Value>,
}
fn bounded_text(value: &Value, limit: usize) -> Option<&str> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= limit && !s.chars().any(char::is_control))
}
pub(super) fn parse(bytes: &[u8], fields: &BTreeMap<String, String>) -> Result<Packet, String> {
    let invalid = || "Invalid bounded DCS ownship export packet.".to_owned();
    if bytes.is_empty() || bytes.len() > MAX_PACKET {
        return Err(invalid());
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if value["schemaVersion"] != 1 || value["source"] != "osg-dcs-ownship" {
        return Err(invalid());
    }
    let session = bounded_text(&value["session"], 96)
        .ok_or_else(invalid)?
        .to_owned();
    let sequence = value["sequence"]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(invalid)?;
    let model_time = value["modelTime"]
        .as_f64()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .ok_or_else(invalid)?;
    let aircraft = if value["aircraft"].is_null() {
        None
    } else {
        Some(
            bounded_text(&value["aircraft"], 256)
                .ok_or_else(invalid)?
                .to_owned(),
        )
    };
    let plane = if value["planeId"].is_null() {
        None
    } else {
        Some(
            value["planeId"]
                .as_u64()
                .filter(|v| *v > 0 && *v <= 9_007_199_254_740_991)
                .ok_or_else(invalid)?,
        )
    };
    let allowed = value["allowed"].as_bool().ok_or_else(invalid)?;
    let paused = value["paused"].as_bool().ok_or_else(invalid)?
        | value["activePause"].as_bool().ok_or_else(invalid)?;
    let stopped = value["stopped"].as_bool().ok_or_else(invalid)?;
    let entries = value["values"]
        .as_object()
        .filter(|v| v.len() <= 128)
        .ok_or_else(invalid)?;
    let mut values = BTreeMap::new();
    for (id, sample) in entries {
        let Some(kind) = fields.get(id) else {
            continue;
        };
        let valid = match kind.as_str() {
            "number" => sample
                .as_f64()
                .is_some_and(|v| v.is_finite() && v.abs() <= 1e30),
            "integer" => sample.as_f64().is_some_and(|v| {
                v.is_finite() && v.fract() == 0.0 && v.abs() <= 9_007_199_254_740_991.0
            }),
            "boolean" => sample.is_boolean(),
            "string" => sample
                .as_str()
                .is_some_and(|v| v.len() <= 256 && !v.chars().any(char::is_control)),
            _ => false,
        };
        if !valid {
            continue;
        }
        values.insert(id.clone(), sample.clone());
    }
    Ok(Packet {
        session,
        sequence,
        model_time,
        aircraft,
        plane,
        allowed,
        paused,
        stopped,
        values,
    })
}
