//! Presentation of existing native samples, mirroring hid-inspector/model.ts.
//! This is deliberately not a HID decoder or a retained baseline.
use serde_json::{json, Value};

pub(super) fn rows(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
pub(super) fn exact(value: &Value) -> String {
    match value {
        Value::Null => "unavailable".into(),
        Value::String(s) => s.clone(),
        _ => value.to_string(),
    }
}
pub(super) fn detail(value: &Value) -> &Value {
    value
        .get("detail")
        .filter(|v| v.as_object().is_some_and(|o| !o.is_empty()))
        .unwrap_or(value)
}
fn first<'a>(values: &[&'a Value]) -> &'a Value {
    values.iter().copied().find(|v| !v.is_null()).unwrap_or(&Value::Null)
}
pub(super) fn field_count(value: &Value) -> usize {
    let source = detail(value);
    rows(&source["descriptor"]["fields"]).len()
        + rows(&source["elements"]).len()
        + rows(&source["capabilities"]["reports"])
            .iter()
            .map(|report| rows(&report["valueCaps"]).len() + rows(&report["buttonCaps"]).len())
            .sum::<usize>()
}
pub(super) fn fields(value: &Value) -> Vec<Value> {
    let source = detail(value);
    let mut result = vec![];
    for (index, field) in rows(&source["descriptor"]["fields"]).iter().enumerate() {
        result.push(json!({"id":format!("descriptor:{index}"),"collection":exact(&field["collection"]),"kind":exact(&field["kind"]),"reportId":field["globals"]["report_id"],"usage":field["primary"],"metadata":field}));
    }
    for report in rows(&source["capabilities"]["reports"]) {
        for (key, prefix) in [("valueCaps", "hidp-value"), ("buttonCaps", "hidp-button")] {
            for (index, field) in rows(&report[key]).iter().enumerate() {
                result.push(json!({"id":format!("{prefix}:{index}"),"collection":exact(&field["linkCollection"]),"kind":exact(&report["kind"]),"reportId":field["reportId"],"usage":{"page":field["usagePage"],"usage":field["usage"],"range":field["range"]},"metadata":field}));
            }
        }
    }
    for field in rows(&source["elements"]) {
        result.push(json!({"id":format!("iohid-cookie:{}",exact(&field["cookie"])),"collection":exact(&field["parentCookie"]),"kind":exact(&field["reportType"]),"reportId":field["reportId"],"usage":{"page":field["usagePage"],"id":field["usage"]},"metadata":field}));
    }
    result
}
pub(super) fn find_field<'a>(fields: &'a [Value], value: &Value) -> Option<&'a Value> {
    let id = exact(&value["fieldId"]);
    fields.iter().find(|f| {
        let key = exact(&f["id"]);
        id == key || id.starts_with(&format!("{key}:"))
    })
}
pub(super) fn sample(value: &Value) -> Value {
    let sample = &value["sample"];
    if sample.get("state").is_some() {
        return sample.clone();
    }
    for (key, state) in [("Integer", "valid"), ("Null", "null"), ("Invalid", "invalid")] {
        if let Some(raw) = sample.get(key) {
            return json!({"state":state,"kind":"integer","value":raw});
        }
    }
    if let Some(raw) = sample.get("Array") {
        let mut row = json!({"state":"valid","kind":"array-selector"});
        if let Some(map) = raw.as_object() {
            row.as_object_mut().unwrap().extend(map.clone());
        }
        return row;
    }
    if let Some(raw) = sample.get("Bytes") {
        return json!({"state":"valid","kind":"bytes","bytes":raw});
    }
    let mut row = json!({"state":"unknown"});
    if let Some(map) = sample["Unknown"].as_object() {
        row.as_object_mut().unwrap().extend(map.clone());
    }
    row
}
fn usage_has(value: &Value, key: &str, expected: u64) -> bool {
    match value {
        Value::Object(map) => {
            map.get(key).and_then(Value::as_u64) == Some(expected) || map.values().any(|v| usage_has(v, key, expected))
        }
        Value::Array(a) => a.iter().any(|v| usage_has(v, key, expected)),
        _ => false,
    }
}
pub(super) fn presentation(value: &Value, field: Option<&Value>) -> Value {
    let s = sample(value);
    let f = field.unwrap_or(&Value::Null);
    let m = &f["metadata"];
    let g = &m["globals"];
    let min = first(&[&value["logical"]["minimum"], &g["logical_min"], &m["logicalMin"]]);
    let max = first(&[&value["logical"]["maximum"], &g["logical_max"], &m["logicalMax"]]);
    let semantics = exact(&value["semantics"]).to_lowercase();
    let usage = first(&[&value["usages"], &f["usage"], &value["usage"]]);
    let hat = usage_has(usage, "id", 57)
        || usage_has(usage, "usage", 57)
        || (value["usagePage"] == 1 && value["usage"] == 57);
    let button = exact(&f["id"]).starts_with("hidp-button")
        || value["usagePage"] == 9
        || usage_has(usage, "page", 9)
        || usage_has(usage, "usagePage", 9);
    let kind = if s["kind"] == "array-selector" {
        "Usage selectors"
    } else if s["kind"] == "bytes" {
        "Opaque bytes"
    } else if semantics == "relative" {
        "Relative delta"
    } else if semantics == "event" {
        "Event"
    } else if hat {
        "Hat"
    } else if button {
        "Button"
    } else if m["bitSize"] == 1 || g["size"] == 1 {
        "Bitfield"
    } else if semantics == "absolute" {
        "Absolute position"
    } else {
        "Opaque value"
    };
    let raw = first(&[&s["value"], &s["selector"], &s["bytes"], &s["encoded"]]);
    let percent = if s["state"] == "valid" && kind == "Absolute position" {
        exact(raw)
            .parse::<i128>()
            .ok()
            .zip(exact(min).parse::<i128>().ok())
            .zip(exact(max).parse::<i128>().ok())
            .and_then(|((n, lo), hi)| {
                if hi <= lo || n < lo || n > hi {
                    return None;
                }
                // Integer division before conversion keeps the visual ratio exact for wide values.
                let span = hi.checked_sub(lo)? as u128;
                let delta = n.checked_sub(lo)? as u128;
                let whole = delta / span;
                let mut remainder = delta % span;
                let mut scaled = whole * 10000;
                for power in [1000, 100, 10, 1] {
                    // The native domains are at most 64 bits; checked arithmetic also
                    // keeps an unrepresentable imported domain explicitly unavailable.
                    remainder = remainder.checked_mul(10)?;
                    scaled += (remainder / span) * power;
                    remainder %= span;
                }
                Some(scaled as f64 / 100.0)
            })
    } else {
        None
    };
    json!({"kind":kind,"quality":exact(&s["state"]),"text":exact(raw),"min":exact(min),"max":exact(max),"percent":percent,"active":s["state"]=="valid"&&button&&!raw.is_null()&&exact(raw)!="0","sample":s})
}
pub(super) fn report_values(reports: &[Value]) -> Vec<Value> {
    let mut result = vec![];
    for report in reports {
        let projection = first(&[&report["samples"], &report["decoded"]]);
        let mut values = vec![];
        for value in rows(&projection["values"]) {
            let mut v = value.clone();
            v["fieldId"] = value
                .get("fieldId")
                .cloned()
                .unwrap_or_else(|| json!(format!("descriptor:{}", exact(&value["field"]))));
            v["elementIndex"] = first(&[&value["elementIndex"], &value["element"]]).clone();
            values.push(v);
        }
        for data in rows(&projection["data"]) {
            for value in rows(&data["values"]) {
                values.push(json!({"fieldId":format!("hidp-value:{}:data:{}",exact(&value["capabilityIndex"]),exact(&data["dataIndex"])),"elementIndex":0,"semantics":value["semantics"],"sample":{"state":value["quality"],"kind":if value["semantics"]=="array-selector" {"array-selector"} else {"integer"},"value":value["native"]},"encoded":data["rawValue"]}));
            }
        }
        for value in rows(&projection["elements"]) {
            values.push(json!({"fieldId":format!("iohid-cookie:{}",exact(&value["cookie"])),"elementIndex":0,"semantics":match value["relative"].as_bool(){Some(true)=>"relative",Some(false)=>"absolute",None=>"opaque"},"usagePage":value["usagePage"],"usage":value["usage"],"sample":if value["integer"].is_string(){json!({"state":"valid","kind":"integer","value":value["integer"]})}else{json!({"state":"unsupported","reason":"No exact native scalar"})},"elementValueHex":value["valueHex"]}));
        }
        for mut value in values {
            value["reportId"] = first(&[&report["reportId"], &report["windowsReportId"]]).clone();
            value["sequence"] = report["sequence"].clone();
            value["receivedUs"] = report["receivedUs"].clone();
            result.push(value);
        }
    }
    result
}
pub(super) fn matches(field: Option<&Value>, value: &Value, args: &Value, previous: &[Value]) -> bool {
    let f = field.unwrap_or(&Value::Null);
    let contains = |key: &str, haystack: String| {
        args[key]
            .as_str()
            .is_none_or(|needle| haystack.to_lowercase().contains(&needle.to_lowercase()))
    };
    if args["kind"].as_str().is_some_and(|kind| f["kind"] != kind) {
        return false;
    }
    if args["report"]
        .as_str()
        .is_some_and(|id| exact(first(&[&value["reportId"], &f["reportId"]])) != id)
    {
        return false;
    }
    if !contains("collection", exact(&f["collection"]))
        || !contains(
            "usage",
            json!([value["usages"], value["usage"], f["usage"], value["fieldId"]]).to_string(),
        )
    {
        return false;
    }
    if args["fieldId"].as_str().is_some_and(|id| value["fieldId"] != id)
        || args
            .get("elementIndex")
            .is_some_and(|index| value["elementIndex"] != *index)
    {
        return false;
    }
    if args["changed"] == true
        && previous
            .iter()
            .rev()
            .find(|old| old["fieldId"] == value["fieldId"] && old["elementIndex"] == value["elementIndex"])
            .is_some_and(|old| old["sample"] == value["sample"])
    {
        return false;
    }
    args["active"] != true || presentation(value, field)["active"] == true
}
