//! Device operation diagnostics are identity-bound metadata, never input authority.
use crate::publication::{digest_id, ensure, id};
use serde_json::Value;

pub fn validate_panel_operation(value: &Value, instance: &str, publication: &str) -> Result<Value, String> {
    let object = value.as_object().ok_or("Invalid panel operation.")?;
    ensure(object.len() <= 14 && object.keys().all(|key| matches!(key.as_str(),
        "instanceId" | "publicationHash" | "phase" | "completedBytes" | "totalBytes" |
        "cacheHits" | "cacheMisses" | "errorCode" | "retryable" | "message" | "attemptId" | "sessionId" | "generation" | "diagnostic")), "Invalid panel operation fields.")?;
    ensure(id(instance) && digest_id(publication) && value["instanceId"] == instance
        && value["publicationHash"] == publication, "Panel operation belongs to another assignment.")?;
    ensure(matches!(value["phase"].as_str(), Some("opening" | "checking-cache" | "downloading" |
        "rendering" | "awaiting-neutral" | "ready" | "failed")), "Invalid panel operation phase.")?;
    for (field, maximum) in [("completedBytes", 1200 * 1024), ("totalBytes", 1200 * 1024),
        ("cacheHits", 32), ("cacheMisses", 32)] {
        ensure(value.get(field).is_none_or(|v| v.as_u64().is_some_and(|n| n <= maximum)), "Invalid panel operation progress.")?;
    }
    ensure(value["completedBytes"].as_u64().unwrap_or(0) <= value["totalBytes"].as_u64().unwrap_or(0), "Invalid panel operation progress.")?;
    ensure(value.get("retryable").is_none_or(Value::is_boolean), "Invalid panel operation retry flag.")?;
    let counter = |v: &Value| v.as_u64().is_some() || v.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 20 && s.bytes().all(|b|b.is_ascii_digit()) && s.parse::<u64>().is_ok());
    ensure(value.get("attemptId").is_none_or(counter), "Invalid panel operation attempt.")?;
    ensure(value.get("sessionId").is_none_or(|v|v.as_str().is_some_and(id)) && value.get("generation").is_none_or(counter), "Invalid panel operation session.")?;
    ensure(value.get("errorCode").is_none_or(|v| v.as_str().is_some_and(|s| !s.is_empty()
        && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'))), "Invalid panel operation error code.")?;
    ensure(value.get("message").is_none_or(|v| v.as_str().is_some_and(|s| !s.is_empty()
        && s.len() <= 191 && !s.chars().any(char::is_control))), "Invalid panel operation message.")?;
    ensure(value["phase"] != "failed" || (value["errorCode"].is_string() && value["retryable"].is_boolean()), "Panel failure requires an error code and retry policy.")?;
    if let Some(diagnostic) = value.get("diagnostic") {
        let fields = diagnostic.as_object().ok_or("Invalid panel diagnostic.")?;
        ensure(fields.len() == 6 && fields.keys().all(|key| matches!(key.as_str(),
            "widget" | "definition" | "layer" | "field" | "required" | "available")), "Invalid panel diagnostic fields.")?;
        for key in ["widget", "definition", "layer", "field"] {
            ensure(diagnostic[key].as_str().is_some_and(|s| s.len() <= 64 && s.bytes().all(|c| (32..=126).contains(&c))), "Invalid panel diagnostic identity.")?;
        }
        ensure(diagnostic["required"].as_u64().is_some() && diagnostic["available"].as_u64().is_some(), "Invalid panel diagnostic budget.")?;
    }
    Ok(value.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn operation_metadata_is_bounded_and_bound_to_exact_assignment() {
        let hash = format!("sha256:{}", "a".repeat(64));
        let good = json!({"instanceId":"instance","publicationHash":hash,"phase":"downloading", "completedBytes":64,"totalBytes":128,"cacheHits":2,"cacheMisses":1});
        assert!(validate_panel_operation(&good, "instance", &hash).is_ok());
        assert!(validate_panel_operation(&good, "other", &hash).is_err());
        for (key, value) in [("completedBytes",json!(129)), ("cacheMisses",json!(33)), ("phase",json!("armed")), ("message",json!("bad\ntext")), ("token",json!("secret"))] {
            let mut bad = good.clone(); bad[key] = value;
            assert!(validate_panel_operation(&bad,"instance",&hash).is_err(), "{key}");
        }
        let mut failed = good; failed["phase"] = json!("failed");
        assert!(validate_panel_operation(&failed,"instance",&hash).is_err());
        failed["errorCode"] = json!("cache-full"); failed["retryable"] = json!(false);
        assert!(validate_panel_operation(&failed,"instance",&hash).is_ok());
        failed["diagnostic"] = json!({"widget":"master-widget","definition":"button","layer":"caption","field":"fontSize","required":128,"available":48});
        assert_eq!(validate_panel_operation(&failed,"instance",&hash).unwrap()["diagnostic"]["required"],128);
        for (key,value) in [("widget",json!("x".repeat(65))),("layer",json!("bad\nline")),("required",json!(-1)),("available",json!(0.5)),("token",json!("secret"))] {
            let mut bad=failed.clone();bad["diagnostic"][key]=value;
            assert!(validate_panel_operation(&bad,"instance",&hash).is_err(),"{key}");
        }
        failed["diagnostic"]=Value::Null;assert!(validate_panel_operation(&failed,"instance",&hash).is_err());
    }
}
