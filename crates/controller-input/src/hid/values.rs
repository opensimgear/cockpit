//! Exact, generation-local absolute observations. This layer never scales or integrates.
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{self, Write},
    sync::Arc,
};
#[cfg(test)]
#[path = "values_tests.rs"]
mod tests;

#[derive(Default)]
pub(super) struct Current {
    detail: Arc<Value>,
    values: BTreeMap<String, (usize, Value)>,
    bytes: usize,
    received: Option<String>,
    status: Option<&'static str>,
    diagnostics: Value,
}
impl Current {
    pub fn new(detail: Value) -> Self {
        Self {
            detail: Arc::new(detail),
            ..Self::default()
        }
    }
    pub fn clear(&mut self) {
        self.values.clear();
        self.bytes = 0;
    }
    pub fn state(&self, latest: u64, terminal: Option<&Value>) -> Value {
        json!({"status":if terminal.is_some() {"offline"} else {self.status.unwrap_or("waiting")},
            "latest":latest.to_string(),"receivedUs":self.received,
            "values":self.values.values().map(|(_, value)| value).collect::<Vec<_>>(),"diagnostics":self.diagnostics,
            "terminal":terminal})
    }
    pub fn observe(
        &mut self,
        seq: u64,
        micros: u128,
        projection: &Value,
        provenance: Option<&Value>,
    ) {
        let report = provenance
            .map(|p| &p["reportId"])
            .unwrap_or_else(|| {
                projection
                    .get("report_id")
                    .unwrap_or(&projection["reportId"])
            })
            .clone();
        let diagnostics = projection
            .get("diagnostics")
            .or_else(|| projection.get("failures"));
        self.diagnostics = diagnostics.cloned().unwrap_or_else(|| json!([]));
        self.received = Some(micros.to_string());
        // A failed decode is no observation, and cannot leave the old report looking current.
        let failed = diagnostics.is_some_and(|d| {
            d.as_array().is_some_and(|a| {
                a.iter().any(|entry| {
                    entry["reason"]
                        != "Trailing delivery bytes preserved outside the declared payload"
                })
            })
        });
        if projection.get("unavailable").is_some() || failed {
            self.invalidate(&report);
            self.status = Some(if projection.get("unavailable").is_some() {
                "unsupported"
            } else {
                "stale"
            });
            return;
        }
        // Own a cheap immutable reference independently of the mutable baseline.
        // Construct at most one row before applying the baseline's byte/entry cap.
        let detail = Arc::clone(&self.detail);
        let rows: Box<dyn Iterator<Item = Value> + '_> =
            if let Some(values) = projection["values"].as_array() {
                Box::new(
                    values
                        .iter()
                        .filter(|v| {
                            v["semantics"]
                                .as_str()
                                .is_some_and(|s| s.eq_ignore_ascii_case("absolute"))
                        })
                        .map(|v| portable(&detail, v)),
                )
            } else if let Some(elements) = projection["elements"].as_array() {
                Box::new(
                    elements
                        .iter()
                        .filter(|v| v["relative"] == false)
                        .map(|v| element(&detail, v)),
                )
            } else if projection["data"].is_array() {
                Box::new(windows(&detail, projection))
            } else {
                self.invalidate(&report);
                self.status = Some("unsupported");
                return;
            };
        // Raw reports describe their whole layout; element callbacks describe only
        // the cookies actually delivered. In particular, do not retain old pressed
        // button data omitted by the next HidP_GetData result.
        if !projection["elements"].is_array() {
            self.invalidate(&report);
        }
        self.status = Some("live");
        self.retain_rows(rows, &report, seq, micros);
    }
    fn retain_rows(
        &mut self,
        rows: impl IntoIterator<Item = Value>,
        report: &Value,
        seq: u64,
        micros: u128,
    ) {
        for mut row in rows {
            if row["semantics"] != "absolute" {
                continue;
            }
            row["reportId"] = report.clone();
            row["sequence"] = json!(seq.to_string());
            row["receivedUs"] = json!(micros.to_string());
            let key = format!("{}:{}:{}", report, row["fieldId"], row["elementIndex"]);
            if let Some((old_size, _)) = self.values.remove(&key) {
                self.bytes -= old_size;
            }
            let size = (self.values.len() < 16_384)
                .then(|| bounded_size(&row, 8 * 1024 * 1024 - self.bytes))
                .flatten();
            let Some(size) = size else {
                self.clear();
                self.status = Some("stale");
                self.diagnostics = json!([{"kind":"capacity","reason":"Current exact value budget exceeded; baseline invalidated"}]);
                return;
            };
            self.bytes += size;
            self.values.insert(key, (size, row));
        }
    }
    fn invalidate(&mut self, report: &Value) {
        self.values.retain(|_, (size, value)| {
            if &value["reportId"] == report {
                self.bytes -= *size;
                false
            } else {
                true
            }
        });
    }
}
fn bounded_size(value: &Value, limit: usize) -> Option<usize> {
    struct Counter {
        bytes: usize,
        limit: usize,
    }
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.bytes = self.bytes.saturating_add(bytes.len());
            if self.bytes > self.limit {
                return Err(io::Error::other("current value capacity"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut count = Counter { bytes: 0, limit };
    serde_json::to_writer(&mut count, value).ok()?;
    Some(count.bytes)
}
fn portable(detail: &Value, v: &Value) -> Value {
    let field = v["field"]
        .as_u64()
        .and_then(|n| detail["descriptor"]["fields"].get(n as usize));
    let globals = field.map(|f| &f["globals"]).unwrap_or(&Value::Null);
    let mut row = json!({"fieldId":format!("descriptor:{}",v["field"]),"elementIndex":v["element"],
            "semantics":v["semantics"].as_str().unwrap_or("Opaque").to_lowercase(),
            "rawBits":{"state":"observed","source":"report-descriptor","value":v["raw_bits"]},
            "sample":sample(&v["sample"]),"logical":{"minimum":globals["logical_min"],"maximum":globals["logical_max"]},
            "physicalBounds":{"minimum":globals["physical_min"],"maximum":globals["physical_max"]},
            "usages":v["usages"]});
    if !v["physical"].is_null() {
        row["physical"] = v["physical"].clone();
    }
    row
}
fn element(detail: &Value, v: &Value) -> Value {
    let metadata = detail["elements"]
        .as_array()
        .and_then(|a| a.iter().find(|e| e["cookie"] == v["cookie"]));
    let bound = |key: &str| {
        v.get(key)
            .or_else(|| metadata.and_then(|m| m.get(key)))
            .cloned()
            .unwrap_or(Value::Null)
    };
    json!({"fieldId":format!("iohid-cookie:{}",v["cookie"]),"elementIndex":0,
            "semantics":match v["relative"].as_bool() {Some(true)=>"relative",Some(false)=>"absolute",None=>"opaque"},
            "rawBits":unavailable("element-only"),"elementValueHex":v["valueHex"],
            "sample":if v["integer"].is_string() {json!({"state":"valid","kind":"integer","value":v["integer"]})}
                else {json!({"state":"unsupported","reason":"IOHID supplied no exact scalar integer","encoded":null})},
            "logical":{"minimum":bound("logicalMin"),"maximum":bound("logicalMax")},
            "physicalBounds":{"minimum":bound("physicalMin"),"maximum":bound("physicalMax")},
            "usagePage":v["usagePage"],"usage":v["usage"]})
}
fn windows<'a>(detail: &'a Value, projection: &'a Value) -> impl Iterator<Item = Value> + 'a {
    let report = detail["capabilities"]["reports"]
        .as_array()
        .and_then(|a| a.iter().find(|r| r["kind"] == projection["kind"]));
    let data = projection["data"].as_array().into_iter().flatten().flat_map(move |data| {
            let values = data["values"].as_array().into_iter().flatten().filter_map(move |v| {
                let index = v["capabilityIndex"].as_u64().unwrap_or(u64::MAX) as usize;
                let caps = report
                    .and_then(|r| r["valueCaps"].get(index))
                    .unwrap_or(&Value::Null);
                let semantics = if v["semantics"] == "array-selector" {
                    if caps["isAbsolute"] == true {
                        "absolute"
                    } else {
                        "opaque"
                    }
                } else {
                    v["semantics"].as_str().unwrap_or("opaque")
                };
                let quality = v["quality"].as_str().unwrap_or("unknown");
                if semantics != "absolute" { return None; }
                Some(json!({"fieldId":format!("hidp-value:{}:data:{}",index,data["dataIndex"]),
                    "elementIndex":0,"semantics":semantics,"rawBits":unavailable("os-projected"),
                    "encoded":data["rawValue"],"dataIndex":data["dataIndex"],
                    "sample":if quality == "valid" {json!({"state":"valid","kind":if v["semantics"] == "array-selector" {"array-selector"} else {"integer"},"value":v["native"]})}
                        else {json!({"state":if quality == "null" || quality == "invalid" {quality} else {"unsupported"},"reason":quality,"encoded":v["native"]})},
                    "logical":{"minimum":caps["logicalMin"],"maximum":caps["logicalMax"]},
                    "physicalBounds":{"minimum":caps["physicalMin"],"maximum":caps["physicalMax"]},
                    "bitSize":caps["bitSize"],"unitCode":caps["units"],"unitExponent":caps["unitExponent"]}))
            });
            let buttons = data["matchingButtonCaps"].as_array().into_iter().flatten().filter_map(move |index| {
                let caps = index
                    .as_u64()
                    .and_then(|i| report.and_then(|r| r["buttonCaps"].get(i as usize)))
                    .unwrap_or(&Value::Null);
                if caps["isAbsolute"] != true { return None; }
                Some(json!({"fieldId":format!("hidp-button:{}:data:{}",index,data["dataIndex"]),"elementIndex":0,
                    "semantics":if caps["isAbsolute"] == true {"absolute"} else {"opaque"},
                    "rawBits":unavailable("os-projected"),"sample":{"state":"valid","kind":"integer","value":data["rawValue"]},
                    "logical":{"minimum":null,"maximum":null},"physicalBounds":{"minimum":null,"maximum":null}}))
            });
            values.chain(buttons)
        });
    let arrays = projection["valueArrays"].as_array().into_iter().flatten().filter_map(move |array| {
            let index = array["capabilityIndex"].as_u64().unwrap_or(u64::MAX) as usize;
            let caps = report
                .and_then(|r| r["valueCaps"].get(index))
                .unwrap_or(&Value::Null);
            if caps["isAbsolute"] != true { return None; }
            Some(json!({"fieldId":format!("hidp-array:{}",index),"elementIndex":0,
                "semantics":if caps["isAbsolute"] == true {"absolute"} else {"opaque"},
                "rawBits":unavailable("os-projected"),"osProjectionHex":array["bytes"],
                "sample":{"state":"unsupported","reason":"Packed HidP usage value array retained without inferred selectors"},
                "logical":{"minimum":caps["logicalMin"],"maximum":caps["logicalMax"]},
                "physicalBounds":{"minimum":caps["physicalMin"],"maximum":caps["physicalMax"]},
                "bitSize":array["bitSize"],"reportCount":array["reportCount"]}))
        });
    data.chain(arrays)
}
fn unavailable(reason: &str) -> Value {
    json!({"state":"unavailable","reason":reason})
}
fn sample(v: &Value) -> Value {
    for (variant, state) in [
        ("Integer", "valid"),
        ("Null", "null"),
        ("Invalid", "invalid"),
    ] {
        if let Some(encoded) = v.get(variant) {
            return if state == "valid" {
                json!({"state":state,"kind":"integer","value":encoded})
            } else {
                json!({"state":state,"encoded":encoded})
            };
        }
    }
    if let Some(value) = v.get("Array") {
        return json!({"state":"valid","kind":"array-selector","selector":value["selector"],"usages":value["usages"]});
    }
    if let Some(value) = v.get("Bytes") {
        return json!({"state":"valid","kind":"bytes","bytes":value});
    }
    if let Some(value) = v.get("Unknown") {
        return json!({"state":"unknown","reason":value["reason"],"encoded":value["encoded"]});
    }
    json!({"state":"unsupported","reason":"Unrecognized native sample"})
}
