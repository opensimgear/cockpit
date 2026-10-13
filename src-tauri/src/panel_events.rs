//! Validate the bounded device event log before any simulator command is sent.
use serde_json::Value;
use std::collections::HashSet;

pub const INVALID: &str = "The device returned an invalid input event log.";
fn number(value: &Value, minimum: u64, maximum: u64) -> Result<u64, String> {
    value
        .as_u64()
        .filter(|n| *n >= minimum && *n <= maximum)
        .ok_or_else(|| INVALID.into())
}

pub fn validate(reply: &Value, read: Option<(u32, u32)>) -> Result<(), String> {
    if reply.to_string().len() > 4091 || reply["ok"] != true || reply["schemaVersion"] != 1 {
        return Err(INVALID.into());
    }
    let stream = number(&reply["stream"], 1, u32::MAX as u64)?;
    let head = number(&reply["head"], 0, u32::MAX as u64)?;
    let oldest = number(&reply["oldest"], 1, (head + 1).min(u32::MAX as u64))?;
    number(&reply["overflow"], 0, u32::MAX as u64)?;
    let count = number(&reply["buttonCount"], 0, 128)?;
    let held = reply["held"].as_array().ok_or(INVALID)?;
    if held.len() != count as usize || held.iter().any(|v| !v.is_boolean()) {
        return Err(INVALID.into());
    }
    let encoders = reply["encoderButtons"].as_array().ok_or(INVALID)?;
    let mut seen = HashSet::new();
    if encoders.len() > 64
        || encoders
            .iter()
            .any(|v| v.as_u64().is_none_or(|n| n >= count || !seen.insert(n)))
    {
        return Err(INVALID.into());
    }
    if let Some((expected_stream, after)) = read {
        if stream != expected_stream as u64
            || reply["after"].as_u64() != Some(after as u64)
            || after as u64 > head
            || (after as u64) + 1 < oldest
        {
            return Err(INVALID.into());
        }
        let events = reply["events"].as_array().ok_or(INVALID)?;
        if events.len() > 16 {
            return Err(INVALID.into());
        }
        let mut next = after as u64;
        for event in events {
            next += 1;
            if event["sequence"].as_u64() != Some(next)
                || next > head
                || event["control"].as_u64().is_none_or(|n| n >= count)
                || !event["pressed"].is_boolean()
            {
                return Err(INVALID.into());
            }
        }
        if reply["next"].as_u64() != Some(next)
            || reply["more"].as_bool() != Some(next < head)
            || (next < head && events.is_empty())
        {
            return Err(INVALID.into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn state() -> Value {
        json!({"ok":true,"schemaVersion":1,"stream":7,"head":2,"oldest":1,"overflow":0,"buttonCount":2,"held":[false,false],"encoderButtons":[1]})
    }
    fn page() -> Value {
        let mut v = state();
        v["after"] = json!(0);
        v["next"] = json!(2);
        v["more"] = json!(false);
        v["events"] = json!([{"sequence":1,"control":0,"pressed":true},{"sequence":2,"control":0,"pressed":false}]);
        v
    }
    #[test]
    fn complete_short_press_is_preserved_and_empty_or_partial_pages_are_bounded() {
        assert!(validate(&state(), None).is_ok());
        assert!(validate(&page(), Some((7, 0))).is_ok());
        let mut v = page();
        v["events"].as_array_mut().unwrap().pop();
        v["next"] = json!(1);
        v["more"] = json!(true);
        assert!(validate(&v, Some((7, 0))).is_ok());
        v["after"] = json!(2);
        v["events"] = json!([]);
        v["next"] = json!(2);
        v["more"] = json!(false);
        assert!(validate(&v, Some((7, 2))).is_ok());
    }
    #[test]
    fn malformed_metadata_and_cursor_shapes_never_reach_bindings() {
        for (key, value) in [
            ("ok", json!(false)),
            ("schemaVersion", json!(2)),
            ("stream", json!(0)),
            ("head", json!(-1)),
            ("oldest", json!(4)),
            ("overflow", json!(4294967296u64)),
            ("buttonCount", json!(129)),
            ("held", json!([false])),
            ("held", json!([false, 0])),
            ("encoderButtons", json!([1, 1])),
            ("encoderButtons", json!([2])),
        ] {
            let mut v = state();
            v[key] = value;
            assert!(validate(&v, None).is_err(), "{key}");
        }
        for (key, value) in [
            ("after", json!(1)),
            ("next", json!(1)),
            ("more", json!(true)),
            ("events", json!([])),
        ] {
            let mut v = page();
            v[key] = value;
            assert!(validate(&v, Some((7, 0))).is_err(), "{key}");
        }
        assert!(validate(&page(), Some((8, 0))).is_err());
        assert!(validate(&page(), Some((7, 3))).is_err());
        let mut v = page();
        v["oldest"] = json!(2);
        assert!(validate(&v, Some((7, 0))).is_err());
    }
    #[test]
    fn duplicate_reordered_wrong_type_and_oversized_events_are_rejected() {
        for (key, value) in [
            ("sequence", json!(2)),
            ("control", json!(2)),
            ("pressed", json!(1)),
        ] {
            let mut v = page();
            v["events"][0][key] = value;
            assert!(validate(&v, Some((7, 0))).is_err());
        }
        let mut v = page();
        v["events"] = json!(vec![json!({"sequence":1,"control":0,"pressed":true}); 17]);
        assert!(validate(&v, Some((7, 0))).is_err());
        let mut v = state();
        v["untrusted"] = json!("x".repeat(4092));
        assert!(validate(&v, None).is_err());
        assert!(validate(&Value::Null, None).is_err());
    }
}
