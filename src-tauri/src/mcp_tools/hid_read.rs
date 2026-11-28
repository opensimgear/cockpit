use super::*;

/// Headroom for the MCP envelope, operation identity and JSON-RPC framing.
pub(super) const MAX_DATA_BYTES: usize = 1_000_000;
const PAGE_BYTES: usize = 880 * 1024;

pub(super) fn validate_args(args: &Value) -> Result<(), Value> {
    for key in ["after", "elapsedUs", "sequence"] {
        if let Some(value) = args.get(key) {
            let valid = value
                .as_str()
                .is_some_and(|s| s.parse::<u64>().is_ok_and(|n| n.to_string() == s));
            if !valid {
                return Err(failure(
                    "invalid_arguments",
                    &format!("{key} must be a canonical unsigned 64-bit decimal."),
                    false,
                ));
            }
        }
    }
    if let Some(hash) = args.get("contentFingerprint") {
        if !hash.as_str().is_some_and(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }) {
            return Err(failure(
                "invalid_arguments",
                "contentFingerprint must be the returned lowercase SHA-256 digest.",
                false,
            ));
        }
    }
    Ok(())
}

impl Tools {
    pub(super) fn hid_read(&self, name: &str, args: &Value, client: &str) -> Result<Value, Value> {
        let mut request = select(
            args,
            &[
                "interfaceId",
                "generation",
                "replayId",
                "after",
                "elapsedUs",
                "seek",
                "part",
                "byteOffset",
                "maxBytes",
            ],
        );
        if name == "controller_hid_replay_read" {
            request["bounded"] = json!(true);
            if let Some(limit) = args.get("limit") {
                request["limit"] = limit.clone();
            }
        }
        let mut data = self.native(name, request, client, Some("controllers"))?;
        let source = name.strip_prefix("controller_hid_").unwrap();
        let rows = match source {
            "list" => Some("/interfaces"),
            "catalog" => Some("/snapshots"),
            "state" => Some("/result/values"),
            _ => None,
        };
        if let Some(path) = rows {
            if data.pointer(path).is_some_and(Value::is_array) {
                return page(data, path, source, args);
            }
        }
        if data.to_string().len() > PAGE_BYTES {
            data = slice_hint(&data, source, args);
        }
        Ok(data)
    }
}

fn slice_hint(data: &Value, source: &str, args: &Value) -> Value {
    let mut next = select(
        args,
        &[
            "interfaceId",
            "generation",
            "replayId",
            "after",
            "elapsedUs",
            "seek",
            "part",
        ],
    );
    next["source"] = json!(source);
    next["offset"] = json!(0);
    next["limit"] = json!(32768);
    let mut result = select(
        data,
        &[
            "interfaceId",
            "generation",
            "contractFingerprint",
            "snapshotId",
            "replayId",
        ],
    );
    result["sliceRequired"] = json!({"tool":"controller_hid_inspection_slice","arguments":next});
    result
}

fn page(mut data: Value, path: &str, source: &str, args: &Value) -> Result<Value, Value> {
    let offset = args["offset"].as_u64().unwrap_or(0) as usize;
    let limit = args["limit"].as_u64().unwrap_or(128) as usize;
    let rows = data.pointer(path).unwrap().as_array().unwrap();
    let total = rows.len();
    if offset > total {
        return Err(failure(
            "invalid_cursor",
            "Offset exceeds the current HID page. Restart at zero.",
            false,
        ));
    }
    let encoded = data.to_string();
    let digest = format!("{:x}", Sha256::digest(encoded.as_bytes()));
    let expected = args.get("contentFingerprint").and_then(Value::as_str);
    if expected.is_some_and(|hash| hash != digest) || (offset > 0 && expected.is_none()) {
        return Err(failure(
            "stale_cursor",
            "HID content changed or the continuation fingerprint is absent. Restart at zero.",
            false,
        ));
    }
    let rows = data
        .pointer_mut(path)
        .unwrap()
        .take()
        .as_array_mut()
        .map(std::mem::take)
        .unwrap();
    let mut used = data.to_string().len() + 512;
    let mut selected = Vec::new();
    let mut next = offset;
    for row in rows.into_iter().skip(offset).take(limit) {
        let cost = row.to_string().len() + 1;
        if used + cost > PAGE_BYTES {
            break;
        }
        used += cost;
        selected.push(row);
        next += 1;
    }
    if used > PAGE_BYTES || (next == offset && offset < total) {
        return Ok(slice_hint(&data, source, args));
    }
    *data.pointer_mut(path).unwrap() = Value::Array(selected);
    data["page"] = json!({"offset":offset,"nextOffset":next,"total":total,"done":next==total,"contentFingerprint":digest});
    Ok(data)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paged_discovery_fences_content_and_recovers_large_rows() {
        let original = json!({"interfaces":(0..150).map(|i|json!({"interfaceId":i})).collect::<Vec<_>>(),"failures":[]});
        let first = page(original.clone(), "/interfaces", "list", &json!({})).unwrap();
        assert_eq!(first["interfaces"].as_array().unwrap().len(), 128);
        let args = json!({"offset":128,"contentFingerprint":first["page"]["contentFingerprint"]});
        let last = page(original.clone(), "/interfaces", "list", &args).unwrap();
        assert_eq!(last["interfaces"][0]["interfaceId"], 128);
        assert_eq!(last["page"]["done"], true);
        let mut changed = original.clone();
        changed["interfaces"][0] = json!("other");
        assert_eq!(
            page(changed, "/interfaces", "list", &args).unwrap_err()["error"]["code"],
            "stale_cursor"
        );
        assert!(page(original, "/interfaces", "list", &json!({"offset":151})).is_err());
        let large = json!({"result":{"values":["x".repeat(PAGE_BYTES)]},"generation":"g"});
        let hint = page(
            large,
            "/result/values",
            "state",
            &json!({"generation":"g","interfaceId":"x"}),
        )
        .unwrap();
        assert_eq!(
            hint["sliceRequired"]["arguments"],
            json!({"interfaceId":"x","generation":"g","source":"state","offset":0,"limit":32768})
        );
    }
}
