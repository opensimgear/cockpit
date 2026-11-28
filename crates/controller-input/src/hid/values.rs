//! Exact, generation-local absolute observations. This layer never scales or integrates.
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Current {
    detail: Value,
    values: BTreeMap<String, (usize, Value)>,
    bytes: usize,
    received: Option<String>,
    status: Option<&'static str>,
    diagnostics: Value,
}
impl Current {
    pub fn new(detail: Value) -> Self {
        Self {
            detail,
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
        let rows = if let Some(values) = projection["values"].as_array() {
            values.iter().map(|v| self.portable(v)).collect::<Vec<_>>()
        } else if let Some(elements) = projection["elements"].as_array() {
            elements.iter().map(|v| self.element(v)).collect()
        } else if projection["data"].is_array() {
            self.windows(projection)
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
        for mut row in rows {
            if row["semantics"] != "absolute" {
                continue;
            }
            row["reportId"] = report.clone();
            row["sequence"] = json!(seq.to_string());
            row["receivedUs"] = json!(micros.to_string());
            let key = format!("{}:{}:{}", report, row["fieldId"], row["elementIndex"]);
            let size = row.to_string().len();
            if let Some((old_size, _)) = self.values.remove(&key) {
                self.bytes -= old_size;
            }
            if self.values.len() >= 16_384 || self.bytes.saturating_add(size) > 8 * 1024 * 1024 {
                self.clear();
                self.status = Some("stale");
                self.diagnostics = json!([{"kind":"capacity","reason":"Current exact value budget exceeded; baseline invalidated"}]);
                return;
            }
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
    fn portable(&self, v: &Value) -> Value {
        let field = v["field"]
            .as_u64()
            .and_then(|n| self.detail["descriptor"]["fields"].get(n as usize));
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
    fn element(&self, v: &Value) -> Value {
        let metadata = self.detail["elements"]
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
    fn windows(&self, projection: &Value) -> Vec<Value> {
        let mut rows = Vec::new();
        let report = self.detail["capabilities"]["reports"]
            .as_array()
            .and_then(|a| a.iter().find(|r| r["kind"] == projection["kind"]));
        for data in projection["data"].as_array().into_iter().flatten() {
            for v in data["values"].as_array().into_iter().flatten() {
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
                rows.push(json!({"fieldId":format!("hidp-value:{}:data:{}",index,data["dataIndex"]),
                    "elementIndex":0,"semantics":semantics,"rawBits":unavailable("os-projected"),
                    "encoded":data["rawValue"],"dataIndex":data["dataIndex"],
                    "sample":if quality == "valid" {json!({"state":"valid","kind":if v["semantics"] == "array-selector" {"array-selector"} else {"integer"},"value":v["native"]})}
                        else {json!({"state":if quality == "null" || quality == "invalid" {quality} else {"unsupported"},"reason":quality,"encoded":v["native"]})},
                    "logical":{"minimum":caps["logicalMin"],"maximum":caps["logicalMax"]},
                    "physicalBounds":{"minimum":caps["physicalMin"],"maximum":caps["physicalMax"]},
                    "bitSize":caps["bitSize"],"unitCode":caps["units"],"unitExponent":caps["unitExponent"]}));
            }
            for index in data["matchingButtonCaps"].as_array().into_iter().flatten() {
                let caps = index
                    .as_u64()
                    .and_then(|i| report.and_then(|r| r["buttonCaps"].get(i as usize)))
                    .unwrap_or(&Value::Null);
                rows.push(json!({"fieldId":format!("hidp-button:{}:data:{}",index,data["dataIndex"]),"elementIndex":0,
                    "semantics":if caps["isAbsolute"] == true {"absolute"} else {"opaque"},
                    "rawBits":unavailable("os-projected"),"sample":{"state":"valid","kind":"integer","value":data["rawValue"]},
                    "logical":{"minimum":null,"maximum":null},"physicalBounds":{"minimum":null,"maximum":null}}));
            }
        }
        for array in projection["valueArrays"].as_array().into_iter().flatten() {
            let index = array["capabilityIndex"].as_u64().unwrap_or(u64::MAX) as usize;
            let caps = report
                .and_then(|r| r["valueCaps"].get(index))
                .unwrap_or(&Value::Null);
            rows.push(json!({"fieldId":format!("hidp-array:{}",index),"elementIndex":0,
                "semantics":if caps["isAbsolute"] == true {"absolute"} else {"opaque"},
                "rawBits":unavailable("os-projected"),"osProjectionHex":array["bytes"],
                "sample":{"state":"unsupported","reason":"Packed HidP usage value array retained without inferred selectors"},
                "logical":{"minimum":caps["logicalMin"],"maximum":caps["logicalMax"]},
                "physicalBounds":{"minimum":caps["physicalMin"],"maximum":caps["physicalMax"]},
                "bitSize":array["bitSize"],"reportCount":array["reportCount"]}));
        }
        rows
    }
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
