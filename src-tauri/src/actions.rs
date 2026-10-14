//! Bounded device-advertised actions. Descriptors, never board names, determine
//! validation and wire encoding. An invoke acknowledgement is applied state,
//! not electrical verification of an attached output.
use serde_json::{json, Value};
use std::collections::HashSet;

pub const INVALID: &str = "The device returned an invalid action catalogue or state.";
pub const UNKNOWN: &str = "The action reply was lost. The output may have changed; refresh its state before invoking again.";
pub const MAX_ACTIONS: usize = 16;

fn identifier(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        !s.is_empty()
            && s.len() <= 31
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    })
}
fn label(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|s| !s.is_empty() && s.len() <= 63 && !s.chars().any(char::is_control))
}
pub fn header(reply: &Value) -> Result<u64, String> {
    if reply["ok"] != true || reply["schemaVersion"] != 1 {
        return Err(INVALID.into());
    }
    reply["catalogueRevision"]
        .as_u64()
        .filter(|v| *v > 0 && *v <= u32::MAX as u64)
        .ok_or(INVALID.into())
}
pub fn descriptor(action: &Value) -> Result<(), String> {
    let params = action["parameters"].as_array().ok_or(INVALID)?;
    if !identifier(&action["id"])
        || !identifier(&action["endpoint"])
        || !label(&action["label"])
        || !matches!(action["semantics"].as_str(), Some("setter" | "oneShot"))
        || !action["readable"].is_boolean()
        || params.len() > 4
    {
        return Err(INVALID.into());
    }
    let mut ids = HashSet::new();
    for p in params {
        if !identifier(&p["id"]) || !label(&p["label"]) || !ids.insert(p["id"].as_str()) {
            return Err(INVALID.into());
        }
        let valid = match p["type"].as_str() {
            Some("boolean" | "rgb") => true,
            Some("integer") => p["minimum"]
                .as_i64()
                .zip(p["maximum"].as_i64())
                .is_some_and(|(min, max)| {
                    min >= i32::MIN as i64 && max <= i32::MAX as i64 && min <= max
                }),
            Some("enum") => p["options"].as_array().is_some_and(|options| {
                !options.is_empty()
                    && options.len() <= 16
                    && options.iter().all(identifier)
                    && options
                        .iter()
                        .map(|v| v.as_str().unwrap())
                        .collect::<HashSet<_>>()
                        .len()
                        == options.len()
            }),
            Some("string") => p["maxBytes"]
                .as_u64()
                .is_some_and(|max| max > 0 && max <= 63),
            _ => false,
        };
        if !valid {
            return Err(INVALID.into());
        }
    }
    if let Some(display) = action.get("display") {
        let columns = display["columns"].as_u64().filter(|n| (1..=63).contains(n));
        let rows = display["rows"].as_u64().filter(|n| (1..=4).contains(n));
        if columns.zip(rows).is_none_or(|(columns, rows)| {
            columns * rows > 63
                || params.len() != 1
                || params[0]["type"] != "string"
                || params[0]["maxBytes"]
                    .as_u64()
                    .is_none_or(|max| max < columns * rows)
        }) || !matches!(display["encoding"].as_str(), Some("ascii" | "utf8"))
            || action["semantics"] != "setter"
            || action["readable"] != true
        {
            return Err(INVALID.into());
        }
    }
    Ok(())
}
/// Validate a page and exact cursor progression before caching any descriptors.
pub fn page(
    reply: &Value,
    offset: usize,
) -> Result<(u64, usize, Option<usize>, Vec<Value>), String> {
    let revision = header(reply)?;
    let total = reply["total"]
        .as_u64()
        .filter(|v| *v <= MAX_ACTIONS as u64)
        .ok_or(INVALID)? as usize;
    let actions = reply["actions"].as_array().ok_or(INVALID)?;
    if actions.len() > 4 || offset > total || offset + actions.len() > total {
        return Err(INVALID.into());
    }
    let expected = offset + actions.len();
    let next = if expected == total {
        if !reply["next"].is_null() {
            return Err(INVALID.into());
        }
        None
    } else {
        if actions.is_empty() || reply["next"].as_u64() != Some(expected as u64) {
            return Err(INVALID.into());
        }
        Some(expected)
    };
    for action in actions {
        descriptor(action)?;
    }
    Ok((revision, total, next, actions.clone()))
}
pub fn arguments(action: &Value, values: &Value) -> Result<Vec<String>, String> {
    descriptor(action)?;
    let values = values
        .as_array()
        .ok_or("Action arguments must be an array.")?;
    let parameters = action["parameters"].as_array().unwrap();
    if values.len() != parameters.len() {
        return Err("The action argument count does not match its descriptor.".into());
    }
    parameters
        .iter()
        .zip(values)
        .map(|(p, v)| {
            let encoded = match p["type"].as_str().unwrap() {
                "boolean" => v.as_bool().map(|b| if b { "1" } else { "0" }.into()),
                "integer" => v
                    .as_i64()
                    .filter(|n| {
                        *n >= p["minimum"].as_i64().unwrap() && *n <= p["maximum"].as_i64().unwrap()
                    })
                    .map(|n| n.to_string()),
                "enum" => v
                    .as_str()
                    .filter(|s| {
                        p["options"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|option| option == *s)
                    })
                    .map(str::to_owned),
                "rgb" => v
                    .as_str()
                    .filter(|s| s.len() == 6 && s.bytes().all(|b| b.is_ascii_hexdigit()))
                    .map(str::to_ascii_uppercase),
                "string" => v
                    .as_str()
                    .filter(|s| {
                        s.len() <= p["maxBytes"].as_u64().unwrap() as usize
                            && !s.chars().any(char::is_control)
                            && action.get("display").is_none_or(|display| {
                                s.chars().count() as u64
                                    <= display["columns"].as_u64().unwrap()
                                        * display["rows"].as_u64().unwrap()
                                    && (display["encoding"] != "ascii" || s.is_ascii())
                            })
                    })
                    .map(|s| {
                        if s.is_empty() {
                            "-".into()
                        } else {
                            s.as_bytes().iter().map(|b| format!("{b:02X}")).collect()
                        }
                    }),
                _ => None,
            };
            encoded.ok_or_else(|| {
                format!(
                    "Invalid value for action parameter {}.",
                    p["label"].as_str().unwrap()
                )
            })
        })
        .collect()
}
pub fn states(
    reply: &Value,
    revision: u64,
    descriptors: &[Value],
    invocation: bool,
) -> Result<Vec<Value>, String> {
    if header(reply)? != revision {
        return Err("The device action catalogue changed. Discover its actions again.".into());
    }
    let states = if invocation {
        vec![json!({"id":reply["id"], "values":reply["values"]})]
    } else {
        reply["states"].as_array().ok_or(INVALID)?.clone()
    };
    if states.len() > MAX_ACTIONS {
        return Err(INVALID.into());
    }
    let mut ids = HashSet::new();
    for state in &states {
        if !ids.insert(state["id"].as_str()) {
            return Err(INVALID.into());
        }
        let action = descriptors
            .iter()
            .find(|action| action["id"] == state["id"])
            .ok_or(INVALID)?;
        if !invocation && action["readable"] != true {
            return Err(INVALID.into());
        }
        arguments(action, &state["values"]).map_err(|_| INVALID.to_owned())?;
    }
    if !invocation && descriptors.iter().filter(|a| a["readable"] == true).count() != states.len() {
        return Err(INVALID.into());
    }
    Ok(states)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn action() -> Value {
        json!({"id":"output.set","label":"Output","endpoint":"output","semantics":"setter","readable":true,"parameters":[
        {"id":"enabled","label":"Enabled","type":"boolean"}, {"id":"level","label":"Level","type":"integer","minimum":-1,"maximum":100},
        {"id":"mode","label":"Mode","type":"enum","options":["auto","on","off"]}, {"id":"color","label":"Color","type":"rgb"}]})
    }
    #[test]
    fn display_geometry_encoding_and_applied_text_are_bounded() {
        let mut display = json!({"id":"display.text","label":"Text","endpoint":"display",
            "semantics":"setter","readable":true,"parameters":[{"id":"text","label":"Text","type":"string","maxBytes":63}],
            "display":{"columns":4,"rows":2,"encoding":"utf8"}});
        assert_eq!(
            arguments(&display, &json!(["éééééééé"])).unwrap()[0].len(),
            32
        );
        assert!(arguments(&display, &json!(["123456789"])).is_err());
        display["display"]["encoding"] = json!("ascii");
        assert!(arguments(&display, &json!(["é"])).is_err());
        assert!(arguments(&display, &json!(["12345678"])).is_ok());
        for (key, value) in [
            ("columns", json!(0)),
            ("columns", json!(64)),
            ("rows", json!(0)),
            ("rows", json!(5)),
            ("encoding", json!("latin1")),
        ] {
            let mut invalid = display.clone();
            invalid["display"][key] = value;
            assert!(descriptor(&invalid).is_err());
        }
        let mut invalid = display.clone();
        invalid["display"]["columns"] = json!(32);
        assert!(descriptor(&invalid).is_err());
        for (key, value) in [
            ("semantics", json!("oneShot")),
            ("readable", json!(false)),
            ("parameters", json!([])),
        ] {
            let mut invalid = display.clone();
            invalid[key] = value;
            assert!(descriptor(&invalid).is_err());
        }
        let mut invalid = display.clone();
        invalid["parameters"][0]["maxBytes"] = json!(7);
        assert!(descriptor(&invalid).is_err());
        let mut invalid = display.clone();
        invalid["parameters"][0]["type"] = json!("boolean");
        assert!(descriptor(&invalid).is_err());
        let readback = json!({"ok":true,"schemaVersion":1,"catalogueRevision":1,"states":[{"id":"display.text","values":["é"]}]});
        assert!(states(&readback, 1, &[display], false).is_err());
    }
    #[test]
    fn encodes_every_type_without_command_injection() {
        assert_eq!(
            arguments(&action(), &json!([true, 42, "auto", "ab008f"])).unwrap(),
            vec!["1", "42", "auto", "AB008F"]
        );
        for args in [
            json!([1, 42, "auto", "ab008f"]),
            json!([true, 101, "auto", "ab008f"]),
            json!([true, 1, "auto x", "ab008f"]),
            json!([true, 1, "auto", "#ab008f"]),
            json!([]),
        ] {
            assert!(arguments(&action(), &args).is_err());
        }
        let mut text = action();
        text["parameters"] = json!([{"id":"text","label":"Text","type":"string","maxBytes":8}]);
        assert_eq!(
            arguments(&text, &json!(["é hi"])).unwrap(),
            vec!["C3A9206869"]
        );
        assert_eq!(arguments(&text, &json!([""])).unwrap(), vec!["-"]);
        for value in ["ééééé", "\n"] {
            assert!(arguments(&text, &json!([value])).is_err());
        }
    }
    #[test]
    fn validates_schema_constraints_and_no_argument_actions() {
        for (path, value) in [
            ("id", json!("bad id")),
            ("endpoint", json!("")),
            ("semantics", json!("toggle")),
            ("readable", json!(1)),
            ("label", json!("\n")),
        ] {
            let mut a = action();
            a[path] = value;
            assert!(descriptor(&a).is_err());
        }
        for (index, key, value) in [
            (1, "maximum", json!(-2)),
            (2, "options", json!(["on", "on"])),
            (2, "options", json!([])),
            (3, "type", json!("float")),
        ] {
            let mut a = action();
            a["parameters"][index][key] = value;
            assert!(descriptor(&a).is_err());
        }
        for max in [0, 64] {
            let mut a = action();
            a["parameters"] = json!([{"id":"text","label":"Text","type":"string","maxBytes":max}]);
            assert!(descriptor(&a).is_err());
        }
        let mut a = action();
        a["parameters"] = json!([]);
        assert_eq!(arguments(&a, &json!([])).unwrap(), Vec::<String>::new());
        let mut a = action();
        a["parameters"][1]["id"] = a["parameters"][0]["id"].clone();
        assert!(descriptor(&a).is_err());
    }
    #[test]
    fn pagination_is_bounded_monotonic_and_complete() {
        let p = json!({"ok":true,"schemaVersion":1,"catalogueRevision":7,"total":2,"next":1,"actions":[action()]});
        assert_eq!(page(&p, 0).unwrap().2, Some(1));
        let mut end = p.clone();
        end["next"] = Value::Null;
        assert_eq!(page(&end, 1).unwrap().2, None);
        for (key, value) in [
            ("next", json!(0)),
            ("total", json!(17)),
            ("schemaVersion", json!(2)),
            ("catalogueRevision", json!(0)),
            ("actions", json!([])),
        ] {
            let mut p = p.clone();
            p[key] = value;
            assert!(page(&p, 0).is_err());
        }
        assert!(page(&p, 3).is_err());
    }
    #[test]
    fn readback_must_match_catalogue_and_validated_values() {
        let reply = json!({"ok":true,"schemaVersion":1,"catalogueRevision":7,"states":[{"id":"output.set","values":[true,42,"on","123456"]}]});
        assert!(states(&reply, 7, &[action()], false).is_ok());
        assert!(states(&reply, 8, &[action()], false).is_err());
        let mut missing = reply.clone();
        missing["states"] = json!([]);
        assert!(states(&missing, 7, &[action()], false).is_err());
        let mut duplicate = reply.clone();
        duplicate["states"]
            .as_array_mut()
            .unwrap()
            .push(reply["states"][0].clone());
        assert!(states(&duplicate, 7, &[action()], false).is_err());
        let mut invalid = reply.clone();
        invalid["states"][0]["values"][1] = json!(999);
        assert!(states(&invalid, 7, &[action()], false).is_err());
        let applied = json!({"ok":true,"schemaVersion":1,"catalogueRevision":7,"id":"output.set","values":reply["states"][0]["values"]});
        assert!(states(&applied, 7, &[action()], true).is_ok());
    }
}
