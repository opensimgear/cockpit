//! Additive volatile endpoint leases. Tokens are capabilities and never state.
use serde_json::Value;
pub(crate) const TTL: u64 = 5000;
pub(crate) fn capability(value: &Value) -> Result<Value, String> {
    if value["schemaVersion"] != 1 || value["scope"] != "endpoint" || value["maxLeaseMs"] != TTL {
        return Err(crate::actions::INVALID.into());
    }
    Ok(serde_json::json!({"schemaVersion":1,"scope":"endpoint","maxLeaseMs":TTL}))
}
pub(crate) fn token(value: &Value) -> Result<&str, String> {
    value
        .as_str()
        .filter(|s| {
            s.len() == 36
                && s.bytes().enumerate().all(|(i, b)| {
                    if [8, 13, 18, 23].contains(&i) {
                        b == b'-'
                    } else {
                        b.is_ascii_hexdigit()
                    }
                })
                && s.bytes().any(|b| b != b'0' && b != b'-')
        })
        .ok_or_else(|| "Invalid output ownership token.".into())
}
fn lease(request: &Value) -> Result<u64, String> {
    request["leaseId"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= u32::MAX as u64)
        .ok_or_else(|| "Invalid output lease identity.".into())
}
fn ttl(request: &Value) -> Result<u64, String> {
    request["ttlMs"]
        .as_u64()
        .filter(|n| (250..=TTL).contains(n))
        .ok_or_else(|| "Output lease duration must be 250 to 5000 ms.".into())
}
pub(crate) fn child(
    command: &str,
    request: &Value,
    revision: u64,
    descriptors: &[Value],
) -> Result<String, String> {
    if command == "action_ownership" {
        let offset = request
            .get("offset")
            .map_or(Some(0), Value::as_u64)
            .filter(|n| *n <= 16)
            .ok_or("Invalid output ownership cursor.")?;
        return Ok(format!("ownership {offset}"));
    }
    let secret = token(&request["ownershipToken"])?;
    match command {
        "action_renew" => Ok(format!(
            "renew {secret} {} {}",
            lease(request)?,
            ttl(request)?
        )),
        "action_release" => Ok(format!("release {secret} {}", lease(request)?)),
        "action_claim" | "action_invoke_owned" => {
            if request["catalogueRevision"].as_u64() != Some(revision) {
                return Err(
                    "The device action catalogue changed. Discover its actions again.".into(),
                );
            }
            let action = descriptors
                .iter()
                .find(|a| a["id"] == request["actionId"])
                .ok_or("This action is no longer advertised by the device.")?;
            if action["readable"] != true || action["semantics"] != "setter" {
                return Err("Output ownership requires a readable setter.".into());
            }
            let args = crate::actions::arguments(action, &request["values"])?;
            let suffix = if args.is_empty() {
                String::new()
            } else {
                format!(" {}", args.join(" "))
            };
            let id = action["id"].as_str().unwrap();
            Ok(if command == "action_claim" {
                format!("claim {revision} {id} {secret} {}{suffix}", ttl(request)?)
            } else {
                format!(
                    "invoke_owned {secret} {} {revision} {id}{suffix}",
                    lease(request)?
                )
            })
        }
        _ => Err("Unknown output ownership command.".into()),
    }
}
pub(crate) fn acknowledgement(
    reply: &Value,
    revision: u64,
    request: &Value,
    command: &str,
) -> Result<(), String> {
    if crate::actions::header(reply)? != revision {
        return Err(crate::actions::INVALID.into());
    }
    if !reply["leaseId"]
        .as_u64()
        .is_some_and(|n| n > 0 && n <= u32::MAX as u64)
        || !reply["endpoint"].as_str().is_some_and(|s| {
            !s.is_empty()
                && s.len() <= 31
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
        || command != "action_claim" && reply["leaseId"] != request["leaseId"]
        || command == "action_release" && reply["released"] != true
        || command != "action_release" && !reply["remainingMs"].as_u64().is_some_and(|n| n <= TTL)
        || reply.get("token").is_some()
        || reply.get("ownershipToken").is_some()
    {
        return Err(crate::actions::INVALID.into());
    }
    Ok(())
}
pub(crate) fn ownership_page(reply: &Value, revision: u64, offset: usize) -> Result<Value, String> {
    if crate::actions::header(reply)? != revision {
        return Err(crate::actions::INVALID.into());
    }
    let total = reply["total"]
        .as_u64()
        .filter(|n| *n <= 16)
        .ok_or(crate::actions::INVALID)? as usize;
    let entries = reply["ownership"]
        .as_array()
        .filter(|a| a.len() <= 4)
        .ok_or(crate::actions::INVALID)?;
    if offset > total || offset + entries.len() > total {
        return Err(crate::actions::INVALID.into());
    }
    let end = offset + entries.len();
    if end == total && !reply["next"].is_null()
        || end < total && (entries.is_empty() || reply["next"].as_u64() != Some(end as u64))
    {
        return Err(crate::actions::INVALID.into());
    }
    let mut seen = std::collections::HashSet::new();
    let mut sanitized = Vec::new();
    for entry in entries {
        let valid_id = |v: &Value| {
            v.as_str().is_some_and(|s| {
                !s.is_empty()
                    && s.len() <= 31
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            })
        };
        if !valid_id(&entry["endpoint"])
            || !valid_id(&entry["actionId"])
            || !seen.insert(entry["endpoint"].as_str().unwrap())
            || !entry["owned"].is_boolean()
            || !entry["leaseId"]
                .as_u64()
                .is_some_and(|n| n <= u32::MAX as u64)
            || !entry["remainingMs"].as_u64().is_some_and(|n| n <= TTL)
            || !entry["fallback"].as_array().is_some_and(|a| a.len() <= 4)
            || !matches!(
                entry["fallbackStatus"].as_str(),
                Some("none" | "applied" | "failed")
            )
        {
            return Err(crate::actions::INVALID.into());
        }
        sanitized.push(serde_json::json!({"endpoint":entry["endpoint"],"actionId":entry["actionId"],"owned":entry["owned"],"leaseId":entry["leaseId"],"remainingMs":entry["remainingMs"],"fallback":entry["fallback"],"fallbackStatus":entry["fallbackStatus"]}));
    }
    Ok(
        serde_json::json!({"schemaVersion":1,"catalogueRevision":revision,"total":total,"next":reply["next"],"ownership":sanitized}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn action() -> Value {
        json!({"id":"led.set","endpoint":"led","label":"LED","readable":true,"semantics":"setter","parameters":[{"id":"on","label":"On","type":"boolean"}]})
    }
    fn request() -> Value {
        json!({"ownershipToken":"00112233-4455-6677-8899-aabbccddeeff","leaseId":42,"ttlMs":5000,"catalogueRevision":1,"actionId":"led.set","values":[false]})
    }
    #[test]
    fn bounded_tokens_commands_and_readable_setters() {
        let r = request();
        let a = vec![action()];
        assert_eq!(
            child("action_claim", &r, 1, &a).unwrap(),
            "claim 1 led.set 00112233-4455-6677-8899-aabbccddeeff 5000 0"
        );
        assert!(child("action_invoke_owned", &r, 1, &a)
            .unwrap()
            .starts_with("invoke_owned "));
        assert!(child("action_release", &r, 1, &a).unwrap().ends_with(" 42"));
        assert!(child("action_renew", &r, 1, &a)
            .unwrap()
            .ends_with("42 5000"));
        assert_eq!(
            child("action_ownership", &json!({}), 1, &a).unwrap(),
            "ownership 0"
        );
        for (key, value) in [
            ("ownershipToken", json!("bad")),
            ("leaseId", json!(0)),
            ("ttlMs", json!(5001)),
            ("ttlMs", json!(249)),
            ("catalogueRevision", json!(2)),
            ("actionId", json!("missing")),
            ("values", json!(["bad"])),
        ] {
            let mut x = r.clone();
            x[key] = value;
            assert!(child(
                if key == "leaseId" {
                    "action_invoke_owned"
                } else {
                    "action_claim"
                },
                &x,
                1,
                &a
            )
            .is_err());
        }
        let mut once = action();
        once["semantics"] = json!("oneShot");
        assert!(child("action_claim", &r, 1, &[once]).is_err());
        assert!(child("bad", &r, 1, &a).is_err());
        assert!(child("action_ownership", &json!({"offset":17}), 1, &a).is_err());
        assert!(token(&json!("00000000-0000-0000-0000-000000000000")).is_err());
    }
    #[test]
    fn replies_cannot_leak_capabilities_or_change_lease() {
        let r = request();
        let mut reply = json!({"ok":true,"schemaVersion":1,"catalogueRevision":1,"leaseId":42,"endpoint":"led","remainingMs":5000});
        assert!(acknowledgement(&reply, 1, &r, "action_claim").is_ok());
        assert!(acknowledgement(&reply, 1, &r, "action_renew").is_ok());
        reply["released"] = json!(true);
        assert!(acknowledgement(&reply, 1, &r, "action_release").is_ok());
        for (key, value) in [
            ("leaseId", json!(0)),
            ("leaseId", json!(43)),
            ("endpoint", json!("bad name")),
            ("remainingMs", json!(5001)),
            ("ownershipToken", r["ownershipToken"].clone()),
            ("catalogueRevision", json!(2)),
        ] {
            let mut x = reply.clone();
            x[key] = value;
            assert!(acknowledgement(&x, 1, &r, "action_renew").is_err());
        }
    }
    #[test]
    fn ownership_pages_have_exact_bounded_progression_and_no_tokens() {
        let empty = json!({"ok":true,"schemaVersion":1,"catalogueRevision":1,"total":0,"next":null,"ownership":[]});
        assert!(ownership_page(&empty, 1, 0).is_ok());
        let entry = json!({"endpoint":"led","actionId":"led.set","owned":true,"leaseId":1,"remainingMs":2000,"fallback":[false],"fallbackStatus":"none","token":"secret"});
        let good = json!({"ok":true,"schemaVersion":1,"catalogueRevision":1,"total":2,"next":1,"ownership":[entry]});
        let result = ownership_page(&good, 1, 0).unwrap();
        assert!(!result.to_string().contains("secret"));
        for (key, value) in [
            ("total", json!(17)),
            ("next", json!(0)),
            ("next", Value::Null),
            ("ownership", json!([])),
            ("catalogueRevision", json!(2)),
        ] {
            let mut bad = good.clone();
            bad[key] = value;
            assert!(ownership_page(&bad, 1, 0).is_err());
        }
        for (key, value) in [
            ("endpoint", json!("bad name")),
            ("owned", json!(1)),
            ("leaseId", json!(-1)),
            ("remainingMs", json!(5001)),
            ("fallback", json!([1, 2, 3, 4, 5])),
            ("fallbackStatus", json!("bad")),
        ] {
            let mut bad = good.clone();
            bad["ownership"][0][key] = value;
            assert!(ownership_page(&bad, 1, 0).is_err());
        }
        let mut dup = good;
        dup["ownership"] = json!([dup["ownership"][0], dup["ownership"][0]]);
        dup["next"] = Value::Null;
        assert!(ownership_page(&dup, 1, 0).is_err());
    }
    #[test]
    fn opt_in_capability_requires_endpoint_scope_and_known_bounds() {
        assert!(
            capability(&json!({"schemaVersion":1,"scope":"endpoint","maxLeaseMs":5000})).is_ok()
        );
        for invalid in [
            Value::Null,
            json!({"schemaVersion":2,"scope":"endpoint","maxLeaseMs":5000}),
            json!({"schemaVersion":1,"scope":"client","maxLeaseMs":5000}),
            json!({"schemaVersion":1,"scope":"endpoint","maxLeaseMs":5001}),
        ] {
            assert!(capability(&invalid).is_err());
        }
    }
}
