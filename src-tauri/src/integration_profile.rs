//! Portable named simulator bindings and bounded formatting. No device, file or
//! network I/O occurs here; the integration service owns validation snapshots.
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashSet};

const INVALID: &str = "Invalid simulator integration profile.";
pub(crate) const MAX_PROFILE_BYTES: usize = 65536;

fn text(value: &Value, max: usize, empty: bool) -> Option<&str> {
    value
        .as_str()
        .filter(|s| (empty || !s.is_empty()) && s.len() <= max && !s.chars().any(char::is_control))
}
fn id(value: &Value, max: usize) -> Option<&str> {
    text(value, max, false).filter(|s| {
        s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    })
}
fn known(value: &Value, keys: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|v| v.keys().all(|k| keys.contains(&k.as_str())))
}

fn uint(value: &Value, default: u64, max: u64) -> Result<u64, String> {
    if value.is_null() {
        return Ok(default);
    }
    value.as_u64().filter(|n| *n <= max).ok_or(INVALID.into())
}
fn finite(value: &Value, default: f64, max: f64) -> Result<f64, String> {
    if value.is_null() {
        return Ok(default);
    }
    value
        .as_f64()
        .filter(|n| n.is_finite() && n.abs() <= max)
        .ok_or(INVALID.into())
}
fn color(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|s| s.len() == 6 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_ascii_uppercase)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Assignment {
    pub role: String,
    pub gateway_id: String,
    pub module_id: String,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Config {
    pub document: Value,
    pub connection: Value,
    pub assignments: Vec<Assignment>,
}
impl Config {
    pub fn provider(&self) -> &str {
        self.document["provider"].as_str().unwrap()
    }
    pub fn value(&self) -> Value {
        json!({"document":self.document,"connection":self.connection,"assignments":self.assignments.iter().map(|a|json!({"role":a.role,"gatewayId":a.gateway_id,"moduleId":a.module_id})).collect::<Vec<_>>()})
    }
    /// Resolve explicit local assignments to the existing DCS engine. Importing
    /// or parsing does not call this method and cannot silently rebind a role.
    pub fn resolved_dcs_profile(&self) -> Result<Value, String> {
        if self.provider() != "dcs-bios" {
            return Err("This provider has no DCS command transport.".into());
        }
        let mut profile = self.connection.clone();
        profile["schemaVersion"] = json!(2);
        profile["aircraftModule"] = self.document["context"]["aircraftModule"].clone();
        for key in ["inputs", "outputs"] {
            let mut rows = Vec::new();
            for binding in self.document[key].as_array().unwrap() {
                let role = binding["role"].as_str().unwrap();
                let target = self
                    .assignments
                    .iter()
                    .find(|a| a.role == role)
                    .ok_or_else(|| format!("Assign device role {role} before starting."))?;
                let mut row = binding.clone();
                row.as_object_mut().unwrap().remove("role");
                row["gatewayId"] = json!(target.gateway_id);
                row["moduleId"] = json!(target.module_id);
                if key == "outputs" {
                    row["control"] = row["channel"].clone();
                    row.as_object_mut().unwrap().remove("channel");
                }
                rows.push(row);
            }
            profile[key] = json!(rows);
        }
        Ok(profile)
    }
}

fn validate_format(format: &Value) -> Result<(), String> {
    if !known(
        format,
        &[
            "kind",
            "scale",
            "decimals",
            "prefix",
            "suffix",
            "align",
            "overflow",
            "unknownText",
            "staleText",
            "threshold",
            "comparison",
            "bands",
            "unknownColor",
            "staleColor",
        ],
    ) {
        return Err(INVALID.into());
    }
    let kind = format["kind"].as_str().ok_or(INVALID)?;
    if !matches!(
        kind,
        "boolean" | "text" | "integer" | "fixed" | "threshold" | "rgb"
    ) {
        return Err(INVALID.into());
    }
    finite(&format["scale"], 1.0, 1e6)?;
    uint(&format["decimals"], 0, 6)?;
    for (key, max) in [
        ("prefix", 16),
        ("suffix", 16),
        ("unknownText", 63),
        ("staleText", 63),
    ] {
        if format
            .get(key)
            .is_some_and(|v| text(v, max, true).is_none())
        {
            return Err(INVALID.into());
        }
    }
    if format
        .get("align")
        .is_some_and(|v| !matches!(v.as_str(), Some("left" | "right")))
        || format
            .get("overflow")
            .is_some_and(|v| !matches!(v.as_str(), Some("truncate" | "reject")))
    {
        return Err(INVALID.into());
    }
    if kind == "threshold"
        || format.get("threshold").is_some()
        || format.get("comparison").is_some()
    {
        if !format["threshold"]
            .as_f64()
            .is_some_and(|n| n.is_finite() && n.abs() <= 1e15)
            || format
                .get("comparison")
                .is_some_and(|v| !matches!(v.as_str(), Some("gte" | "lte")))
        {
            return Err(INVALID.into());
        }
    }
    if kind == "rgb" || format.get("bands").is_some() {
        let bands = format["bands"]
            .as_array()
            .filter(|b| !b.is_empty() && b.len() <= 8)
            .ok_or(INVALID)?;
        let mut previous = f64::NEG_INFINITY;
        for band in bands {
            if !known(band, &["minimum", "color"]) {
                return Err(INVALID.into());
            }
            let minimum = band["minimum"]
                .as_f64()
                .filter(|n| n.is_finite() && n.abs() <= 1e15 && *n > previous)
                .ok_or(INVALID)?;
            color(&band["color"]).ok_or(INVALID)?;
            previous = minimum;
        }
    }
    for key in ["unknownColor", "staleColor"] {
        if format.get(key).is_some_and(|v| color(v).is_none()) {
            return Err(INVALID.into());
        }
    }
    Ok(())
}

pub(crate) fn feedback_outputs(document: &Value) -> Result<Vec<Value>, String> {
    let Some(value) = document.get("feedbackOutputs") else {
        return Ok(vec![]);
    };
    let rows = value.as_array().filter(|a| a.len() <= 16).ok_or(INVALID)?;
    let mut ids = HashSet::new();
    for row in rows {
        if !known(row, &["id", "channel", "output"])
            || !ids.insert(id(&row["id"], 31).ok_or(INVALID)?)
            || id(&row["channel"], 64).is_none()
        {
            return Err(INVALID.into());
        }
        uint(&row["output"], 0, 15)?;
    }
    Ok(rows.clone())
}

/// Pure document/local-settings validation. Missing assignments are preserved
/// for import previews; resolution and runtime validation require them.
pub(crate) fn validate(value: &Value) -> Result<Config, String> {
    if !value.is_object() || value.to_string().len() > MAX_PROFILE_BYTES {
        return Err(INVALID.into());
    }
    let document = &value["document"];
    if !document.is_object()
        || !matches!(document["schemaVersion"].as_u64(), Some(1 | 2))
        || id(&document["id"], 31).is_none()
        || text(&document["name"], 63, false).is_none()
        || !matches!(
            document["provider"].as_str(),
            Some("dcs-bios" | "assetto-corsa-rally" | "x-plane")
        )
    {
        return Err(INVALID.into());
    }
    // Local addresses/paths/targets cannot leak through a portable export.
    if document.as_object().unwrap().keys().any(|key| {
        !matches!(
            key.as_str(),
            "schemaVersion"
                | "id"
                | "name"
                | "provider"
                | "context"
                | "roles"
                | "inputs"
                | "outputs"
                | "feedbackOutputs"
        )
    }) {
        return Err(INVALID.into());
    }
    let provider = document["provider"].as_str().unwrap();
    if !known(&document["context"], &["aircraftModule"])
        || provider == "dcs-bios"
            && text(&document["context"]["aircraftModule"], 64, false).is_none()
    {
        return Err(INVALID.into());
    }
    if document["context"]
        .get("aircraftModule")
        .is_some_and(|v| text(v, 64, false).is_none())
    {
        return Err(INVALID.into());
    }
    let roles = document["roles"]
        .as_array()
        .filter(|v| v.len() <= 8)
        .ok_or(INVALID)?;
    let mut role_ids = HashSet::new();
    for role in roles {
        if !known(role, &["id", "label"]) {
            return Err(INVALID.into());
        }
        if !role_ids.insert(id(&role["id"], 31).ok_or(INVALID)?.to_owned())
            || text(&role["label"], 63, false).is_none()
        {
            return Err(INVALID.into());
        }
    }
    let inputs = document["inputs"]
        .as_array()
        .filter(|v| v.len() <= 16)
        .ok_or(INVALID)?;
    let outputs = document["outputs"]
        .as_array()
        .filter(|v| v.len() <= 16)
        .ok_or(INVALID)?;
    let feedback = feedback_outputs(document)?;
    if document["schemaVersion"] == 1 && document.get("feedbackOutputs").is_some() {
        return Err(
            "Migrate the integration document to schema 2 for companion telemetry sources.".into(),
        );
    }
    if outputs.len() + feedback.len() > 16
        || inputs.is_empty() && outputs.is_empty() && feedback.is_empty()
        || provider != "dcs-bios" && !inputs.is_empty()
    {
        return Err(INVALID.into());
    }
    let mut ids = HashSet::new();
    let mut sources = HashSet::new();
    let mut commands = HashSet::new();
    let mut endpoints = HashSet::new();
    for binding in inputs.iter().chain(outputs) {
        if !ids.insert(id(&binding["id"], 31).ok_or(INVALID)?)
            || !role_ids.contains(id(&binding["role"], 31).ok_or(INVALID)?)
        {
            return Err(INVALID.into());
        }
    }
    for binding in &feedback {
        if !ids.insert(id(&binding["id"], 31).ok_or(INVALID)?) {
            return Err(INVALID.into());
        }
    }
    for binding in inputs {
        if !known(
            binding,
            &[
                "id",
                "role",
                "kind",
                "control",
                "command",
                "interface",
                "pressedValue",
                "releasedValue",
                "step",
                "deadband",
            ],
        ) {
            return Err(INVALID.into());
        }
        let kind = binding["kind"].as_str().ok_or(INVALID)?;
        let control = binding["control"]
            .as_u64()
            .filter(|n| {
                *n <= if matches!(kind, "axis" | "encoder") {
                    7
                } else {
                    127
                }
            })
            .ok_or(INVALID)?;
        let interface = binding["interface"].as_str().ok_or(INVALID)?;
        if !matches!(
            (kind, interface),
            ("encoder", "fixed_step" | "variable_step")
                | ("button" | "switch" | "axis", "set_state")
        ) || !sources.insert((binding["role"].as_str().unwrap(), kind, control))
            || !commands.insert(id(&binding["command"], 64).ok_or(INVALID)?)
        {
            return Err(INVALID.into());
        }
        for (key, default) in [("pressedValue", 1), ("releasedValue", 0), ("deadband", 128)] {
            uint(&binding[key], default, 65535)?;
        }
        if uint(&binding["step"], 1, 65535)? == 0 {
            return Err(INVALID.into());
        }
    }
    for binding in outputs {
        if !known(
            binding,
            &[
                "id", "role", "channel", "output", "endpoint", "actionId", "format",
            ],
        ) {
            return Err(INVALID.into());
        }
        id(&binding["channel"], 64).ok_or(INVALID)?;
        id(&binding["actionId"], 31).ok_or(INVALID)?;
        let endpoint = id(&binding["endpoint"], 31).ok_or(INVALID)?;
        if !endpoints.insert((binding["role"].as_str().unwrap(), endpoint)) {
            return Err(INVALID.into());
        }
        uint(&binding["output"], 0, 63)?;
        validate_format(&binding["format"])?;
    }
    let connection = &value["connection"];
    if !connection.is_object() {
        return Err(INVALID.into());
    }
    if provider == "dcs-bios" {
        if text(&connection["cataloguePath"], 4096, false).is_none() {
            return Err("Choose the installed DCS-BIOS catalogue directory.".into());
        }
        for key in ["commandHost", "listenAddress", "multicastInterface"] {
            if text(&connection[key], 253, false).is_none_or(|s| s.chars().any(char::is_whitespace))
            {
                return Err(INVALID.into());
            }
        }
        if text(&connection["multicastGroup"], 253, true)
            .is_none_or(|s| s.chars().any(char::is_whitespace))
        {
            return Err(INVALID.into());
        }
        for key in ["commandPort", "listenPort"] {
            if connection[key]
                .as_u64()
                .is_none_or(|n| !(1..=65535).contains(&n))
            {
                return Err(INVALID.into());
            }
        }
    } else if provider == crate::xplane::PROVIDER {
        crate::xplane::validate_connection(connection)?;
    } else {
        match connection["source"].as_str() {
            Some("shared-memory")
                if connection["processId"].is_null()
                    || connection["processId"]
                        .as_u64()
                        .is_some_and(|n| n > 0 && n <= u32::MAX as u64) =>
            {
                ()
            }
            Some("synthetic") => (),
            _ => return Err("Choose a supported ACR telemetry source and process.".into()),
        }
    }
    if connection["freshnessMs"]
        .as_u64()
        .is_none_or(|n| !(100..=10000).contains(&n))
    {
        return Err(INVALID.into());
    }
    let rows = value["assignments"]
        .as_array()
        .filter(|a| a.len() <= 8)
        .ok_or(INVALID)?;
    let mut assigned = HashSet::new();
    let mut gateways = BTreeSet::new();
    let mut assignments = Vec::new();
    for row in rows {
        let role = id(&row["role"], 31).ok_or(INVALID)?;
        let gateway = text(&row["gatewayId"], 78, false)
            .filter(|s| {
                s.contains(':')
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b":._-".contains(&b))
            })
            .ok_or(INVALID)?;
        let gateway = gateway
            .split_once(':')
            .and_then(|(backend, serial)| crate::gateways::canonical_id(backend, serial))
            .ok_or(INVALID)?;
        let module = text(&row["moduleId"], 32, false)
            .filter(|s| {
                s.len() == 32
                    && s.bytes().all(|b| b.is_ascii_hexdigit())
                    && s.bytes().any(|b| b != b'0')
                    && s.bytes().any(|b| b != b'f' && b != b'F')
            })
            .ok_or(INVALID)?;
        if !role_ids.contains(role) || !assigned.insert(role) {
            return Err(INVALID.into());
        }
        gateways.insert(gateway.clone());
        assignments.push(Assignment {
            role: role.into(),
            gateway_id: gateway,
            module_id: module.to_ascii_lowercase(),
        });
    }
    if gateways.len() > 4 {
        return Err(INVALID.into());
    }
    // Different roles must not alias the same physical endpoint/input.
    let target = |role: &str| {
        assignments
            .iter()
            .find(|a| a.role == role)
            .map(|a| (&a.gateway_id, &a.module_id))
    };
    let mut physical = HashSet::new();
    let mut setters = HashSet::new();
    for row in inputs {
        if let Some(target) = target(row["role"].as_str().unwrap()) {
            if !physical.insert((target, row["kind"].as_str(), row["control"].as_u64())) {
                return Err("A physical input is assigned more than once.".into());
            }
        }
    }
    for row in outputs {
        if let Some(target) = target(row["role"].as_str().unwrap()) {
            if !setters.insert((target, row["endpoint"].as_str())) {
                return Err("A physical output endpoint is assigned more than once.".into());
            }
        }
    }
    Ok(Config {
        document: document.clone(),
        connection: connection.clone(),
        assignments,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Diagnostic {
    pub severity: &'static str,
    pub code: &'static str,
    pub binding_id: Option<String>,
    pub message: String,
}
impl Diagnostic {
    pub fn value(&self) -> Value {
        json!({"severity":self.severity,"code":self.code,"bindingId":self.binding_id,"message":self.message})
    }
}
fn diagnostic(code: &'static str, binding: &Value, message: &str) -> Diagnostic {
    Diagnostic {
        severity: "error",
        code,
        binding_id: binding["id"].as_str().map(str::to_owned),
        message: message.into(),
    }
}

/// Named-control diagnostics over normalized metadata. Original DCS output
/// ordinals remain identities even when unsupported outputs were filtered out.
pub(crate) fn catalogue_diagnostics(config: &Config, catalogue: &Value) -> Vec<Diagnostic> {
    let controls = catalogue["controls"]
        .as_array()
        .or_else(|| catalogue["channels"].as_array());
    let mut result = Vec::new();
    for key in ["inputs", "outputs", "feedbackOutputs"] {
        for binding in config.document[key].as_array().into_iter().flatten() {
            let name = &binding[if key == "inputs" {
                "command"
            } else {
                "channel"
            }];
            let Some(control) = controls.and_then(|rows| rows.iter().find(|c| c["id"] == *name))
            else {
                result.push(diagnostic(
                    "controlMissing",
                    binding,
                    "The named provider control is missing from the current catalogue.",
                ));
                continue;
            };
            if key == "inputs" {
                let interface = control["inputs"]
                    .as_array()
                    .or_else(|| control["interfaces"].as_array())
                    .and_then(|rows| {
                        rows.iter().find(|v| {
                            v["interface"] == binding["interface"]
                                || v["kind"] == binding["interface"]
                        })
                    });
                let Some(interface) = interface else {
                    result.push(diagnostic(
                        "interfaceChanged",
                        binding,
                        "The selected catalogue interface is no longer supported.",
                    ));
                    continue;
                };
                if binding["interface"] == "set_state"
                    && interface["maxValue"]
                        .as_u64()
                        .or_else(|| interface["maximum"].as_u64())
                        .is_some_and(|max| {
                            [
                                uint(&binding["pressedValue"], 1, 65535).unwrap(),
                                uint(&binding["releasedValue"], 0, 65535).unwrap(),
                            ]
                            .iter()
                            .any(|n| *n > max)
                        })
                {
                    result.push(diagnostic(
                        "rangeChanged",
                        binding,
                        "A configured state exceeds the current catalogue range.",
                    ));
                }
            } else {
                let index = uint(&binding["output"], 0, 63).unwrap();
                let output = control["outputs"]
                    .as_array()
                    .and_then(|rows| rows.iter().find(|v| v["index"].as_u64() == Some(index)))
                    .or_else(|| (index == 0 && control["type"].is_string()).then_some(control));
                let Some(output) = output else {
                    result.push(diagnostic(
                        "outputChanged",
                        binding,
                        "The selected original output ordinal is no longer supported.",
                    ));
                    continue;
                };
                if key == "feedbackOutputs" {
                    continue;
                }
                let kind = binding["format"]["kind"].as_str().unwrap();
                let compatible = match kind {
                    "text" => output["type"] == "string",
                    "boolean" => {
                        output["type"] == "boolean"
                            || output["type"] == "integer" && output["maxValue"] == 1
                    }
                    _ => matches!(output["type"].as_str(), Some("integer" | "number")),
                };
                if !compatible {
                    result.push(diagnostic(
                        "outputTypeChanged",
                        binding,
                        "The provider channel type no longer matches this output format.",
                    ));
                }
            }
        }
    }
    result
}

/// Return the one argument array used by action invoke. This validates the
/// actual advertised endpoint and does not infer an output from its label.
pub(crate) fn format_output(
    output: &Value,
    value: Option<&Value>,
    fresh: bool,
    action: &Value,
) -> Result<Value, String> {
    validate_format(&output["format"])?;
    crate::actions::descriptor(action)?;
    let parameters = action["parameters"].as_array().unwrap();
    if action["semantics"] != "setter"
        || action["readable"] != true
        || parameters.len() != 1
        || output["actionId"] != action["id"]
        || output["endpoint"] != action["endpoint"]
    {
        return Err("The bound endpoint requires its advertised readable setter.".into());
    }
    let parameter = &parameters[0];
    let f = &output["format"];
    let kind = f["kind"].as_str().unwrap();
    let numeric = |value: &Value| {
        value
            .as_f64()
            .filter(|n| n.is_finite() && n.abs() <= 1e15)
            .ok_or("Numeric telemetry must be finite and bounded.")
    };
    let result = if matches!(kind, "boolean" | "threshold") {
        let on = if !fresh || value.is_none() {
            false
        } else if kind == "threshold" {
            let n = numeric(value.unwrap())?;
            let threshold = f["threshold"].as_f64().unwrap();
            if f["comparison"] == "lte" {
                n <= threshold
            } else {
                n >= threshold
            }
        } else {
            value
                .unwrap()
                .as_bool()
                .or_else(|| value.unwrap().as_u64().filter(|n| *n <= 1).map(|n| n == 1))
                .ok_or("Boolean telemetry must be boolean or 0/1.")?
        };
        match parameter["type"].as_str() {
            Some("boolean") => json!([on]),
            Some("enum")
                if parameter["options"].as_array().is_some_and(|o| {
                    o.iter().any(|v| v == "on") && o.iter().any(|v| v == "off")
                }) =>
            {
                json!([if on { "on" } else { "off" }])
            }
            _ => return Err("Boolean feedback requires a boolean or on/off enum setter.".into()),
        }
    } else if kind == "rgb" {
        if parameter["type"] != "rgb" {
            return Err("RGB feedback requires an advertised RGB setter.".into());
        }
        let rgb = if !fresh {
            color(&f["staleColor"]).unwrap_or("000000".into())
        } else if let Some(value) = value {
            let n = numeric(value)?;
            f["bands"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .find(|band| n >= band["minimum"].as_f64().unwrap())
                .and_then(|b| color(&b["color"]))
                .unwrap_or("000000".into())
        } else {
            color(&f["unknownColor"]).unwrap_or("000000".into())
        };
        json!([rgb])
    } else {
        if parameter["type"] != "string" {
            return Err("Display feedback requires an advertised string setter.".into());
        }
        let optional_text = |key: &str| f[key].as_str().unwrap_or("");
        let content = if !fresh {
            optional_text("staleText").to_owned()
        } else if let Some(value) = value {
            let raw = if kind == "text" {
                value
                    .as_str()
                    .ok_or("Text feedback must be a string.")?
                    .to_owned()
            } else {
                let n = numeric(value)? * finite(&f["scale"], 1.0, 1e6)?;
                if !n.is_finite() || n.abs() > 1e15 {
                    return Err("Numeric display feedback exceeds its formatting bound.".into());
                }
                if kind == "integer" {
                    format!("{:.0}", n.round())
                } else {
                    format!("{:.*}", uint(&f["decimals"], 0, 6)? as usize, n)
                }
            };
            format!(
                "{}{}{}",
                optional_text("prefix"),
                raw,
                optional_text("suffix")
            )
        } else {
            optional_text("unknownText").to_owned()
        };
        if content.chars().any(char::is_control) {
            return Err("Display text contains control characters.".into());
        }
        let max = parameter["maxBytes"].as_u64().unwrap() as usize;
        let display = action.get("display");
        let cells = display
            .map(|d| (d["columns"].as_u64().unwrap() * d["rows"].as_u64().unwrap()) as usize);
        if display.is_some_and(|d| d["encoding"] == "ascii") && !content.is_ascii() {
            return Err("The display accepts ASCII only.".into());
        }
        let fits = |s: &str| {
            s.len() + cells.map_or(0, |c| c.saturating_sub(s.chars().count())) <= max
                && cells.is_none_or(|c| s.chars().count() <= c)
        };
        let mut formatted = content.clone();
        if !fits(&formatted) {
            if f["overflow"] == "reject" {
                return Err("Formatted text exceeds the advertised cell/byte budget.".into());
            }
            formatted.clear();
            for ch in content.chars() {
                let mut next = formatted.clone();
                next.push(ch);
                if !fits(&next) {
                    break;
                }
                formatted = next;
            }
        }
        if let Some(cells) = cells {
            let padding = " ".repeat(cells - formatted.chars().count());
            formatted = if f["align"] == "right" {
                format!("{padding}{formatted}")
            } else {
                format!("{formatted}{padding}")
            };
        }
        json!([formatted])
    };
    crate::actions::arguments(action, &result)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_portable_fields_cannot_carry_local_data() {
        for path in [
            "/document/context",
            "/document/roles/0",
            "/document/outputs/0",
            "/document/outputs/0/format",
            "/document/outputs/0/format/bands/0",
        ] {
            let mut c = fixture();
            c.pointer_mut(path).unwrap()["gatewayId"] = json!("private-target");
            assert!(validate(&c).is_err(), "{path}");
        }
        let mut c = fixture();
        c["document"]["context"]["aircraftModule"] = json!({"cataloguePath":"/private"});
        assert!(validate(&c).is_err());
        c = fixture();
        c["document"]["outputs"][0]["format"] =
            json!({"kind":"boolean","bands":[{"minimum":0,"color":"FFFFFF","private":"secret"}]});
        assert!(validate(&c).is_err());
    }
    #[test]
    fn role_aliases_and_gateway_count_remain_bounded() {
        let mut c = fixture();
        c["document"]["roles"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"other","label":"Other"}));
        let mut row = c["document"]["outputs"][0].clone();
        row["id"] = json!("other");
        row["role"] = json!("other");
        c["document"]["outputs"].as_array_mut().unwrap().push(row);
        let mut assignment = c["assignments"][0].clone();
        assignment["role"] = json!("other");
        c["assignments"].as_array_mut().unwrap().push(assignment);
        assert!(validate(&c).is_err());
        c["assignments"][1]["gatewayId"] = json!("stm32:00112233445566778899aabb");
        assert!(validate(&c).is_ok());
        c["document"]["roles"] = json!([]);
        c["document"]["outputs"] = json!([]);
        c["assignments"] = json!([]);
        for i in 0..5 {
            c["document"]["roles"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id":format!("r{i}"),"label":"Role"}));
            let mut row = fixture()["document"]["outputs"][0].clone();
            row["id"] = json!(format!("o{i}"));
            row["role"] = json!(format!("r{i}"));
            c["document"]["outputs"].as_array_mut().unwrap().push(row);
            c["assignments"].as_array_mut().unwrap().push(json!({"role":format!("r{i}"),"gatewayId":format!("stm32:{:024x}", i + 1),"moduleId":"00112233445566778899aabbccddeeff"}));
        }
        assert!(validate(&c).is_err());
    }
    #[test]
    fn canonical_gateway_identity_prevents_case_aliases_of_one_endpoint() {
        let mut c = fixture();
        let gateway = "stm32:00112233445566778899aabb";
        c["assignments"][0]["gatewayId"] = json!(gateway);
        c["document"]["roles"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"other","label":"Other role"}));
        let mut row = c["document"]["outputs"][0].clone();
        row["id"] = json!("other-rev");
        row["role"] = json!("other");
        c["document"]["outputs"].as_array_mut().unwrap().push(row);
        let mut assignment = c["assignments"][0].clone();
        assignment["role"] = json!("other");
        assignment["gatewayId"] = json!("stm32:00112233445566778899AABB");
        c["assignments"].as_array_mut().unwrap().push(assignment);
        assert!(validate(&c)
            .unwrap_err()
            .contains("physical output endpoint"));
        c["document"]["outputs"][1]["endpoint"] = json!("led.other");
        let parsed = validate(&c).unwrap();
        assert!(parsed.assignments.iter().all(|a| a.gateway_id == gateway));

        // The Pico serial is an advertised textual identity; retain its case.
        c = fixture();
        c["assignments"][0]["gatewayId"] = json!("pico:OSG-PICO-00112233AABBCCDD");
        assert_eq!(
            validate(&c).unwrap().assignments[0].gateway_id,
            "pico:OSG-PICO-00112233AABBCCDD"
        );
    }
    fn fixture() -> Value {
        json!({"document":{"schemaVersion":1,"id":"rev-lights","name":"Rev lights","provider":"assetto-corsa-rally","context":{},"roles":[{"id":"lights","label":"Lights"}],"inputs":[],"outputs":[{"id":"rev","role":"lights","endpoint":"led.rgb","actionId":"led.rgb.set_color","channel":"rev_ratio","format":{"kind":"rgb","bands":[{"minimum":0.7,"color":"00FF00"},{"minimum":0.9,"color":"FF0000"}]}}]},"connection":{"source":"synthetic","freshnessMs":1500},"assignments":[{"role":"lights","gatewayId":"pico:OSG-PICO-0011223344556677","moduleId":"00112233445566778899aabbccddeeff"}]})
    }
    fn action(kind: &str) -> Value {
        let p = match kind {
            "enum" => json!({"id":"value","label":"Value","type":"enum","options":["off","on"]}),
            "string" => json!({"id":"value","label":"Value","type":"string","maxBytes":8}),
            _ => json!({"id":"value","label":"Value","type":kind}),
        };
        json!({"id":"led.rgb.set_color","label":"Output","endpoint":"led.rgb","readable":true,"semantics":"setter","parameters":[p]})
    }
    fn output(kind: &str) -> Value {
        let mut row = fixture()["document"]["outputs"][0].clone();
        row["format"] = json!({"kind":kind});
        if kind == "threshold" {
            row["format"]["threshold"] = json!(0.8);
        }
        row
    }
    fn dcs() -> Value {
        let mut c = fixture();
        c["document"]["provider"] = json!("dcs-bios");
        c["document"]["context"] = json!({"aircraftModule":"F-16C_50"});
        c["connection"] = json!({"cataloguePath":"/installed/doc/json","commandHost":"127.0.0.1","commandPort":7778,"listenAddress":"0.0.0.0","listenPort":5010,"multicastGroup":"","multicastInterface":"127.0.0.1","freshnessMs":1500});
        c
    }
    #[test]
    fn portable_roundtrip_preserves_scope_without_copying_local_fields() {
        let c = fixture();
        let parsed = validate(&c).unwrap();
        assert_eq!(parsed.value(), c);
        assert_eq!(parsed.provider(), "assetto-corsa-rally");
        assert!(parsed.resolved_dcs_profile().is_err());
        let mut leaked = c;
        leaked["document"]["cataloguePath"] = json!("secret");
        assert!(validate(&leaked).is_err());
    }
    #[test]
    fn dcs_resolution_requires_explicit_assignment_and_preserves_named_identity() {
        let c = dcs();
        let parsed = validate(&c).unwrap();
        let p = parsed.resolved_dcs_profile().unwrap();
        assert_eq!(p["schemaVersion"], 2);
        assert_eq!(p["aircraftModule"], "F-16C_50");
        assert_eq!(p["outputs"][0]["control"], "rev_ratio");
        assert_eq!(p["outputs"][0]["moduleId"], c["assignments"][0]["moduleId"]);
        let mut preview = c;
        preview["assignments"] = json!([]);
        assert!(validate(&preview).unwrap().resolved_dcs_profile().is_err());
    }
    #[test]
    fn invalid_structure_bounds_and_duplicates_are_rejected() {
        for (path, v) in [
            ("/document/schemaVersion", json!(3)),
            ("/document/provider", json!("unknown")),
            ("/document/id", json!("bad id")),
            ("/document/name", json!("\n")),
            ("/document/roles", json!([])),
            (
                "/document/roles",
                json!(vec![json!({"id":"same","label":"X"}); 9]),
            ),
            ("/document/outputs", json!([])),
            ("/document/outputs/0/role", json!("missing")),
            ("/document/outputs/0/channel", json!("bad\n")),
            ("/document/outputs/0/actionId", json!("")),
            ("/document/outputs/0/endpoint", json!("")),
            ("/connection/source", json!("udp")),
            ("/connection/freshnessMs", json!(99)),
            ("/assignments/0/role", json!("missing")),
            ("/assignments/0/gatewayId", json!("bad")),
            ("/assignments/0/moduleId", json!("0".repeat(32))),
        ] {
            let mut c = fixture();
            *c.pointer_mut(path).unwrap_or_else(|| panic!("{path}")) = v;
            assert!(validate(&c).is_err(), "{path}");
        }
        let mut c = fixture();
        let duplicate = c["document"]["outputs"][0].clone();
        c["document"]["outputs"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        assert!(validate(&c).is_err());
        let mut c = fixture();
        c["document"]["outputs"][0]["output"] = json!(64);
        assert!(validate(&c).is_err());
        let mut c = fixture();
        c["document"]["name"] = json!("x".repeat(65536));
        assert!(validate(&c).is_err());
        let mut c = fixture();
        c["connection"] = json!({"source":"shared-memory","processId":0,"freshnessMs":1500});
        assert!(validate(&c).is_err());
        c["connection"]["processId"] = json!(1);
        assert!(validate(&c).is_ok());
    }
    #[test]
    fn inputs_are_dcs_only_typed_unique_and_bounded() {
        let input = json!({"id":"button","role":"lights","kind":"button","control":0,"command":"MASTER","interface":"set_state"});
        let mut c = fixture();
        c["document"]["inputs"] = json!([input.clone()]);
        assert!(validate(&c).is_err());
        c = dcs();
        c["document"]["inputs"] = json!([input]);
        assert!(validate(&c).is_ok());
        for (key, v) in [
            ("kind", json!("hat")),
            ("control", json!(128)),
            ("interface", json!("fixed_step")),
            ("command", json!("bad command")),
            ("pressedValue", json!(65536)),
            ("step", json!(0)),
        ] {
            let mut bad = c.clone();
            bad["document"]["inputs"][0][key] = v;
            assert!(validate(&bad).is_err(), "{key}");
        }
        c["document"]["inputs"][0]["kind"] = json!("encoder");
        c["document"]["inputs"][0]["interface"] = json!("variable_step");
        assert!(validate(&c).is_ok());
        let mut row = c["document"]["inputs"][0].clone();
        row["id"] = json!("duplicate");
        c["document"]["inputs"].as_array_mut().unwrap().push(row);
        assert!(validate(&c).is_err());
    }
    #[test]
    fn connection_and_format_options_are_bounded() {
        for (key, v) in [
            ("cataloguePath", json!("")),
            ("commandHost", json!("bad host")),
            ("commandPort", json!(0)),
            ("listenPort", json!(65536)),
            ("multicastGroup", json!(" ")),
        ] {
            let mut c = dcs();
            c["connection"][key] = v;
            assert!(validate(&c).is_err(), "{key}");
        }
        for f in [
            json!({"kind":"code"}),
            json!({"kind":"fixed","scale":1000001}),
            json!({"kind":"fixed","decimals":7}),
            json!({"kind":"text","prefix":"x".repeat(17)}),
            json!({"kind":"text","suffix":"\t"}),
            json!({"kind":"text","align":"center"}),
            json!({"kind":"text","overflow":"wrap"}),
            json!({"kind":"threshold"}),
            json!({"kind":"threshold","threshold":0,"comparison":"eq"}),
            json!({"kind":"rgb","bands":[]}),
            json!({"kind":"rgb","bands":[{"minimum":1,"color":"FFFFFF"},{"minimum":1,"color":"FF0000"}]}),
            json!({"kind":"rgb","bands":[{"minimum":1,"color":"bad"}]}),
        ] {
            let mut c = fixture();
            c["document"]["outputs"][0]["format"] = f;
            assert!(validate(&c).is_err());
        }
    }
    #[test]
    fn catalogue_reports_missing_type_interface_range_and_original_ordinal() {
        let mut c = dcs();
        c["document"]["outputs"][0]["format"] = json!({"kind":"boolean"});
        let config = validate(&c).unwrap();
        assert_eq!(
            catalogue_diagnostics(&config, &json!({"controls":[]}))[0].code,
            "controlMissing"
        );
        let meta = json!({"controls":[{"id":"rev_ratio","outputs":[{"index":2,"type":"integer","maxValue":1}]}]});
        assert_eq!(
            catalogue_diagnostics(&config, &meta)[0].code,
            "outputChanged"
        );
        let mut meta = meta;
        meta["controls"][0]["outputs"][0]["index"] = json!(0);
        assert!(catalogue_diagnostics(&config, &meta).is_empty());
        meta["controls"][0]["outputs"][0]["maxValue"] = json!(10);
        assert_eq!(
            catalogue_diagnostics(&config, &meta)[0].code,
            "outputTypeChanged"
        );
        c["document"]["inputs"] = json!([{"id":"input","role":"lights","kind":"button","control":0,"command":"BUTTON","interface":"set_state","pressedValue":2}]);
        let parsed = validate(&c).unwrap();
        meta["controls"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"BUTTON","inputs":[{"interface":"fixed_step"}]}));
        assert_eq!(
            catalogue_diagnostics(&parsed, &meta)[0].code,
            "interfaceChanged"
        );
        meta["controls"][1]["inputs"] = json!([{"interface":"set_state","maxValue":1}]);
        assert_eq!(
            catalogue_diagnostics(&parsed, &meta)[0].code,
            "rangeChanged"
        );
        assert!(catalogue_diagnostics(
            &validate(&fixture()).unwrap(),
            &json!({"channels":[{"id":"rev_ratio","type":"number"}]})
        )
        .is_empty());
    }
    #[test]
    fn booleans_thresholds_and_rgb_bands_have_safe_unknown_stale_defaults() {
        let b = output("boolean");
        assert_eq!(
            format_output(&b, Some(&json!(1)), true, &action("boolean")).unwrap(),
            json!([true])
        );
        assert_eq!(
            format_output(&b, None, false, &action("enum")).unwrap(),
            json!(["off"])
        );
        assert!(format_output(&b, Some(&json!(2)), true, &action("boolean")).is_err());
        let mut a = action("enum");
        a["parameters"][0]["options"] = json!(["off"]);
        assert!(format_output(&b, None, false, &a).is_err());
        let mut t = output("threshold");
        assert_eq!(
            format_output(&t, Some(&json!(0.8)), true, &action("boolean")).unwrap(),
            json!([true])
        );
        t["format"]["comparison"] = json!("lte");
        assert_eq!(
            format_output(&t, Some(&json!(0.9)), true, &action("boolean")).unwrap(),
            json!([false])
        );
        let mut rgb = fixture()["document"]["outputs"][0].clone();
        assert_eq!(
            format_output(&rgb, Some(&json!(0.85)), true, &action("rgb")).unwrap(),
            json!(["00FF00"])
        );
        assert_eq!(
            format_output(&rgb, Some(&json!(0.95)), true, &action("rgb")).unwrap(),
            json!(["FF0000"])
        );
        assert_eq!(
            format_output(&rgb, Some(&json!(0.1)), true, &action("rgb")).unwrap(),
            json!(["000000"])
        );
        rgb["format"]["staleColor"] = json!("abcdef");
        rgb["format"]["unknownColor"] = json!("010203");
        assert_eq!(
            format_output(&rgb, None, false, &action("rgb")).unwrap(),
            json!(["ABCDEF"])
        );
        assert_eq!(
            format_output(&rgb, None, true, &action("rgb")).unwrap(),
            json!(["010203"])
        );
        assert!(format_output(&rgb, Some(&json!("0.5")), true, &action("rgb")).is_err());
        assert!(format_output(&rgb, None, true, &action("string")).is_err());
    }
    #[test]
    fn display_formatting_fits_whole_unicode_cells_and_bytes() {
        let mut out = output("text");
        let mut a = action("string");
        a["display"] = json!({"columns":4,"rows":1,"encoding":"utf8"});
        assert_eq!(
            format_output(&out, Some(&json!("😀ABCD")), true, &a).unwrap(),
            json!(["😀ABC"])
        );
        out["format"]["align"] = json!("right");
        assert_eq!(
            format_output(&out, Some(&json!("é")), true, &a).unwrap(),
            json!(["   é"])
        );
        out["format"]["unknownText"] = json!("?");
        out["format"]["staleText"] = json!("-");
        assert_eq!(
            format_output(&out, None, true, &a).unwrap(),
            json!(["   ?"])
        );
        assert_eq!(
            format_output(&out, None, false, &a).unwrap(),
            json!(["   -"])
        );
        out["format"]["overflow"] = json!("reject");
        assert!(format_output(&out, Some(&json!("longtext")), true, &a).is_err());
        a["display"]["encoding"] = json!("ascii");
        assert!(format_output(&out, Some(&json!("é")), true, &a).is_err());
        assert!(format_output(&out, Some(&json!("\n")), true, &a).is_err());
        assert!(format_output(&out, Some(&json!(1)), true, &a).is_err());
    }
    #[test]
    fn numeric_formats_and_route_metadata_are_validated() {
        let mut out = output("fixed");
        out["format"]["scale"] = json!(0.01);
        out["format"]["decimals"] = json!(2);
        out["format"]["prefix"] = json!("$");
        assert_eq!(
            format_output(&out, Some(&json!(123)), true, &action("string")).unwrap(),
            json!(["$1.23"])
        );
        out["format"] = json!({"kind":"integer","scale":0.5});
        assert_eq!(
            format_output(&out, Some(&json!(5)), true, &action("string")).unwrap(),
            json!(["3"])
        );
        assert!(format_output(&out, Some(&json!(1e20)), true, &action("string")).is_err());
        let mut a = action("string");
        a["endpoint"] = json!("another");
        assert!(format_output(&out, None, true, &a).is_err());
        a = action("string");
        a["readable"] = json!(false);
        assert!(format_output(&out, None, true, &a).is_err());
        assert!(format_output(&out, None, true, &action("boolean")).is_err());
    }
}
