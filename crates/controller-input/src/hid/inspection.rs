//! Byte slices are read from the same native owner and fenced by their exact JSON digest.
use super::Service;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(super) fn slice(bytes: &[u8], request: &Value) -> Result<Value, String> {
    let offset = request.get("offset").map_or(Ok(0), |value| {
        value
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or("Invalid HID byte offset")
    })?;
    let limit = request
        .get("limit")
        .map_or(Ok(super::transfer::MAX_CHUNK_BYTES), |value| {
            value
                .as_u64()
                .filter(|n| (1..=super::transfer::MAX_CHUNK_BYTES as u64).contains(n))
                .map(|n| n as usize)
                .ok_or("HID byte limit must be 1..32768")
        })?;
    if offset > bytes.len() {
        return Err("HID byte offset exceeds retained data".into());
    }
    let end = offset.saturating_add(limit).min(bytes.len());
    Ok(
        json!({"offset":offset,"nextOffset":end,"totalBytes":bytes.len(),"done":end==bytes.len(),
        "encoding":"base64","data":super::transfer::encode(&bytes[offset..end])}),
    )
}
impl Service {
    pub(super) fn inspection_slice(&self, request: &Value) -> Result<Value, String> {
        let source = super::required(request, "source")?;
        let command = match source {
            "list" => "controller_hid_list",
            "catalog" => "controller_hid_catalog",
            "inspect" => "controller_hid_inspect",
            "snapshot" => "controller_hid_snapshot",
            "state" => "controller_hid_state",
            "replay_read" if matches!(request["part"].as_str(), Some("descriptor" | "state")) => {
                "controller_hid_replay_read"
            }
            _ => return Err("Unsupported HID inspection slice source".into()),
        };
        // Do not retain another payload cache: mutable sources must still match
        // the first chunk's immutable content identity on every continuation.
        let mut native_request = request.clone();
        if source == "replay_read" {
            native_request.as_object_mut().unwrap().remove("limit");
        }
        let value = self.request(command, &native_request)?;
        for identity in ["contractFingerprint", "snapshotId"] {
            if request
                .get(identity)
                .is_some_and(|expected| value.get(identity) != Some(expected))
            {
                return Err(format!("Stale HID inspection {identity}; inspect again"));
            }
        }
        let bytes = value.to_string();
        let fingerprint = format!("{:x}", Sha256::digest(bytes.as_bytes()));
        if request
            .get("contentFingerprint")
            .is_some_and(|v| v.as_str() != Some(&fingerprint))
        {
            return Err(
                "Stale HID inspection content fingerprint; restart from offset zero".into(),
            );
        }
        if request["offset"].as_u64().is_some_and(|n| n > 0)
            && request.get("contentFingerprint").is_none()
        {
            return Err("HID inspection continuation requires contentFingerprint".into());
        }
        let mut result = slice(bytes.as_bytes(), request)?;
        result["source"] = json!(source);
        result["contentFingerprint"] = json!(fingerprint);
        Ok(result)
    }
}
