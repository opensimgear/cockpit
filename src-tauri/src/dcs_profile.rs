//! Validated simulator profiles, catalogue-driven formatting and atomic exports.
//! The runtime owns panel routing and command queues; this module has no I/O loop.
use crate::dcs_bios::{self, Catalogue, ExportEvent, IntegerOutput, Memory, Network, Parser};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

const INVALID: &str = "Invalid DCS-BIOS panel profile.";
const CATALOGUE_ERROR: &str = "The installed DCS-BIOS catalogue is incompatible or malformed.";
fn text(value: &Value, max: usize, empty: bool) -> Option<&str> {
    value
        .as_str()
        .filter(|s| (empty || !s.is_empty()) && s.len() <= max && !s.chars().any(char::is_control))
}
fn identifier(value: &str, max: usize, punctuation: bool) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || (punctuation && b".-".contains(&b)))
}
fn number(value: &Value, key: &str, default: u64, max: u64) -> Result<u16, String> {
    value
        .get(key)
        .map_or(Some(default), Value::as_u64)
        .filter(|n| *n <= max)
        .map(|n| n as u16)
        .ok_or(INVALID.into())
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Target {
    pub gateway_id: String,
    pub module_id: String,
}
impl Target {
    pub(crate) fn parse(value: &Value) -> Result<Self, String> {
        let gateway = text(&value["gatewayId"], 78, false).ok_or(INVALID)?;
        let module = text(&value["moduleId"], 32, false).ok_or(INVALID)?;
        let valid = |s: &str, length| {
            s.len() == length
                && s.bytes().all(|b| b.is_ascii_hexdigit())
                && s.bytes().any(|b| b != b'0')
                && s.bytes().any(|b| b != b'f' && b != b'F')
        };
        let gateway = gateway
            .split_once(':')
            .and_then(|(backend, serial)| {
                let supported = backend == "stm32"
                    || backend == "pico"
                        && serial.strip_prefix("OSG-PICO-").is_some_and(|id| {
                            id.len() == 16
                                && id.bytes().all(|b| b.is_ascii_hexdigit())
                                && id.bytes().any(|b| b != b'0')
                        });
                supported
                    .then(|| crate::gateways::canonical_id(backend, serial))
                    .flatten()
            })
            .ok_or(INVALID)?;
        if !valid(module, 32) {
            return Err(INVALID.into());
        }
        Ok(Self {
            gateway_id: gateway,
            module_id: module.to_ascii_lowercase(),
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum InputKind {
    Button,
    Switch,
    Encoder,
    Axis,
}
impl InputKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Button => "button",
            Self::Switch => "switch",
            Self::Encoder => "encoder",
            Self::Axis => "axis",
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Input {
    pub id: String,
    pub target: Target,
    pub kind: InputKind,
    pub control: usize,
    pub command: String,
    pub interface: String,
    pub pressed_value: u16,
    pub released_value: u16,
    pub step: u16,
    pub deadband: u16,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Format {
    pub kind: String,
    pub scale: f64,
    pub decimals: u8,
    pub prefix: String,
    pub suffix: String,
    pub align: String,
    pub overflow: String,
    pub unknown_text: String,
    pub stale_text: String,
}
impl Format {
    fn parse(value: &Value) -> Result<Self, String> {
        if !value.is_object() {
            return Err(INVALID.into());
        }
        let kind = text(&value["kind"], 16, false).ok_or(INVALID)?;
        if !matches!(kind, "boolean" | "text" | "integer" | "fixed") {
            return Err(INVALID.into());
        }
        let scale = value
            .get("scale")
            .map_or(Some(1.0), Value::as_f64)
            .filter(|v| v.is_finite() && v.abs() <= 1e6)
            .ok_or(INVALID)?;
        let align = value
            .get("align")
            .map_or(Some("left"), Value::as_str)
            .filter(|v| matches!(*v, "left" | "right"))
            .ok_or(INVALID)?;
        let overflow = value
            .get("overflow")
            .map_or(Some("truncate"), Value::as_str)
            .filter(|v| matches!(*v, "truncate" | "reject"))
            .ok_or(INVALID)?;
        let field = |key, max| {
            value
                .get(key)
                .map_or(Some(""), |v| text(v, max, true))
                .map(str::to_owned)
                .ok_or_else(|| INVALID.to_owned())
        };
        Ok(Self {
            kind: kind.into(),
            scale,
            decimals: number(value, "decimals", 0, 6)? as u8,
            prefix: field("prefix", 16)?,
            suffix: field("suffix", 16)?,
            align: align.into(),
            overflow: overflow.into(),
            unknown_text: field("unknownText", 63)?,
            stale_text: field("staleText", 63)?,
        })
    }
    pub fn value(&self) -> Value {
        json!({"kind":self.kind,"scale":self.scale,"decimals":self.decimals,"prefix":self.prefix,"suffix":self.suffix,
        "align":self.align,"overflow":self.overflow,"unknownText":self.unknown_text,"staleText":self.stale_text})
    }
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Output {
    pub id: String,
    pub target: Target,
    pub control: String,
    pub output: usize,
    pub action_id: String,
    pub format: Format,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Profile {
    pub aircraft_module: String,
    pub catalogue_path: PathBuf,
    pub network: Network,
    pub inputs: Vec<Input>,
    pub outputs: Vec<Output>,
}
impl Profile {
    pub fn parse(value: &Value) -> Result<Self, String> {
        if !value.is_object()
            || value.to_string().len() > 65536
            || value["schemaVersion"] != 2
            || !value["aircraftModule"].as_str().is_some_and(|s| {
                !s.is_empty()
                    && s.len() <= 63
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            })
        {
            return Err(INVALID.into());
        }
        let path = text(&value["cataloguePath"], 4096, false)
            .filter(|p| Path::new(p).is_absolute())
            .ok_or(INVALID)?;
        let input_values = value["inputs"]
            .as_array()
            .filter(|v| v.len() <= 16)
            .ok_or(INVALID)?;
        let output_values = value["outputs"]
            .as_array()
            .filter(|v| v.len() <= 16)
            .ok_or(INVALID)?;
        if input_values.is_empty() && output_values.is_empty() {
            return Err(INVALID.into());
        }
        let mut ids = HashSet::new();
        let mut sources = HashSet::new();
        let mut commands = HashSet::new();
        let mut setters = HashSet::new();
        let mut targets = BTreeSet::new();
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        for binding in input_values {
            let id = text(&binding["id"], 31, false)
                .filter(|s| identifier(s, 31, true))
                .ok_or(INVALID)?
                .to_owned();
            let target = Target::parse(binding)?;
            let kind = match binding["kind"].as_str() {
                Some("button") => InputKind::Button,
                Some("switch") => InputKind::Switch,
                Some("encoder") => InputKind::Encoder,
                Some("axis") => InputKind::Axis,
                _ => return Err(INVALID.into()),
            };
            let control = binding["control"]
                .as_u64()
                .filter(|n| {
                    *n <= if matches!(kind, InputKind::Encoder | InputKind::Axis) {
                        7
                    } else {
                        127
                    }
                })
                .ok_or(INVALID)? as usize;
            let command = text(&binding["command"], 64, false)
                .filter(|s| identifier(s, 64, false))
                .ok_or(INVALID)?
                .to_owned();
            let interface = text(&binding["interface"], 32, false)
                .ok_or(INVALID)?
                .to_owned();
            if !matches!(
                (kind, interface.as_str()),
                (InputKind::Encoder, "fixed_step" | "variable_step")
                    | (
                        InputKind::Button | InputKind::Switch | InputKind::Axis,
                        "set_state"
                    )
            ) || !ids.insert(id.clone())
                || !sources.insert((target.clone(), kind, control))
                || !commands.insert(command.clone())
            {
                return Err(INVALID.into());
            }
            targets.insert(target.clone());
            let step = number(binding, "step", 1, 65535)?;
            if step == 0 {
                return Err(INVALID.into());
            }
            inputs.push(Input {
                id,
                target,
                kind,
                control,
                command,
                interface,
                pressed_value: number(binding, "pressedValue", 1, 65535)?,
                released_value: number(binding, "releasedValue", 0, 65535)?,
                step,
                deadband: number(binding, "deadband", 128, 65535)?,
            });
        }
        for binding in output_values {
            let id = text(&binding["id"], 31, false)
                .filter(|s| identifier(s, 31, true))
                .ok_or(INVALID)?
                .to_owned();
            if !ids.insert(id.clone()) {
                return Err(INVALID.into());
            }
            let target = Target::parse(binding)?;
            targets.insert(target.clone());
            let control = text(&binding["control"], 64, false)
                .filter(|s| identifier(s, 64, false))
                .ok_or(INVALID)?
                .to_owned();
            let action_id = text(&binding["actionId"], 31, false)
                .filter(|s| identifier(s, 31, true))
                .ok_or(INVALID)?
                .to_owned();
            if !setters.insert((target.clone(), action_id.clone())) {
                return Err(
                    "Each simulator output setter requires one authoritative binding.".into(),
                );
            }
            outputs.push(Output {
                id,
                target,
                control,
                output: number(binding, "output", 0, 15)? as usize,
                action_id,
                format: Format::parse(&binding["format"])?,
            });
        }
        if targets.len() > 8
            || targets
                .iter()
                .map(|t| &t.gateway_id)
                .collect::<HashSet<_>>()
                .len()
                > 4
        {
            return Err(INVALID.into());
        }
        Ok(Self {
            aircraft_module: value["aircraftModule"].as_str().unwrap().into(),
            catalogue_path: path.into(),
            network: Network::parse(value)?,
            inputs,
            outputs,
        })
    }
    pub fn value(&self) -> Value {
        let mut value = self.network.value();
        value["schemaVersion"] = json!(2);
        value["aircraftModule"] = json!(self.aircraft_module);
        value["cataloguePath"] = json!(self.catalogue_path);
        value["inputs"]=json!(self.inputs.iter().map(|i|json!({"id":i.id,"gatewayId":i.target.gateway_id,"moduleId":i.target.module_id,"kind":i.kind.name(),"control":i.control,"command":i.command,"interface":i.interface,"pressedValue":i.pressed_value,"releasedValue":i.released_value,"step":i.step,"deadband":i.deadband})).collect::<Vec<_>>());
        value["outputs"]=json!(self.outputs.iter().map(|o|json!({"id":o.id,"gatewayId":o.target.gateway_id,"moduleId":o.target.module_id,"control":o.control,"output":o.output,"actionId":o.action_id,"format":o.format.value()})).collect::<Vec<_>>());
        value
    }
    pub fn load(path: &Path) -> Result<Option<Self>, String> {
        match fs::metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
            Ok(_) => Self::parse(&dcs_bios::read_json(path, 65536)?).map(Some),
        }
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        CompiledProfile::load(Self::parse(&self.value())?)?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or(INVALID)?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temporary = parent.join(format!(".dcs-profile-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut options = OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
            file.write_all(
                serde_json::to_string_pretty(&self.value())
                    .unwrap()
                    .as_bytes(),
            )
            .map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            fs::rename(&temporary, path).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            fs::File::open(parent)
                .and_then(|f| f.sync_all())
                .map_err(|e| e.to_string())?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}
#[derive(Clone, Debug)]
pub(crate) struct CompiledInput {
    pub binding: Input,
    pub maximum: u16,
}
#[derive(Clone, Debug)]
pub(crate) enum OutputSource {
    Integer(IntegerOutput),
    String { address: u16, length: usize },
}
impl OutputSource {
    pub fn read(&self, memory: &Memory) -> Option<Value> {
        match self {
            Self::Integer(output) => output
                .read(memory)
                .filter(|n| *n <= output.maximum)
                .map(|n| json!(n)),
            Self::String { address, length } => memory.string(*address, *length).map(Value::String),
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct CompiledOutput {
    pub binding: Output,
    pub source: OutputSource,
}
#[derive(Clone, Debug)]
pub(crate) struct CompiledProfile {
    pub profile: Profile,
    pub catalogue: Catalogue,
    pub inputs: Vec<CompiledInput>,
    pub outputs: Vec<CompiledOutput>,
}
fn overlaps(start: usize, length: usize, metadata: &Catalogue) -> bool {
    let end = start + length;
    end > 65536
        || (start < metadata.aircraft_address as usize + metadata.aircraft_length
            && end > metadata.aircraft_address as usize)
        || end > metadata.counter.address as usize
}
fn input_interface(value: &Value) -> Option<(&str, Option<u16>, Option<u16>)> {
    let interface = value["interface"].as_str()?;
    match interface {
        "fixed_step" => Some((interface, None, None)),
        "set_state" | "variable_step" => {
            let maximum = value["max_value"].as_u64().filter(|n| *n <= 65535)? as u16;
            let suggested = value
                .get("suggested_step")
                .and_then(Value::as_u64)
                .filter(|n| *n > 0 && *n <= maximum as u64)
                .map(|n| n as u16);
            Some((interface, Some(maximum), suggested))
        }
        _ => None,
    }
}
impl CompiledProfile {
    pub fn load(profile: Profile) -> Result<Self, String> {
        let catalogue = Catalogue::load_module(&profile.catalogue_path, &profile.aircraft_module)?;
        let document = dcs_bios::read_json(
            &profile
                .catalogue_path
                .join(format!("{}.json", profile.aircraft_module)),
            dcs_bios::MAX_DOCUMENT,
        )?;
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        for binding in &profile.inputs {
            let control = dcs_bios::control(&document, &binding.command)?;
            let interfaces = control["inputs"]
                .as_array()
                .filter(|inputs| inputs.len() <= 16)
                .ok_or(CATALOGUE_ERROR)?;
            let matching = interfaces
                .iter()
                .filter_map(input_interface)
                .filter(|(name, _, _)| *name == binding.interface)
                .collect::<Vec<_>>();
            if matching.len() != 1 {
                return Err(format!(
                    "{} does not advertise the selected input interface.",
                    binding.command
                ));
            }
            let maximum = matching[0].1.unwrap_or(65535);
            if (binding.interface == "set_state"
                && (binding.pressed_value > maximum || binding.released_value > maximum))
                || (binding.interface == "variable_step" && binding.step > maximum)
            {
                return Err(format!(
                    "{} input values exceed its installed catalogue bounds.",
                    binding.command
                ));
            }
            inputs.push(CompiledInput {
                binding: binding.clone(),
                maximum,
            });
        }
        for binding in &profile.outputs {
            let control = dcs_bios::control(&document, &binding.control)?;
            let output = control["outputs"]
                .as_array()
                .filter(|outputs| outputs.len() <= 16)
                .and_then(|v| v.get(binding.output))
                .ok_or(CATALOGUE_ERROR)?;
            let source = match output["type"].as_str() {
                Some("integer") => {
                    let output = IntegerOutput::parse(output)?;
                    if overlaps(output.address as usize, 2, &catalogue)
                        || !matches!(
                            binding.format.kind.as_str(),
                            "boolean" | "integer" | "fixed"
                        )
                        || (binding.format.kind == "boolean" && output.maximum != 1)
                    {
                        return Err(CATALOGUE_ERROR.into());
                    }
                    OutputSource::Integer(output)
                }
                Some("string") => {
                    let address = output["address"]
                        .as_u64()
                        .filter(|n| *n <= 65534 && *n % 2 == 0)
                        .ok_or(CATALOGUE_ERROR)? as u16;
                    let length = output["max_length"]
                        .as_u64()
                        .filter(|n| (1..=1024).contains(n))
                        .ok_or(CATALOGUE_ERROR)? as usize;
                    if overlaps(address as usize, length, &catalogue)
                        || binding.format.kind != "text"
                    {
                        return Err(CATALOGUE_ERROR.into());
                    }
                    OutputSource::String { address, length }
                }
                _ => return Err(CATALOGUE_ERROR.into()),
            };
            outputs.push(CompiledOutput {
                binding: binding.clone(),
                source,
            });
        }
        Ok(Self {
            profile,
            catalogue,
            inputs,
            outputs,
        })
    }
}
pub(crate) fn query_catalogue(request: &Value) -> Result<Value, String> {
    let path = text(&request["cataloguePath"], 4096, false)
        .filter(|p| Path::new(p).is_absolute())
        .ok_or(INVALID)?;
    let aircraft_module = text(&request["aircraftModule"], 63, false)
        .filter(|s| {
            s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        })
        .ok_or(INVALID)?;
    let query = request
        .get("query")
        .map_or(Some(""), |v| text(v, 128, true))
        .ok_or(INVALID)?
        .to_ascii_lowercase();
    let kind = request
        .get("kind")
        .map_or(Some("all"), Value::as_str)
        .filter(|v| matches!(*v, "input" | "output" | "all"))
        .ok_or(INVALID)?;
    let offset = request
        .get("offset")
        .map_or(Some(0), Value::as_u64)
        .filter(|n| *n <= 20000)
        .ok_or(INVALID)? as usize;
    let limit = request
        .get("limit")
        .map_or(Some(32), Value::as_u64)
        .filter(|n| (1..=64).contains(n))
        .ok_or(INVALID)? as usize;
    let document = dcs_bios::read_json(
        &Path::new(path).join(format!("{aircraft_module}.json")),
        dcs_bios::MAX_DOCUMENT,
    )?;
    let metadata = Catalogue::load_module(Path::new(path), aircraft_module)?;
    let categories = document.as_object().ok_or(CATALOGUE_ERROR)?;
    let mut controls = Vec::new();
    let mut seen = HashSet::new();
    let mut count = 0;
    for (category, entries) in categories {
        for (id, control) in entries.as_object().ok_or(CATALOGUE_ERROR)? {
            count += 1;
            if count > 20000 || !seen.insert(id.clone()) {
                return Err(CATALOGUE_ERROR.into());
            }
            if !identifier(id, 64, false) || control["identifier"] != *id {
                continue;
            }
            let label = text(&control["description"], 128, false).unwrap_or(id);
            let category = text(&control["category"], 63, false).unwrap_or_else(|| {
                if !category.is_empty()
                    && category.len() <= 63
                    && !category.chars().any(char::is_control)
                {
                    category
                } else {
                    "Other"
                }
            });
            if !format!("{id} {label} {category}")
                .to_ascii_lowercase()
                .contains(&query)
            {
                continue;
            }
            let mut interfaces = HashSet::new();
            let inputs = control["inputs"]
                .as_array()
                .filter(|inputs| inputs.len() <= 16)
                .into_iter()
                .flatten()
                .filter_map(input_interface)
                .filter(|(interface, _, _)| interfaces.insert(*interface))
                .map(|(interface, max, suggested)| {
                    let mut value = json!({"interface":interface});
                    if let Some(max) = max {
                        value["maxValue"] = json!(max);
                    }
                    if let Some(step) = suggested {
                        value["suggestedStep"] = json!(step);
                    }
                    value
                })
                .collect::<Vec<_>>();
            let outputs = control["outputs"]
                .as_array()
                .filter(|outputs| outputs.len() <= 16)
                .into_iter()
                .flatten()
                .enumerate()
                .filter_map(|(index, output)| {
                    let mut normalized = match output["type"].as_str() {
                        Some("integer") => IntegerOutput::parse(output)
                            .ok()
                            .filter(|integer| !overlaps(integer.address as usize, 2, &metadata))
                            .map(|v| json!({"type":"integer","maxValue":v.maximum})),
                        Some("string") => output["max_length"]
                            .as_u64()
                            .zip(output["address"].as_u64())
                            .filter(|(length, address)| {
                                (1..=1024).contains(length)
                                    && *address <= 65534
                                    && *address % 2 == 0
                                    && !overlaps(*address as usize, *length as usize, &metadata)
                            })
                            .map(|(length, _)| json!({"type":"string","maxLength":length})),
                        _ => None,
                    }?;
                    normalized["index"] = json!(index);
                    Some(normalized)
                })
                .collect::<Vec<_>>();
            if (kind == "input" && inputs.is_empty())
                || (kind == "output" && outputs.is_empty())
                || (inputs.is_empty() && outputs.is_empty())
            {
                continue;
            }
            controls.push(json!({"id":id,"label":label,"category":category,"inputs":inputs,"outputs":outputs}));
        }
    }
    controls.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    let total = controls.len();
    if offset > total {
        return Err(INVALID.into());
    }
    let reply = json!({"cataloguePath":path,"aircraftModule":aircraft_module,"query":query,"kind":kind,"offset":offset,"total":total,"controls":controls.into_iter().skip(offset).take(limit).collect::<Vec<_>>()});
    if reply.to_string().len() > 65536 {
        return Err("The catalogue page exceeds the response budget; reduce its limit.".into());
    }
    Ok(reply)
}
pub(crate) fn format_output(
    binding: &Output,
    value: Option<&Value>,
    fresh: bool,
    action: &Value,
) -> Result<Value, String> {
    crate::actions::descriptor(action)?;
    let parameters = action["parameters"].as_array().unwrap();
    if action["semantics"] != "setter" || action["readable"] != true || parameters.len() != 1 {
        return Err("Simulator outputs require one readable setter parameter.".into());
    }
    let parameter = &parameters[0];
    if binding.format.kind == "boolean" {
        let on = if fresh {
            match value {
                Some(v) => v
                    .as_bool()
                    .or_else(|| v.as_u64().filter(|n| *n <= 1).map(|n| n == 1))
                    .ok_or("Boolean feedback must be 0 or 1.")?,
                None => false,
            }
        } else {
            false
        };
        let result = match parameter["type"].as_str() {
            Some("boolean") => json!([on]),
            Some("enum")
                if parameter["options"].as_array().is_some_and(|options| {
                    options.iter().any(|option| option == "on")
                        && options.iter().any(|option| option == "off")
                }) =>
            {
                json!([if on { "on" } else { "off" }])
            }
            _ => return Err("Boolean feedback requires a boolean or on/off enum setter.".into()),
        };
        crate::actions::arguments(action, &result)?;
        return Ok(result);
    }
    if parameter["type"] != "string" {
        return Err("Display feedback requires a readable string setter.".into());
    }
    let format = &binding.format;
    let content = if !fresh {
        format.stale_text.clone()
    } else if let Some(value) = value {
        let raw = match format.kind.as_str() {
            "text" => value
                .as_str()
                .ok_or("Text feedback must be a string.")?
                .to_owned(),
            "integer" | "fixed" => {
                let number = value
                    .as_f64()
                    .filter(|v| v.is_finite())
                    .ok_or("Numeric display feedback must be finite.")?
                    * format.scale;
                if !number.is_finite() || number.abs() > 1e15 {
                    return Err("Numeric display feedback exceeds the formatting bound.".into());
                }
                if format.kind == "integer" {
                    format!("{:.0}", number.round())
                } else {
                    format!("{:.*}", format.decimals as usize, number)
                }
            }
            _ => return Err(INVALID.into()),
        };
        format!("{}{}{}", format.prefix, raw, format.suffix)
    } else {
        format.unknown_text.clone()
    };
    if content.chars().any(char::is_control) {
        return Err("Display text contains a control character.".into());
    }
    let max_bytes = parameter["maxBytes"].as_u64().unwrap() as usize;
    let display = action.get("display");
    let (cells, ascii) = if let Some(display) = display {
        let columns = display["columns"]
            .as_u64()
            .filter(|n| (1..=63).contains(n))
            .ok_or(INVALID)? as usize;
        let rows = display["rows"]
            .as_u64()
            .filter(|n| (1..=4).contains(n))
            .ok_or(INVALID)? as usize;
        let encoding = display["encoding"]
            .as_str()
            .filter(|v| matches!(*v, "ascii" | "utf8"))
            .ok_or(INVALID)?;
        if columns * rows > 63 || max_bytes < columns * rows {
            return Err(INVALID.into());
        }
        (Some(columns * rows), encoding == "ascii")
    } else {
        (None, false)
    };
    if ascii && !content.is_ascii() {
        return Err("The display accepts ASCII text only.".into());
    }
    let fits = |s: &str| {
        s.len() + cells.map_or(0, |c| c.saturating_sub(s.chars().count())) <= max_bytes
            && cells.is_none_or(|c| s.chars().count() <= c)
    };
    let mut result = content.clone();
    if !fits(&result) {
        if format.overflow == "reject" {
            return Err(
                "Formatted display text exceeds the endpoint's cell or byte budget.".into(),
            );
        }
        result.clear();
        for character in content.chars() {
            let mut next = result.clone();
            next.push(character);
            if !fits(&next) {
                break;
            }
            result = next;
        }
    }
    if let Some(cells) = cells {
        let padding = " ".repeat(cells - result.chars().count());
        result = if format.align == "right" {
            format!("{padding}{result}")
        } else {
            format!("{result}{padding}")
        };
    }
    let result = json!([result]);
    crate::actions::arguments(action, &result)?;
    Ok(result)
}
#[derive(Clone, Debug)]
pub(crate) struct ExportSnapshot {
    pub epoch: u64,
    pub aircraft: Option<String>,
    pub mission_active: bool,
    pub fresh: bool,
    pub age_ms: Option<u64>,
    pub counter: Option<u16>,
    pub values: BTreeMap<String, Value>,
    pub malformed_frames: u64,
}
pub(crate) struct ExportSession {
    catalogue: Catalogue,
    outputs: Vec<CompiledOutput>,
    freshness_ms: u64,
    parser: Parser,
    memory: Memory,
    aircraft: Option<String>,
    counter: Option<u16>,
    last_frame: Option<u64>,
    epoch: u64,
    malformed_frames: u64,
}
impl ExportSession {
    pub fn new(profile: &CompiledProfile) -> Self {
        Self {
            catalogue: profile.catalogue.clone(),
            outputs: profile.outputs.clone(),
            freshness_ms: profile.profile.network.freshness_ms,
            parser: Parser::new(profile.catalogue.counter.address),
            memory: Memory::new(),
            aircraft: None,
            counter: None,
            last_frame: None,
            epoch: 1,
            malformed_frames: 0,
        }
    }
    fn invalidate(&mut self) {
        self.memory.clear();
        self.counter = None;
        self.last_frame = None;
        self.epoch = self.epoch.saturating_add(1);
    }
    fn active(&self) -> bool {
        self.aircraft
            .as_ref()
            .is_some_and(|a| self.catalogue.aircraft_names.contains(a))
    }
    pub fn tick(&mut self, now_ms: u64) {
        if self
            .last_frame
            .is_some_and(|t| now_ms.saturating_sub(t) > self.freshness_ms)
        {
            // Silence invalidates source bytes, not the last accepted counter.
            // A frozen full export must not rearm the profile after each timeout.
            let counter = self.counter;
            self.invalidate();
            self.counter = counter;
            self.parser = Parser::new(self.catalogue.counter.address);
        }
    }
    pub fn reset_stream(&mut self, _now_ms: u64) {
        self.aircraft = None;
        self.invalidate();
        self.parser = Parser::new(self.catalogue.counter.address);
    }
    pub fn feed(&mut self, bytes: &[u8], now_ms: u64) {
        self.tick(now_ms);
        for event in self.parser.feed(bytes) {
            match event {
                ExportEvent::Reset => {
                    self.malformed_frames = self.malformed_frames.saturating_add(1);
                    self.aircraft = None;
                    self.invalidate();
                }
                ExportEvent::Frame(blocks) => {
                    let Some(counter) = self.catalogue.counter.read_blocks(&blocks) else {
                        self.malformed_frames = self.malformed_frames.saturating_add(1);
                        self.aircraft = None;
                        self.invalidate();
                        continue;
                    };
                    let advance = self
                        .counter
                        .map(|previous| counter.wrapping_sub(previous) & 255);
                    if advance.is_some_and(|d| d >= 128) {
                        self.aircraft = None;
                        self.invalidate();
                        continue;
                    }
                    let mut candidate = self.memory.clone();
                    candidate.apply(&blocks);
                    let name = candidate.string(
                        self.catalogue.aircraft_address,
                        self.catalogue.aircraft_length,
                    );
                    let aircraft = name.clone().filter(|a| !a.trim().is_empty() && a != "NONE");
                    if advance == Some(0) {
                        // After a timeout, a sparse duplicate has unknown name
                        // bytes. Only an explicit blank/NONE name is shutdown.
                        if self.aircraft.is_some()
                            && name.is_some_and(|name| name.trim().is_empty() || name == "NONE")
                        {
                            self.aircraft = None;
                            self.invalidate();
                        }
                        continue;
                    }
                    if aircraft != self.aircraft {
                        self.invalidate();
                        self.memory.apply(&blocks);
                        let start = self.catalogue.aircraft_address as usize;
                        let range = start..start + self.catalogue.aircraft_length;
                        self.memory.bytes[range.clone()]
                            .copy_from_slice(&candidate.bytes[range.clone()]);
                        self.memory.valid[range.clone()].copy_from_slice(&candidate.valid[range]);
                        self.aircraft = aircraft;
                    } else {
                        self.memory = candidate;
                    }
                    self.counter = Some(counter);
                    self.last_frame = Some(now_ms);
                }
            }
        }
    }
    pub fn snapshot(&self, now_ms: u64) -> ExportSnapshot {
        let mission_active = self.active();
        let fresh = mission_active
            && self
                .last_frame
                .is_some_and(|t| now_ms.saturating_sub(t) <= self.freshness_ms);
        let values = if fresh {
            self.outputs
                .iter()
                .filter_map(|output| {
                    output
                        .source
                        .read(&self.memory)
                        .map(|v| (output.binding.id.clone(), v))
                })
                .collect()
        } else {
            BTreeMap::new()
        };
        ExportSnapshot {
            epoch: self.epoch,
            aircraft: self.aircraft.clone(),
            mission_active,
            fresh,
            age_ms: self.last_frame.map(|t| now_ms.saturating_sub(t)),
            counter: self.counter,
            values,
            malformed_frames: self.malformed_frames,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dcs_bios::Block;

    // Authored fixtures use upstream JSON shapes/names from DCS-Skunkworks
    // 11c6209; synthetic addresses here are deliberately not aircraft constants.
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../.research/m3/profile-tests")
                .join(uuid::Uuid::new_v4().to_string());
            fs::create_dir_all(&path).unwrap();
            let sources =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dcs-bios");
            for name in [
                "A-10C.json",
                "MetadataStart.json",
                "MetadataEnd.json",
                "AircraftAliases.json",
            ] {
                fs::copy(sources.join(name), path.join(name)).unwrap();
            }
            let mut document =
                dcs_bios::read_json(&path.join("A-10C.json"), dcs_bios::MAX_DOCUMENT).unwrap();
            document["Radio"] = json!({
                "UHF_VOL":{"identifier":"UHF_VOL","description":"UHF Volume Control","category":"UHF Radio","inputs":[{"interface":"set_state","max_value":65535}],"outputs":[{"type":"integer","address":8192,"mask":65535,"shift_by":0,"max_value":65535}]},
                "UHF_PRESET_SEL":{"identifier":"UHF_PRESET_SEL","inputs":[{"interface":"fixed_step"}],"outputs":[{"type":"integer","address":8194,"mask":31,"shift_by":0,"max_value":20}]},
                "UHF_FREQUENCY":{"identifier":"UHF_FREQUENCY","inputs":[],"outputs":[{"type":"string","address":8208,"max_length":7}]},
                "UHF_PRESET":{"identifier":"UHF_PRESET","inputs":[],"outputs":[{"type":"string","address":8224,"max_length":2}]},
                "DIMMER":{"identifier":"DIMMER","inputs":[{"interface":"variable_step","max_value":100,"suggested_step":5}],"outputs":[]},
                "SWITCH":{"identifier":"SWITCH","inputs":[{"interface":"set_state","max_value":1}],"outputs":[]},
                "UNSUPPORTED":{"identifier":"UNSUPPORTED","inputs":[{"interface":"set_string"}],"outputs":[]}
            });
            fs::write(path.join("A-10C.json"), document.to_string()).unwrap();
            Self(path)
        }
        fn config(&self) -> Value {
            let target = json!({"gatewayId":"stm32:00112233445566778899aabb","moduleId":"00112233445566778899aabbccddeeff"});
            let input = |id: &str, kind: &str, control: u8, command: &str, interface: &str| {
                let mut value = target.clone();
                for (key, v) in [
                    ("id", json!(id)),
                    ("kind", json!(kind)),
                    ("control", json!(control)),
                    ("command", json!(command)),
                    ("interface", json!(interface)),
                ] {
                    value[key] = v;
                }
                value
            };
            let output = |id: &str, control: &str, kind: &str| {
                let mut value = target.clone();
                value["id"] = json!(id);
                value["control"] = json!(control);
                value["actionId"] = json!(if kind == "boolean" {
                    "led.set".to_owned()
                } else {
                    format!("display.{id}.set")
                });
                value["format"] = json!({"kind":kind});
                value
            };
            json!({"schemaVersion":2,"aircraftModule":"A-10C","cataloguePath":self.0,"inputs":[
                input("ack","button",0,"UFC_MASTER_CAUTION","set_state"),input("switch","switch",1,"SWITCH","set_state"),
                input("preset-selector","encoder",0,"UHF_PRESET_SEL","fixed_step"),input("volume","axis",0,"UHF_VOL","set_state")],
                "outputs":[output("caution","MASTER_CAUTION","boolean"),output("frequency","UHF_FREQUENCY","text"),output("preset","UHF_PRESET","text"),output("numeric","UHF_VOL","fixed")]})
        }
        fn compiled(&self) -> CompiledProfile {
            CompiledProfile::load(Profile::parse(&self.config()).unwrap()).unwrap()
        }
        fn edit(&self, edit: impl FnOnce(&mut Value)) {
            let path = self.0.join("A-10C.json");
            let mut doc = dcs_bios::read_json(&path, dcs_bios::MAX_DOCUMENT).unwrap();
            edit(&mut doc);
            fs::write(path, doc.to_string()).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn action(kind: &str, bytes: usize, display: Option<(usize, usize, &str)>) -> Value {
        let parameter = if kind == "enum" {
            json!({"id":"mode","label":"Mode","type":"enum","options":["auto","off","on"]})
        } else {
            json!({"id":"value","label":"Value","type":kind,"maxBytes":bytes})
        };
        let mut action = json!({"id":"display.set","label":"Display","endpoint":"display","semantics":"setter","readable":true,"parameters":[parameter]});
        if let Some((columns, rows, encoding)) = display {
            action["display"] = json!({"columns":columns,"rows":rows,"encoding":encoding});
        }
        action
    }
    fn block(bytes: &mut Vec<u8>, address: u16, data: &[u8]) {
        assert!(data.len() % 2 == 0);
        bytes.extend(address.to_le_bytes());
        bytes.extend((data.len() as u16).to_le_bytes());
        bytes.extend(data);
    }
    fn frame(name: Option<&str>, frequency: Option<&[u8]>, counter: u8) -> Vec<u8> {
        let mut bytes = vec![0x55; 4];
        if let Some(name) = name {
            let mut data = [b' '; 24];
            data[..name.len()].copy_from_slice(name.as_bytes());
            block(&mut bytes, 0, &data);
        }
        if let Some(frequency) = frequency {
            block(&mut bytes, 8208, frequency);
        }
        block(&mut bytes, 0xfffe, &[counter, 0]);
        bytes
    }

    #[test]
    fn profile_round_trip_defaults_flat_targets_and_at_least_one_binding() {
        let fixture = Fixture::new();
        let profile = Profile::parse(&fixture.config()).unwrap();
        assert_eq!(Profile::parse(&profile.value()).unwrap(), profile);
        assert_eq!(
            (
                profile.inputs[0].pressed_value,
                profile.inputs[0].released_value,
                profile.inputs[0].step,
                profile.inputs[0].deadband
            ),
            (1, 0, 1, 128)
        );
        assert_eq!(profile.outputs[1].format.scale, 1.0);
        assert_eq!(
            profile.value()["inputs"][0]["gatewayId"],
            profile.inputs[0].target.gateway_id
        );
        let mut config = fixture.config();
        config["inputs"] = json!([]);
        assert!(Profile::parse(&config).is_ok());
        config["outputs"] = json!([]);
        assert!(Profile::parse(&config).is_err());
        config = fixture.config();
        config["outputs"] = json!([]);
        assert!(Profile::parse(&config).is_ok());
        config["inputs"][0]["gatewayId"] = json!("stm32:00112233445566778899AABB");
        assert_eq!(
            Profile::parse(&config).unwrap().inputs[0].target.gateway_id,
            "stm32:00112233445566778899aabb"
        );
    }
    #[test]
    fn invalid_schema_network_identity_and_binding_bounds_are_rejected() {
        let fixture = Fixture::new();
        for (pointer, value) in [
            ("/schemaVersion", json!(1)),
            ("/aircraftModule", json!("../Other")),
            ("/cataloguePath", json!("relative")),
            ("/cataloguePath", json!("/bad\npath")),
            ("/inputs/0/id", json!("bad id")),
            (
                "/inputs/0/gatewayId",
                json!("stm32:000000000000000000000000"),
            ),
            (
                "/inputs/0/moduleId",
                json!("ffffffffffffffffffffffffffffffff"),
            ),
            ("/inputs/0/kind", json!("selector")),
            ("/inputs/0/control", json!(128)),
            ("/inputs/2/control", json!(8)),
            ("/inputs/3/control", json!(8)),
            ("/inputs/0/command", json!("CMD 1\n")),
            ("/inputs/0/interface", json!("fixed_step")),
            ("/inputs/2/interface", json!("set_state")),
            ("/inputs/0/pressedValue", json!(65536)),
            ("/inputs/0/releasedValue", json!(-1)),
            ("/inputs/0/step", json!(0)),
            ("/inputs/0/deadband", json!(65536)),
            ("/outputs/0/control", json!("bad!")),
            ("/outputs/0/actionId", json!("bad action")),
            ("/outputs/0/output", json!(16)),
            ("/outputs/0/format/kind", json!("code")),
            ("/outputs/0/format/scale", json!(1000001)),
            ("/outputs/0/format/decimals", json!(7)),
            ("/outputs/0/format/align", json!("center")),
            ("/outputs/0/format/overflow", json!("wrap")),
            ("/outputs/0/format/prefix", json!("\t")),
            ("/outputs/0/format/suffix", json!("x".repeat(17))),
            ("/outputs/0/format/unknownText", json!("x".repeat(64))),
            ("/outputs/0/format", Value::Null),
        ] {
            let mut config = fixture.config();
            let (parent, key) = pointer.rsplit_once('/').unwrap();
            config.pointer_mut(parent).unwrap()[key] = value;
            assert!(Profile::parse(&config).is_err(), "accepted {pointer}");
        }
        let mut config = fixture.config();
        config["listenPort"] = json!(0);
        assert!(Profile::parse(&config).is_err());
        config = fixture.config();
        config["unknownPayload"] = json!("x".repeat(65536));
        assert!(Profile::parse(&config).is_err());
        config = fixture.config();
        config["inputs"][0]
            .as_object_mut()
            .unwrap()
            .remove("control");
        assert!(Profile::parse(&config).is_err());
        assert!(Profile::parse(&Value::Null).is_err());
    }

    #[test]
    fn pico_debug_targets_round_trip_without_changing_legacy_can_identity() {
        let fixture = Fixture::new();
        let mut config = fixture.config();
        let pico = "pico:OSG-PICO-0123456789abcdef";
        config["inputs"][0]["gatewayId"] = json!(pico);
        config["outputs"][0]["gatewayId"] = json!(pico);
        let parsed = Profile::parse(&config).unwrap();
        assert_eq!(parsed.inputs[0].target.gateway_id, pico);
        assert_eq!(parsed.value()["outputs"][0]["gatewayId"], pico);
        for invalid in [
            "pico:OSG-PICO-nothex",
            "pico:OSG-PICO-0000000000000000",
            "pico:WRONG-0123456789abcdef",
            "other:0123456789abcdef",
        ] {
            config["inputs"][0]["gatewayId"] = json!(invalid);
            assert!(Profile::parse(&config).is_err(), "accepted {invalid}");
        }
    }
    #[test]
    fn duplicate_sources_commands_ids_and_target_limits_do_not_create_competing_bindings() {
        let fixture = Fixture::new();
        let mut config = fixture.config();
        config["outputs"][0]["id"] = json!("ack");
        assert!(Profile::parse(&config).is_err());
        config = fixture.config();
        let mut duplicate = config["inputs"][0].clone();
        duplicate["id"] = json!("other");
        duplicate["command"] = json!("SWITCH");
        config["inputs"].as_array_mut().unwrap().push(duplicate);
        assert!(Profile::parse(&config).is_err());
        config = fixture.config();
        config["inputs"][1]["command"] = json!("UFC_MASTER_CAUTION");
        assert!(Profile::parse(&config).is_err());
        config = fixture.config();
        config["inputs"] = json!([]);
        config["outputs"] = json!([]);
        for index in 0..9 {
            let mut output = fixture.config()["outputs"][0].clone();
            output["id"] = json!(format!("o{index}"));
            output["moduleId"] = json!(format!("{:032x}", index + 1));
            config["outputs"].as_array_mut().unwrap().push(output);
        }
        assert!(Profile::parse(&config).is_err());
        config["outputs"].as_array_mut().unwrap().truncate(5);
        for (index, output) in config["outputs"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            output["gatewayId"] = json!(format!("stm32:{:024x}", index + 1));
        }
        assert!(Profile::parse(&config).is_err());
        config["outputs"].as_array_mut().unwrap().truncate(4);
        assert!(Profile::parse(&config).is_ok());
        config["inputs"] = json!(vec![fixture.config()["inputs"][0].clone(); 17]);
        assert!(Profile::parse(&config).is_err());
    }
    #[test]
    fn output_setter_has_one_authoritative_source_per_device_but_shared_feedback_can_drive_other_devices(
    ) {
        let fixture = Fixture::new();
        let mut config = fixture.config();
        let mut duplicate = config["outputs"][1].clone();
        duplicate["id"] = json!("other-frequency");
        duplicate["control"] = json!("UHF_PRESET");
        config["outputs"].as_array_mut().unwrap().push(duplicate);
        assert!(Profile::parse(&config)
            .unwrap_err()
            .contains("authoritative"));
        config["outputs"][4]["moduleId"] = json!("112233445566778899aabbccddeeff00");
        assert!(Profile::parse(&config).is_ok());
    }
    #[test]
    fn persistence_validates_installed_interfaces_and_preserves_previous_bytes_on_failure() {
        let fixture = Fixture::new();
        let profile = Profile::parse(&fixture.config()).unwrap();
        let path = fixture.0.join("settings/profile.json");
        assert!(Profile::load(&path).unwrap().is_none());
        profile.save(&path).unwrap();
        assert_eq!(Profile::load(&path).unwrap(), Some(profile.clone()));
        let saved = fs::read(&path).unwrap();
        let mut invalid = profile.clone();
        invalid.inputs[0].command = "ABSENT".into();
        assert!(invalid.save(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), saved);
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::write(&path, "{").unwrap();
        assert!(Profile::load(&path).is_err());
        assert!(profile.save(Path::new("profile.json")).is_err());
    }
    #[test]
    fn named_interfaces_compile_and_selected_values_respect_catalogue_limits() {
        let fixture = Fixture::new();
        let compiled = fixture.compiled();
        assert_eq!(compiled.inputs[3].maximum, 65535);
        assert!(matches!(
            compiled.outputs[1].source,
            OutputSource::String { length: 7, .. }
        ));
        let mut config = fixture.config();
        config["inputs"][0]["pressedValue"] = json!(2);
        assert!(CompiledProfile::load(Profile::parse(&config).unwrap()).is_err());
        config = fixture.config();
        config["inputs"][2]["command"] = json!("DIMMER");
        config["inputs"][2]["interface"] = json!("variable_step");
        config["inputs"][2]["step"] = json!(5);
        assert_eq!(
            CompiledProfile::load(Profile::parse(&config).unwrap())
                .unwrap()
                .inputs[2]
                .maximum,
            100
        );
        config["inputs"][2]["step"] = json!(101);
        assert!(CompiledProfile::load(Profile::parse(&config).unwrap()).is_err());
        config = fixture.config();
        config["inputs"][2]["command"] = json!("ABSENT");
        assert!(CompiledProfile::load(Profile::parse(&config).unwrap()).is_err());
        fixture.edit(|doc|doc["Radio"]["UHF_VOL"]["inputs"]=json!([{"interface":"set_state","max_value":65535},{"interface":"set_state","max_value":65535}]));
        assert!(CompiledProfile::load(Profile::parse(&fixture.config()).unwrap()).is_err());
    }
    #[test]
    fn output_type_geometry_and_metadata_overlap_are_validated_before_start() {
        for (field, value) in [
            ("address", json!(0)),
            ("address", json!(65534)),
            ("address", json!(8209)),
            ("max_length", json!(1025)),
            ("max_length", json!(0)),
            ("type", json!("unknown")),
        ] {
            let fixture = Fixture::new();
            fixture.edit(|doc| doc["Radio"]["UHF_FREQUENCY"]["outputs"][0][field] = value);
            assert!(
                CompiledProfile::load(Profile::parse(&fixture.config()).unwrap()).is_err(),
                "accepted {field}"
            );
        }
        let fixture = Fixture::new();
        let mut config = fixture.config();
        config["outputs"][1]["output"] = json!(2);
        assert!(CompiledProfile::load(Profile::parse(&config).unwrap()).is_err());
        config = fixture.config();
        config["outputs"][1]["format"]["kind"] = json!("fixed");
        assert!(CompiledProfile::load(Profile::parse(&config).unwrap()).is_err());
        config = fixture.config();
        config["outputs"][3]["format"]["kind"] = json!("boolean");
        assert!(CompiledProfile::load(Profile::parse(&config).unwrap()).is_err());
        fixture.edit(|doc| doc["Radio"]["UHF_VOL"]["outputs"][0]["address"] = json!(65534));
        assert!(CompiledProfile::load(Profile::parse(&fixture.config()).unwrap()).is_err());
    }
    #[test]
    fn catalogue_query_is_bounded_normalized_filtered_and_contains_no_wire_addresses() {
        let fixture = Fixture::new();
        let request = json!({"cataloguePath":fixture.0,"aircraftModule":"A-10C","query":"uhf","kind":"all","offset":0,"limit":2});
        let first = query_catalogue(&request).unwrap();
        assert_eq!(first["total"], 4);
        assert_eq!(first["controls"].as_array().unwrap().len(), 2);
        assert!(!first.to_string().contains("address"));
        let mut next = request.clone();
        next["offset"] = json!(2);
        assert_eq!(
            query_catalogue(&next).unwrap()["controls"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        next["offset"] = json!(4);
        assert_eq!(query_catalogue(&next).unwrap()["controls"], json!([]));
        next["offset"] = json!(5);
        assert!(query_catalogue(&next).is_err());
        next = request.clone();
        next["kind"] = json!("input");
        assert_eq!(query_catalogue(&next).unwrap()["total"], 2);
        next["kind"] = json!("output");
        assert_eq!(query_catalogue(&next).unwrap()["total"], 4);
        next["query"] = json!("dimmer");
        next["kind"] = json!("input");
        assert_eq!(
            query_catalogue(&next).unwrap()["controls"][0]["inputs"][0]["suggestedStep"],
            5
        );
        for (key, value) in [
            ("limit", json!(65)),
            ("kind", json!("bad")),
            ("query", json!("x".repeat(129))),
            ("aircraftModule", json!("Other")),
        ] {
            let mut bad = request.clone();
            bad[key] = value;
            assert!(query_catalogue(&bad).is_err());
        }
        fixture.edit(|doc| doc["Bad"] = json!([]));
        assert!(query_catalogue(&request).is_err());
    }
    #[test]
    fn catalogue_query_preserves_raw_output_indexes_and_caps_normalized_response_bytes() {
        let fixture = Fixture::new();
        fixture.edit(|doc| {
            doc["Radio"]["UHF_FREQUENCY"]["description"] = json!("x".repeat(129));
            doc["Radio"]["UHF_FREQUENCY"]["outputs"]
                .as_array_mut()
                .unwrap()
                .insert(0, json!({"type":"unsupported"}));
        });
        let mut request = json!({"cataloguePath":fixture.0,"aircraftModule":"A-10C","query":"UHF_FREQUENCY","kind":"output"});
        let reply = query_catalogue(&request).unwrap();
        assert_eq!(reply["controls"][0]["label"], "UHF_FREQUENCY");
        assert_eq!(reply["controls"][0]["outputs"][0]["index"], 1);
        let mut config = fixture.config();
        config["outputs"][1]["output"] = json!(1);
        assert!(CompiledProfile::load(Profile::parse(&config).unwrap()).is_ok());
        fixture.edit(|doc|{
            for index in 0..64 {
                let id=format!("CONTROL_{index:02}");
                doc["Radio"][&id]=json!({"identifier":id,"description":"\"".repeat(128),"category":"\"".repeat(63),
                    "inputs":[{"interface":"set_state","max_value":65535},{"interface":"variable_step","max_value":65535,"suggested_step":1000},{"interface":"fixed_step"}],
                    "outputs":vec![json!({"type":"string","address":8208,"max_length":7});16]});
            }
        });
        request["query"] = json!("CONTROL_");
        request["limit"] = json!(64);
        assert!(query_catalogue(&request)
            .unwrap_err()
            .contains("response budget"));
        request["limit"] = json!(16);
        assert!(query_catalogue(&request).unwrap().to_string().len() <= 65536);
    }
    #[test]
    fn formatter_preserves_typed_boolean_and_rejects_incompatible_or_unreadable_actions() {
        let fixture = Fixture::new();
        let binding = &fixture.compiled().outputs[0].binding;
        assert_eq!(
            format_output(binding, Some(&json!(1)), true, &action("boolean", 0, None)).unwrap(),
            json!([true])
        );
        assert_eq!(
            format_output(binding, Some(&json!(0)), true, &action("enum", 0, None)).unwrap(),
            json!(["off"])
        );
        assert_eq!(
            format_output(binding, None, true, &action("boolean", 0, None)).unwrap(),
            json!([false])
        );
        assert_eq!(
            format_output(
                binding,
                Some(&json!(true)),
                false,
                &action("boolean", 0, None)
            )
            .unwrap(),
            json!([false])
        );
        assert!(
            format_output(binding, Some(&json!(2)), true, &action("boolean", 0, None)).is_err()
        );
        assert!(format_output(binding, Some(&json!(1)), true, &action("string", 7, None)).is_err());
        let mut incomplete = action("enum", 0, None);
        for options in [json!(["off"]), json!(["on"])] {
            incomplete["parameters"][0]["options"] = options;
            assert!(format_output(binding, None, true, &incomplete).is_err());
            assert!(format_output(binding, None, false, &incomplete).is_err());
        }
        let mut unavailable = action("boolean", 0, None);
        unavailable["readable"] = json!(false);
        assert!(format_output(binding, None, true, &unavailable).is_err());
        unavailable["readable"] = json!(true);
        unavailable["semantics"] = json!("oneShot");
        assert!(format_output(binding, None, true, &unavailable).is_err());
    }
    #[test]
    fn text_formatter_pads_full_surfaces_and_truncates_without_splitting_unicode_or_byte_budget() {
        let fixture = Fixture::new();
        let mut binding = fixture.compiled().outputs[1].binding.clone();
        let ascii = action("string", 7, Some((7, 1, "ascii")));
        assert_eq!(
            format_output(&binding, Some(&json!("123.450")), true, &ascii).unwrap(),
            json!(["123.450"])
        );
        binding.format.align = "right".into();
        assert_eq!(
            format_output(&binding, Some(&json!("24")), true, &ascii).unwrap(),
            json!(["     24"])
        );
        binding.format.unknown_text = "?".into();
        binding.format.stale_text = "STALE".into();
        assert_eq!(
            format_output(&binding, None, true, &ascii).unwrap(),
            json!(["      ?"])
        );
        assert_eq!(
            format_output(&binding, Some(&json!("123")), false, &ascii).unwrap(),
            json!(["  STALE"])
        );
        let utf8 = action("string", 7, Some((4, 1, "utf8")));
        binding.format.align = "left".into();
        assert_eq!(
            format_output(&binding, Some(&json!("😀ABCD")), true, &utf8).unwrap(),
            json!(["😀ABC"])
        );
        let utf8 = action("string", 4, Some((4, 1, "utf8")));
        assert_eq!(
            format_output(&binding, Some(&json!("😀")), true, &utf8).unwrap(),
            json!(["    "])
        );
        assert_eq!(
            format_output(
                &binding,
                Some(&json!("éAB")),
                true,
                &action("string", 3, None)
            )
            .unwrap(),
            json!(["éA"])
        );
        binding.format.overflow = "reject".into();
        assert!(format_output(&binding, Some(&json!("😀")), true, &utf8).is_err());
        assert!(format_output(&binding, Some(&json!("é")), true, &ascii).is_err());
        assert!(format_output(&binding, Some(&json!("\n")), true, &ascii).is_err());
        assert!(format_output(&binding, Some(&json!(1)), true, &ascii).is_err());
    }
    #[test]
    fn numeric_formatter_has_bounded_fixed_and_rounded_modes_without_templates() {
        let fixture = Fixture::new();
        let mut binding = fixture.compiled().outputs[3].binding.clone();
        let display = action("string", 12, Some((12, 1, "ascii")));
        binding.format.scale = 0.01;
        binding.format.decimals = 2;
        binding.format.prefix = "$".into();
        binding.format.suffix = "V".into();
        assert_eq!(
            format_output(&binding, Some(&json!(1234)), true, &display).unwrap(),
            json!(["$12.34V     "])
        );
        binding.format.kind = "integer".into();
        binding.format.prefix.clear();
        binding.format.suffix.clear();
        binding.format.scale = 0.5;
        assert_eq!(
            format_output(&binding, Some(&json!(5)), true, &display).unwrap(),
            json!(["3           "])
        );
        assert!(format_output(&binding, Some(&json!("5")), true, &display).is_err());
        assert!(format_output(&binding, Some(&json!(1e20)), true, &display).is_err());
        assert!(
            format_output(&binding, Some(&json!(1)), true, &action("boolean", 0, None)).is_err()
        );
        binding.format.kind = "unknown".into();
        assert!(format_output(&binding, Some(&json!(1)), true, &display).is_err());
    }
    #[test]
    fn fragmented_atomic_export_commits_full_width_strings_and_known_fields_only() {
        let fixture = Fixture::new();
        let compiled = fixture.compiled();
        let mut session = ExportSession::new(&compiled);
        assert!(!session.snapshot(0).fresh);
        let bytes = frame(Some("A-10C"), Some(b"123.450 "), 1);
        for (index, byte) in bytes.iter().enumerate() {
            session.feed(&[*byte], 10);
            assert_eq!(session.snapshot(10).fresh, index == bytes.len() - 1);
        }
        let snapshot = session.snapshot(10);
        assert_eq!(snapshot.aircraft, Some("A-10C".into()));
        assert_eq!(snapshot.values["frequency"], "123.450");
        assert!(!snapshot.values.contains_key("caution"));
        assert!(!snapshot.values.contains_key("preset"));
        let epoch = snapshot.epoch;
        let mut partial = vec![0x55; 4];
        block(&mut partial, 8208, b"225.");
        session.feed(&partial, 20);
        assert_eq!(session.snapshot(20).values["frequency"], "123.450");
        let mut completion = Vec::new();
        block(&mut completion, 8212, b"500 ");
        block(&mut completion, 0xfffe, &[2, 0]);
        session.feed(&completion, 20);
        assert_eq!(session.snapshot(20).values["frequency"], "225.500");
        assert_eq!(session.snapshot(20).epoch, epoch);
        let mut frames = frame(None, Some(b"300.000 "), 3);
        frames.extend(frame(None, Some(b"400.000 "), 4));
        session.feed(&frames, 30);
        assert_eq!(session.snapshot(30).values["frequency"], "400.000");
    }
    #[test]
    fn duplicates_never_overwrite_refresh_or_reset_a_live_epoch_and_shutdown_names_clear_mission() {
        let fixture = Fixture::new();
        let mut session = ExportSession::new(&fixture.compiled());
        session.feed(&frame(Some("A-10C"), Some(b"123.450 "), 1), 0);
        let epoch = session.snapshot(0).epoch;
        session.feed(&frame(None, Some(b"999.999 "), 1), 1000);
        assert_eq!(session.snapshot(1000).values["frequency"], "123.450");
        assert_eq!(session.snapshot(1000).age_ms, Some(1000));
        assert_eq!(session.snapshot(1000).epoch, epoch);
        session.feed(&frame(Some(""), None, 1), 1001);
        assert!(!session.snapshot(1001).mission_active);
        assert!(session.snapshot(1001).values.is_empty());
        assert!(session.snapshot(1001).epoch > epoch);
    }
    #[test]
    fn frozen_full_and_sparse_exports_cannot_rearm_after_timeout_without_an_advancing_counter() {
        let fixture = Fixture::new();
        let mut session = ExportSession::new(&fixture.compiled());
        let frozen = frame(Some("A-10C"), Some(b"251.000 "), 7);
        session.feed(&frozen, 0);
        assert!(session.snapshot(0).fresh);
        session.tick(1501);
        let epoch = session.snapshot(1501).epoch;
        assert_eq!(session.snapshot(1501).counter, Some(7));
        // Missing name bytes are not a shutdown signal and cannot discard the
        // remembered counter, allowing a subsequent full duplicate to revive.
        session.feed(&frame(None, None, 7), 1502);
        for time in [1503, 1600, 3100, 5000] {
            session.feed(&frozen, time);
            let snapshot = session.snapshot(time);
            assert!(!snapshot.fresh);
            assert!(snapshot.values.is_empty());
            assert_eq!(snapshot.counter, Some(7));
            assert_eq!(snapshot.epoch, epoch);
        }
        session.feed(&frame(Some("A-10C"), Some(b"300.000 "), 8), 5001);
        assert!(session.snapshot(5001).fresh);
        assert_eq!(session.snapshot(5001).values["frequency"], "300.000");
        assert_eq!(session.snapshot(5001).epoch, epoch);
    }
    #[test]
    fn export_staleness_and_counter_regression_require_reconstructed_current_context() {
        let fixture = Fixture::new();
        let mut session = ExportSession::new(&fixture.compiled());
        session.feed(&frame(Some("A-10C"), Some(b"123.450 "), 255), 0);
        let epoch = session.snapshot(0).epoch;
        session.feed(&frame(None, None, 0), 100);
        assert_eq!(session.snapshot(100).epoch, epoch);
        session.feed(&frame(None, Some(b"900.000 "), 255), 101);
        assert!(!session.snapshot(101).fresh);
        assert!(session.snapshot(101).epoch > epoch);
        session.feed(&frame(None, Some(b"225.000 "), 1), 102);
        assert!(!session.snapshot(102).fresh);
        session.feed(&frame(Some("A-10C_2"), Some(b"225.000 "), 2), 103);
        assert!(session.snapshot(103).fresh);
        let epoch = session.snapshot(103).epoch;
        session.tick(1604);
        assert!(!session.snapshot(1604).fresh);
        assert!(session.snapshot(1604).values.is_empty());
        assert!(session.snapshot(1604).epoch > epoch);
        let epoch = session.snapshot(1604).epoch;
        session.tick(1605);
        assert_eq!(session.snapshot(1605).epoch, epoch);
        session.feed(&frame(None, Some(b"300.000 "), 3), 1606);
        assert!(!session.snapshot(1606).fresh);
        session.feed(&frame(Some("OtherAircraft"), Some(b"400.000 "), 4), 1607);
        assert!(!session.snapshot(1607).mission_active);
        session.feed(&frame(Some("A-10C"), None, 5), 1608);
        assert!(session.snapshot(1608).fresh);
        assert!(session.snapshot(1608).values.is_empty());
        session.reset_stream(1609);
        assert_eq!(session.snapshot(1609).aircraft, None);
        assert_eq!(session.snapshot(1609).counter, None);
    }
    #[test]
    fn malformed_or_missing_source_bytes_stay_unknown_and_new_context_never_inherits_old_outputs() {
        let fixture = Fixture::new();
        let compiled = fixture.compiled();
        let mut session = ExportSession::new(&compiled);
        let mut bytes = frame(Some("A-10C"), None, 1);
        session.feed(&bytes, 0);
        assert!(session.snapshot(0).fresh);
        assert!(session.snapshot(0).values.is_empty());
        bytes = vec![0x55; 4];
        block(&mut bytes, 8208, b"123.");
        block(&mut bytes, 0xfffe, &[2, 0]);
        session.feed(&bytes, 1);
        assert!(!session.snapshot(1).values.contains_key("frequency"));
        bytes = vec![0x55; 4];
        block(&mut bytes, 8212, b"450 ");
        block(&mut bytes, 0xfffe, &[3, 0]);
        session.feed(&bytes, 2);
        assert_eq!(session.snapshot(2).values["frequency"], "123.450");
        session.feed(&[0x55, 0x55, 0x55, 0x55, 1, 0, 2, 0], 3);
        assert_eq!(session.snapshot(3).malformed_frames, 1);
        assert!(session.snapshot(3).values.is_empty());
        session.feed(&frame(Some("A-10C"), Some(&[0xff; 8]), 4), 4);
        assert!(session.snapshot(4).fresh);
        assert!(!session.snapshot(4).values.contains_key("frequency"));
        let mut memory = Memory::new();
        memory.apply(&[Block {
            address: 8192,
            data: vec![0xff, 0xff],
        }]);
        assert_eq!(compiled.outputs[3].source.read(&memory), Some(json!(65535)));
        let bounded = OutputSource::Integer(
            IntegerOutput::parse(
                &json!({"type":"integer","address":8194,"mask":31,"shift_by":0,"max_value":20}),
            )
            .unwrap(),
        );
        memory.apply(&[Block {
            address: 8194,
            data: vec![31, 0],
        }]);
        assert_eq!(bounded.read(&memory), None);
    }
    #[test]
    fn other_installed_aircraft_use_supported_metadata_without_master_caution_controls() {
        let fixture = Fixture::new();
        let doc = json!({"Test":{"TEST_ACK":{"identifier":"TEST_ACK","description":"Acknowledgement","inputs":[{"interface":"set_state","max_value":1}],"outputs":[]},
            "TEST_LAMP":{"identifier":"TEST_LAMP","description":"Lamp","inputs":[],"outputs":[{"type":"integer","address":512,"mask":1,"shift_by":0,"max_value":1}]}}});
        fs::write(fixture.0.join("SyntheticJet.json"), doc.to_string()).unwrap();
        fs::write(
            fixture.0.join("AircraftAliases.json"),
            json!({"SyntheticJet":["SyntheticJet"],"SyntheticJet_2":["SyntheticJet"]}).to_string(),
        )
        .unwrap();
        let mut config = fixture.config();
        config["aircraftModule"] = json!("SyntheticJet");
        config["inputs"] = json!([config["inputs"][0]]);
        config["inputs"][0]["command"] = json!("TEST_ACK");
        config["outputs"] = json!([config["outputs"][0]]);
        config["outputs"][0]["control"] = json!("TEST_LAMP");
        let profile = Profile::parse(&config).unwrap();
        assert_eq!(profile.value()["aircraftModule"], "SyntheticJet");
        let compiled = CompiledProfile::load(profile).unwrap();
        assert!(compiled.catalogue.aircraft_names.contains("SyntheticJet_2"));
        assert!(matches!(
            compiled.outputs[0].source,
            OutputSource::Integer(_)
        ));
        let page =
            query_catalogue(&json!({"cataloguePath":fixture.0,"aircraftModule":"SyntheticJet"}))
                .unwrap();
        assert_eq!(page["aircraftModule"], "SyntheticJet");
        assert_eq!(page["total"], 2);
        config["aircraftModule"] = json!("MissingJet");
        assert!(CompiledProfile::load(Profile::parse(&config).unwrap()).is_err());
        assert!(Catalogue::load_module(&fixture.0, "../SyntheticJet").is_err());
    }
}
