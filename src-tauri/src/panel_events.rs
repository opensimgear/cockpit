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
    if reply.get("contactButtonCount").is_some() || reply.get("axisCount").is_some() {
        let contacts = number(&reply["contactButtonCount"], 0, count)?;
        let maintained = reply["maintainedButtons"].as_array().ok_or(INVALID)?;
        let mut seen = HashSet::new();
        if maintained.len() > contacts as usize
            || maintained
                .iter()
                .any(|v| v.as_u64().is_none_or(|n| n >= contacts || !seen.insert(n)))
            || encoders.len() > 16
            || encoders.len() % 2 != 0
            || encoders
                .iter()
                .enumerate()
                .any(|(i, v)| v.as_u64() != Some(contacts + i as u64))
        {
            return Err(INVALID.into());
        }
        let selectors = reply["selectorButtons"].as_array().ok_or(INVALID)?;
        if selectors.len() > 8
            || contacts + encoders.len() as u64 + 3 * selectors.len() as u64 != count
        {
            return Err(INVALID.into());
        }
        for (index, group) in selectors.iter().enumerate() {
            let group = group.as_array().ok_or(INVALID)?;
            if group.len() != 3
                || group.iter().enumerate().any(|(i, v)| {
                    v.as_u64() != Some(contacts + encoders.len() as u64 + (index * 3 + i) as u64)
                })
            {
                return Err(INVALID.into());
            }
        }
        let axes = number(&reply["axisCount"], 0, 8)? as usize;
        for key in ["axes", "axisNeutral", "axisUsage", "axisAvailable"] {
            let values = reply[key].as_array().ok_or(INVALID)?;
            if values.len() != axes
                || values.iter().any(|v| {
                    if key == "axisAvailable" {
                        !v.is_boolean()
                    } else if key == "axisUsage" {
                        v.as_u64().is_none_or(|n| !(48..=56).contains(&n))
                    } else {
                        v.as_u64().is_none_or(|n| n > 65535)
                    }
                })
            {
                return Err(INVALID.into());
            }
        }
        number(&reply["sampledAtMs"], 0, u32::MAX as u64)?;
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
    #[test]
    fn typed_sources_and_current_axes_have_exact_shapes_and_bounds() {
        let v = json!({"ok":true,"schemaVersion":1,"stream":1,"head":0,"oldest":1,"overflow":0,
            "buttonCount":7,"held":[false,false,false,false,false,false,false],"contactButtonCount":2,
            "maintainedButtons":[0],"encoderButtons":[2,3],"selectorButtons":[[4,5,6]],
            "axisCount":2,"axes":[65535,0],"axisNeutral":[32767,0],"axisUsage":[48,56],
            "axisAvailable":[true,false],"sampledAtMs":4294967295u64});
        assert!(validate(&v, None).is_ok());
        for (key, value) in [
            ("contactButtonCount", json!(8)),
            ("maintainedButtons", json!([0, 0])),
            ("maintainedButtons", json!([2])),
            ("encoderButtons", json!([2])),
            ("encoderButtons", json!([3, 2])),
            ("selectorButtons", json!([])),
            ("selectorButtons", json!([null])),
            ("selectorButtons", json!([[4, 5]])),
            ("selectorButtons", json!([[4, 6, 5]])),
            ("axisCount", json!(9)),
            ("axes", json!([65536, 0])),
            ("axisNeutral", json!([-1, 0])),
            ("axisUsage", json!([47, 48])),
            ("axisUsage", json!([48, 57])),
            ("axisAvailable", json!([true, 0])),
            ("axisAvailable", json!([true])),
            ("sampledAtMs", json!(4294967296u64)),
        ] {
            let mut invalid = v.clone();
            invalid[key] = value;
            assert!(validate(&invalid, None).is_err(), "{key}");
        }
        let mut incomplete = state();
        incomplete["axisCount"] = json!(0);
        assert!(validate(&incomplete, None).is_err());
        let empty = json!({"ok":true,"schemaVersion":1,"stream":1,"head":0,"oldest":1,"overflow":0,
            "buttonCount":0,"held":[],"contactButtonCount":0,"maintainedButtons":[],"encoderButtons":[],"selectorButtons":[],
            "axisCount":0,"axes":[],"axisNeutral":[],"axisUsage":[],"axisAvailable":[],"sampledAtMs":0});
        assert!(validate(&empty, None).is_ok());
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
