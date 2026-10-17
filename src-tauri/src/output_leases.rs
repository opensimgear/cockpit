//! Additive volatile endpoint leases. Tokens are capabilities and never state.
use serde_json::Value;
pub(crate) const TTL: u64 = 5000;
pub(crate) fn capability(value: &Value) -> Result<Value, String> {
    if value["schemaVersion"] != 1 || value["scope"] != "endpoint" || value["maxLeaseMs"] != TTL {
        return Err(crate::actions::INVALID.into());
    }
    let mut capability = serde_json::json!({"schemaVersion":1,"scope":"endpoint","maxLeaseMs":TTL});
    if let Some(policy) = value.get("restorePolicy") {
        if policy != "if-unchanged" {
            return Err(crate::actions::INVALID.into());
        }
        capability["restorePolicy"] = policy.clone();
    }
    Ok(capability)
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
    let capability = reply.get("ownership").map(capability).transpose()?;
    if command == "action_release" {
        let policy = capability
            .as_ref()
            .is_some_and(|c| c["restorePolicy"] == "if-unchanged");
        if (policy || reply.get("fallbackStatus").is_some())
            && !matches!(
                reply["fallbackStatus"].as_str(),
                Some("applied" | "preserved")
            )
        {
            return Err(crate::actions::INVALID.into());
        }
    } else if reply.get("fallbackStatus").is_some() {
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
/// Call after acknowledgement validation; retain only bounded public receipt
/// fields, never an ownership token or unrelated firmware response content.
pub(crate) fn lease_receipt(reply: &Value) -> Value {
    let mut receipt = serde_json::json!({"leaseId":reply["leaseId"],"endpoint":reply["endpoint"],"remainingMs":reply["remainingMs"],"released":reply["released"]});
    if let Some(status) = reply.get("fallbackStatus") {
        receipt["fallbackStatus"] = status.clone();
    }
    receipt
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
            || entry["fallbackStatus"] == "preserved"
                && (entry["owned"] != false || entry["leaseId"] == 0 || entry["remainingMs"] != 0)
            || !matches!(
                entry["fallbackStatus"].as_str(),
                Some("none" | "applied" | "preserved" | "failed")
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
#[path = "output_leases_tests.rs"]
mod tests;
