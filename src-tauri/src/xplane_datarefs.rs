//! Numeric RREF source metadata from the selected simulator's own SDK catalogue.
//! The writable column is deliberately ignored: this transport only reads.
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs, io::Read, path::Path};

const MAX_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ARRAY: usize = 8192;
const MAX_CHANNELS: usize = 65_536;
const SOURCE: &str = "https://developer.x-plane.com/sdk/XPLMDataAccess/";

#[derive(Clone, Debug)]
pub(super) struct Channel {
    pub id: String,
    pub dataref: String,
    pub label: String,
    pub unit: String,
    pub kind: String,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub integer: bool,
    pub metadata: Value,
}

impl Channel {
    pub fn value(&self) -> Value {
        let mut value = self.metadata.clone();
        value["id"] = json!(self.id);
        value["label"] = json!(self.label);
        value["dataref"] = json!(self.dataref);
        value["type"] = json!(self.kind);
        value["unit"] = json!(self.unit);
        if let Some(n) = self.minimum { value["minimum"] = json!(n); }
        if let Some(n) = self.maximum { value["maximum"] = json!(n); }
        value
    }
}

pub(super) fn load(executable: &Path) -> Result<Vec<Channel>, String> {
    crate::xplane_installation::version(executable)?;
    let path = executable.parent().ok_or("Choose an X-Plane executable.")?
        .join("Resources/plugins/DataRefs.txt");
    let mut content = String::new();
    fs::File::open(path).map_err(|e| e.to_string())?.take(MAX_BYTES + 1)
        .read_to_string(&mut content).map_err(|e| e.to_string())?;
    if content.len() as u64 > MAX_BYTES { return Err("X-Plane dataref catalogue exceeds 4 MiB.".into()); }
    parse(&content)
}

fn parse(content: &str) -> Result<Vec<Channel>, String> {
    let mut result = Vec::new();
    let mut names = BTreeSet::new();
    for line in content.lines().skip(1).filter(|line| line.starts_with("sim/")) {
        let columns: Vec<_> = line.split('\t').collect();
        // Shipped SDK catalogues are tab-separated; spaces inside units and
        // descriptions must not be mistaken for new columns.
        if columns.len() < 3 { continue; }
        let name = columns[0].trim();
        let native_type = columns[1].trim();
        if name.len() > 350 || !name.is_ascii() || name.bytes().any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
            || name.contains(['[', ']']) || !names.insert(name.to_owned()) { continue; }
        let base = native_type.split('[').next().unwrap_or("");
        if !matches!(base, "int" | "float" | "double") { continue; }
        let mut shape = Vec::new();
        let mut tail = &native_type[base.len()..];
        let mut malformed = false;
        while !tail.is_empty() {
            let Some(end) = tail.strip_prefix('[').and_then(|s| s.find(']').map(|i| i + 1)) else { malformed = true; break; };
            let Some(n) = tail[1..end].parse::<usize>().ok().filter(|n| *n > 0) else { malformed = true; break; };
            shape.push(n);
            tail = &tail[end + 1..];
            if shape.len() > 8 { malformed = true; break; }
        }
        let count = shape.iter().try_fold(1usize, |a, b| a.checked_mul(*b));
        let count = count.filter(|n| !malformed && *n <= MAX_ARRAY);
        let native_unit = columns.get(3).copied().unwrap_or("").trim();
        let unit = if native_unit.len() <= 32 && !native_unit.chars().any(char::is_control) { native_unit } else { "" };
        let description = columns.get(4).copied().unwrap_or("").trim();
        let boolean = matches!(unit.to_ascii_lowercase().as_str(), "bool" | "boolean");
        let group = name.rsplit_once('/').map(|(group, _)| group).unwrap_or("sim");
        let supported = count.is_some();
        let count = count.unwrap_or(1);
        if result.len().saturating_add(count) > MAX_CHANNELS - 15 { return Err("X-Plane numeric catalogue exceeds 65,536 bounded components.".into()); }
        for component in 0..count {
            let dataref = if shape.is_empty() || !supported { name.to_owned() } else { format!("{name}[{component}]") };
            let category = if name.contains("/engine/") || name.contains("/engines/") { "engine" }
                else if name.starts_with("sim/flightmodel") { "physics" }
                else if name.contains("/controls/") || name.starts_with("sim/joystick/") { "controls" }
                else if name.contains("/gauges/") || name.contains("/indicators/") || name.starts_with("sim/cockpit") { "instruments" }
                else if name.starts_with("sim/weather/") || name.starts_with("sim/graphics/scenery/") { "environment" }
                else if name.starts_with("sim/aircraft/") { "aircraft" } else { "system" };
            let mut metadata = json!({"nativeField":dataref,"group":group,"category":category,"nativeType":native_type,"shape":shape,"supported":supported,"readOnly":true,"source":SOURCE});
            if unit != native_unit { metadata["nativeUnit"] = json!(native_unit); }
            if !description.is_empty() { metadata["description"] = json!(description); }
            if !shape.is_empty() && supported { metadata["component"] = json!(component); }
            if !supported { metadata["reason"] = json!("Array length is unspecified or exceeds the bounded 8192-component catalogue limit; no RREF subscription is made."); }
            result.push(Channel { id:dataref.clone(), label:dataref.clone(), dataref, unit:unit.into(), kind:if boolean {"boolean"} else {"number"}.into(), minimum:Some(if boolean { 0. } else { -(f32::MAX as f64) }), maximum:Some(if boolean { 1. } else { f32::MAX as f64 }), integer:base == "int", metadata });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_types_units_and_multidimensional_components_are_preserved() {
        let channels = parse("2 1210\nsim/a/scalar\tdouble\tn\tmeters\tAltitude\nsim/a/array\tfloat[2][3]\ty\tNm\tMoment\nsim/a/flag\tint\tn\tboolean\tOn\nsim/a/text\tbyte[40]\ty\tstring\tText\n").unwrap();
        assert_eq!(channels.len(), 8);
        assert_eq!(channels[0].value()["nativeType"], "double");
        assert_eq!(channels[0].unit, "meters");
        assert_eq!(channels[6].dataref, "sim/a/array[5]");
        assert_eq!(channels[6].value()["shape"], json!([2,3]));
        assert_eq!(channels[7].kind, "boolean");
        assert!(channels.iter().all(|c| c.value()["readOnly"] == true));
    }
    #[test]
    fn oversized_or_unsized_numeric_arrays_remain_explicitly_unavailable() {
        let channels = parse("2 1210\nsim/a/huge\tfloat[56][2][2][721]\tn\t\nsim/a/unsized\tfloat[]\tn\t\n").unwrap();
        assert_eq!(channels.len(), 2);
        assert!(channels.iter().all(|c| c.value()["supported"] == false));
    }
}
