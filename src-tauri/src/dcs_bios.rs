//! DCS-BIOS catalogue, bounded export stream and deterministic binding session.
//! Addresses come from the installed catalogue, never from board or aircraft names.
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    path::{Path, PathBuf},
};

const COMMAND: &str = "UFC_MASTER_CAUTION";
const FEEDBACK: &str = "MASTER_CAUTION";
pub(crate) const MAX_DOCUMENT: u64 = 8 * 1024 * 1024;
const MAX_FRAME: usize = 131_072;
const CONFIG_ERROR: &str = "Invalid DCS-BIOS configuration.";
const CATALOGUE_ERROR: &str = "The installed DCS-BIOS catalogue is incompatible or malformed.";

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub catalogue_path: PathBuf,
    pub gateway_id: String,
    pub module_id: String,
    pub button: usize,
    pub led_action_id: String,
    pub command_host: Ipv4Addr,
    pub command_port: u16,
    pub listen_address: Ipv4Addr,
    pub listen_port: u16,
    pub multicast_group: Option<Ipv4Addr>,
    pub multicast_interface: Ipv4Addr,
    pub freshness_ms: u64,
}
fn bounded_text(value: &Value, max: usize) -> Option<&str> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control))
}
fn identity(value: &str, length: usize) -> bool {
    value.len() == length
        && value.bytes().all(|b| b.is_ascii_hexdigit())
        && value.bytes().any(|b| b != b'0')
        && value.bytes().any(|b| b != b'f' && b != b'F')
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 31
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn address(value: &Value, key: &str, default: &str) -> Result<Ipv4Addr, String> {
    value
        .get(key)
        .map_or(Some(default), Value::as_str)
        .ok_or(CONFIG_ERROR)?
        .parse()
        .map_err(|_| CONFIG_ERROR.into())
}
fn port(value: &Value, key: &str, default: u16) -> Result<u16, String> {
    value
        .get(key)
        .map_or(Some(default as u64), Value::as_u64)
        .filter(|n| *n > 0 && *n <= u16::MAX as u64)
        .map(|n| n as u16)
        .ok_or(CONFIG_ERROR.into())
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Network {
    pub command_host: Ipv4Addr,
    pub command_port: u16,
    pub listen_address: Ipv4Addr,
    pub listen_port: u16,
    pub multicast_group: Option<Ipv4Addr>,
    pub multicast_interface: Ipv4Addr,
    pub freshness_ms: u64,
}
impl Network {
    pub fn parse(value: &Value) -> Result<Self, String> {
        let group = value
            .get("multicastGroup")
            .map_or(Some("239.255.50.10"), Value::as_str)
            .ok_or(CONFIG_ERROR)?;
        let multicast_group = if group.is_empty() {
            None
        } else {
            let group: Ipv4Addr = group.parse().map_err(|_| CONFIG_ERROR)?;
            if !group.is_multicast() {
                return Err(CONFIG_ERROR.into());
            }
            Some(group)
        };
        let command_host = address(value, "commandHost", "127.0.0.1")?;
        let listen_address = address(value, "listenAddress", "0.0.0.0")?;
        let multicast_interface = address(value, "multicastInterface", "127.0.0.1")?;
        if command_host.is_unspecified()
            || command_host.is_multicast()
            || command_host.is_broadcast()
            || listen_address.is_multicast()
            || listen_address.is_broadcast()
            || multicast_interface.is_multicast()
            || multicast_interface.is_broadcast()
        {
            return Err(CONFIG_ERROR.into());
        }
        let freshness_ms = value
            .get("freshnessMs")
            .map_or(Some(1500), Value::as_u64)
            .filter(|v| (100..=10_000).contains(v))
            .ok_or(CONFIG_ERROR)?;
        Ok(Self {
            command_host,
            command_port: port(value, "commandPort", 7778)?,
            listen_address,
            listen_port: port(value, "listenPort", 5010)?,
            multicast_group,
            multicast_interface,
            freshness_ms,
        })
    }
    pub fn value(&self) -> Value {
        json!({"commandHost":self.command_host.to_string(),"commandPort":self.command_port,
            "listenAddress":self.listen_address.to_string(),"listenPort":self.listen_port,
            "multicastGroup":self.multicast_group.map(|g|g.to_string()).unwrap_or_default(),
            "multicastInterface":self.multicast_interface.to_string(),"freshnessMs":self.freshness_ms})
    }
}
pub(crate) fn read_json(path: &Path, max: u64) -> Result<Value, String> {
    let file = fs::File::open(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > max {
        return Err(format!(
            "{} exceeds the DCS-BIOS document limit.",
            path.display()
        ));
    }
    serde_json::from_slice(&bytes).map_err(|_| format!("{} is not valid JSON.", path.display()))
}
impl Config {
    pub fn parse(value: &Value) -> Result<Self, String> {
        if !value.is_object() || value["schemaVersion"] != 1 {
            return Err(CONFIG_ERROR.into());
        }
        let path = bounded_text(&value["cataloguePath"], 4096).ok_or(CONFIG_ERROR)?;
        let gateway = bounded_text(&value["gatewayId"], 30).ok_or(CONFIG_ERROR)?;
        let module = bounded_text(&value["moduleId"], 32).ok_or(CONFIG_ERROR)?;
        let led = bounded_text(&value["ledActionId"], 31).ok_or(CONFIG_ERROR)?;
        let button = value["button"]
            .as_u64()
            .filter(|v| *v < 128)
            .ok_or(CONFIG_ERROR)? as usize;
        if !Path::new(path).is_absolute()
            || !gateway
                .strip_prefix("stm32:")
                .is_some_and(|s| identity(s, 24))
            || !identity(module, 32)
            || !identifier(led)
        {
            return Err(CONFIG_ERROR.into());
        }
        let network = Network::parse(value)?;
        Ok(Self {
            catalogue_path: path.into(),
            gateway_id: gateway.to_ascii_lowercase(),
            module_id: module.to_ascii_lowercase(),
            button,
            led_action_id: led.into(),
            command_host: network.command_host,
            command_port: network.command_port,
            listen_address: network.listen_address,
            listen_port: network.listen_port,
            multicast_group: network.multicast_group,
            multicast_interface: network.multicast_interface,
            freshness_ms: network.freshness_ms,
        })
    }
    pub fn value(&self) -> Value {
        json!({"schemaVersion":1,"cataloguePath":self.catalogue_path,"gatewayId":self.gateway_id,
            "moduleId":self.module_id,"button":self.button,"ledActionId":self.led_action_id,
            "commandHost":self.command_host.to_string(),"commandPort":self.command_port,
            "listenAddress":self.listen_address.to_string(),"listenPort":self.listen_port,
            "multicastGroup":self.multicast_group.map(|g|g.to_string()).unwrap_or_default(),
            "multicastInterface":self.multicast_interface.to_string(),"freshnessMs":self.freshness_ms})
    }
    pub(crate) fn network(&self) -> Network {
        Network {
            command_host: self.command_host,
            command_port: self.command_port,
            listen_address: self.listen_address,
            listen_port: self.listen_port,
            multicast_group: self.multicast_group,
            multicast_interface: self.multicast_interface,
            freshness_ms: self.freshness_ms,
        }
    }
    pub fn load(path: &Path) -> Result<Option<Self>, String> {
        match fs::metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
            Ok(_) => Self::parse(&read_json(path, 16_384)?).map(Some),
        }
    }
    /// The runtime validates the selected live device descriptor before starting.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        Self::parse(&self.value())?;
        Catalogue::load(&self.catalogue_path)?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or(CONFIG_ERROR)?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temporary = parent.join(format!(".dcs-bios-{}.tmp", uuid::Uuid::new_v4()));
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
                .and_then(|file| file.sync_all())
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
pub(crate) struct IntegerOutput {
    pub(crate) address: u16,
    mask: u16,
    shift: u8,
    pub(crate) maximum: u16,
}
impl IntegerOutput {
    pub(crate) fn parse(value: &Value) -> Result<Self, String> {
        let address = value["address"]
            .as_u64()
            .filter(|v| *v <= 65534 && *v % 2 == 0)
            .ok_or(CATALOGUE_ERROR)? as u16;
        let mask = value["mask"]
            .as_u64()
            .filter(|v| *v > 0 && *v <= 65535)
            .ok_or(CATALOGUE_ERROR)? as u16;
        let shift = value["shift_by"]
            .as_u64()
            .filter(|v| *v < 16)
            .ok_or(CATALOGUE_ERROR)? as u8;
        let maximum = value["max_value"]
            .as_u64()
            .filter(|v| *v > 0 && *v <= 65535)
            .ok_or(CATALOGUE_ERROR)? as u16;
        if value["type"] != "integer"
            || mask.trailing_zeros() != shift as u32
            || (mask >> shift) as u32 != ((1u32 << (16 - mask.leading_zeros() - shift as u32)) - 1)
            || maximum > mask >> shift
        {
            return Err(CATALOGUE_ERROR.into());
        }
        Ok(Self {
            address,
            mask,
            shift,
            maximum,
        })
    }
    pub(crate) fn read(&self, memory: &Memory) -> Option<u16> {
        let offset = self.address as usize;
        (memory.valid[offset] && memory.valid[offset + 1]).then(|| {
            (u16::from_le_bytes([memory.bytes[offset], memory.bytes[offset + 1]]) & self.mask)
                >> self.shift
        })
    }
    pub(crate) fn read_blocks(&self, blocks: &[Block]) -> Option<u16> {
        blocks.iter().rev().find_map(|block| {
            let offset = (self.address as usize).checked_sub(block.address as usize)?;
            let bytes = block.data.get(offset..offset + 2)?;
            Some((u16::from_le_bytes([bytes[0], bytes[1]]) & self.mask) >> self.shift)
        })
    }
}
pub(crate) fn control<'a>(document: &'a Value, id: &str) -> Result<&'a Value, String> {
    let categories = document.as_object().ok_or(CATALOGUE_ERROR)?;
    let mut found = None;
    let mut count = 0;
    for category in categories.values() {
        for (key, value) in category.as_object().ok_or(CATALOGUE_ERROR)? {
            count += 1;
            if count > 20_000 {
                return Err(CATALOGUE_ERROR.into());
            }
            if key == id {
                if found.is_some() || value["identifier"] != id {
                    return Err(CATALOGUE_ERROR.into());
                }
                found = Some(value);
            }
        }
    }
    found.ok_or_else(|| format!("The installed DCS-BIOS catalogue has no {id} control."))
}
fn sole_output(control: &Value) -> Result<&Value, String> {
    let outputs = control["outputs"]
        .as_array()
        .filter(|v| v.len() == 1)
        .ok_or(CATALOGUE_ERROR)?;
    Ok(&outputs[0])
}
#[derive(Clone, Debug)]
pub struct Catalogue {
    pub(crate) aircraft_names: HashSet<String>,
    pub(crate) aircraft_address: u16,
    pub(crate) aircraft_length: usize,
    feedback: IntegerOutput,
    pub(crate) counter: IntegerOutput,
}
impl Catalogue {
    pub fn load(path: &Path) -> Result<Self, String> {
        let metadata = |canonical: &str, legacy: &str| {
            let canonical = path.join(canonical);
            if canonical.exists() {
                read_json(&canonical, MAX_DOCUMENT)
            } else {
                read_json(&path.join(legacy), MAX_DOCUMENT)
            }
        };
        Self::from_documents(
            &read_json(&path.join("A-10C.json"), MAX_DOCUMENT)?,
            &metadata("MetadataStart.json", "MetaDataStart.json")?,
            &metadata("MetadataEnd.json", "MetaDataEnd.json")?,
            &read_json(&path.join("AircraftAliases.json"), MAX_DOCUMENT)?,
        )
    }
    pub(crate) fn load_module(path: &Path, module: &str) -> Result<Self, String> {
        if module.is_empty()
            || module.len() > 63
            || !module
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(CATALOGUE_ERROR.into());
        }
        read_json(&path.join(format!("{module}.json")), MAX_DOCUMENT)?;
        let metadata = |canonical: &str, legacy: &str| {
            let canonical = path.join(canonical);
            read_json(
                &if canonical.exists() {
                    canonical
                } else {
                    path.join(legacy)
                },
                MAX_DOCUMENT,
            )
        };
        Self::from_metadata(
            module,
            &metadata("MetadataStart.json", "MetaDataStart.json")?,
            &metadata("MetadataEnd.json", "MetaDataEnd.json")?,
            &read_json(&path.join("AircraftAliases.json"), MAX_DOCUMENT)?,
            None,
        )
    }
    pub fn from_documents(
        aircraft: &Value,
        start: &Value,
        end: &Value,
        aliases: &Value,
    ) -> Result<Self, String> {
        let command = control(aircraft, COMMAND)?;
        if !command["inputs"].as_array().is_some_and(|inputs| {
            inputs
                .iter()
                .any(|i| i["interface"] == "set_state" && i["max_value"] == 1)
        }) {
            return Err("UFC_MASTER_CAUTION must advertise the 0/1 set_state interface.".into());
        }
        let feedback = IntegerOutput::parse(sole_output(control(aircraft, FEEDBACK)?)?)?;
        if feedback.maximum != 1 || feedback.mask >> feedback.shift != 1 {
            return Err(CATALOGUE_ERROR.into());
        }
        Self::from_metadata("A-10C", start, end, aliases, Some(feedback))
    }
    fn from_metadata(
        module: &str,
        start: &Value,
        end: &Value,
        aliases: &Value,
        feedback: Option<IntegerOutput>,
    ) -> Result<Self, String> {
        let counter = IntegerOutput::parse(sole_output(control(end, "_UPDATE_COUNTER")?)?)?;
        if counter.maximum != 255
            || counter.address != 0xfffe
            || counter.mask != 255
            || counter.shift != 0
        {
            return Err(CATALOGUE_ERROR.into());
        }
        let name = sole_output(control(start, "_ACFT_NAME")?)?;
        let aircraft_address = name["address"]
            .as_u64()
            .filter(|n| *n <= 65534 && *n % 2 == 0)
            .ok_or(CATALOGUE_ERROR)? as u16;
        let aircraft_length = name["max_length"]
            .as_u64()
            .filter(|n| (2..=64).contains(n))
            .ok_or(CATALOGUE_ERROR)? as usize;
        if name["type"] != "string"
            || aircraft_address as usize + aircraft_length > counter.address as usize
            || feedback.as_ref().is_some_and(|f| {
                f.address == counter.address
                    || ((f.address as usize) < aircraft_address as usize + aircraft_length
                        && f.address as usize + 2 > aircraft_address as usize)
            })
        {
            return Err(CATALOGUE_ERROR.into());
        }
        let aliases = aliases
            .as_object()
            .filter(|a| a.len() <= 1000)
            .ok_or(CATALOGUE_ERROR)?;
        let mut aircraft_names = HashSet::new();
        for (name, modules) in aliases {
            if bounded_text(&json!(name), 63).is_none() {
                return Err(CATALOGUE_ERROR.into());
            }
            let modules = modules
                .as_array()
                .filter(|v| v.len() <= 32)
                .ok_or(CATALOGUE_ERROR)?;
            if !modules.iter().all(|m| bounded_text(m, 63).is_some()) {
                return Err(CATALOGUE_ERROR.into());
            }
            if modules.iter().any(|m| m == module) {
                aircraft_names.insert(name.clone());
            }
        }
        if !aircraft_names.contains(module) {
            return Err(CATALOGUE_ERROR.into());
        }
        Ok(Self {
            aircraft_names,
            aircraft_address,
            aircraft_length,
            feedback: feedback.unwrap_or(IntegerOutput {
                address: 0,
                mask: 1,
                shift: 0,
                maximum: 1,
            }),
            counter,
        })
    }
    pub(crate) fn output_only() -> Self {
        Self {
            aircraft_names: HashSet::new(),
            aircraft_address: 0,
            aircraft_length: 2,
            feedback: IntegerOutput {
                address: 2,
                mask: 1,
                shift: 0,
                maximum: 1,
            },
            counter: IntegerOutput {
                address: 65534,
                mask: 255,
                shift: 0,
                maximum: 255,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum LedBinding {
    Boolean,
    Mode,
}
impl LedBinding {
    pub fn values(&self, on: bool) -> Value {
        match self {
            Self::Boolean => json!([on]),
            Self::Mode => json!([if on { "on" } else { "off" }]),
        }
    }
}
pub fn validate_led(catalogue: &Value, action_id: &str) -> Result<LedBinding, String> {
    let action = catalogue["actions"]
        .as_array()
        .and_then(|a| a.iter().find(|a| a["id"] == action_id))
        .ok_or("The selected LED action is no longer advertised.")?;
    crate::actions::descriptor(action)?;
    if catalogue["status"] != "ready"
        || catalogue["catalogueRevision"]
            .as_u64()
            .is_none_or(|n| n == 0 || n > u32::MAX as u64)
        || action["semantics"] != "setter"
        || action["readable"] != true
        || action["parameters"].as_array().unwrap().len() != 1
    {
        return Err(
            "Select a readable LED setter with one boolean or on/off enum parameter.".into(),
        );
    }
    let parameter = &action["parameters"][0];
    match parameter["type"].as_str() {
        Some("boolean") => Ok(LedBinding::Boolean),
        Some("enum")
            if parameter["options"]
                .as_array()
                .unwrap()
                .iter()
                .any(|o| o == "on")
                && parameter["options"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|o| o == "off") =>
        {
            Ok(LedBinding::Mode)
        }
        _ => Err("Select a boolean LED or an LED mode action advertising on and off.".into()),
    }
}

#[derive(Debug)]
pub(crate) struct Block {
    pub(crate) address: u16,
    pub(crate) data: Vec<u8>,
}
#[derive(Debug)]
pub(crate) enum ExportEvent {
    Frame(Vec<Block>),
    Reset,
}
/// Sync detection retains a possible marker across datagram boundaries. No
/// address-space allocation or block can grow beyond the 16-bit wire bounds.
pub(crate) struct Parser {
    counter_address: u16,
    sync: usize,
    active: bool,
    header: Vec<u8>,
    address: u16,
    remaining: usize,
    data: Vec<u8>,
    blocks: Vec<Block>,
    frame_bytes: usize,
}
impl Parser {
    pub(crate) fn new(counter_address: u16) -> Self {
        Self {
            counter_address,
            sync: 0,
            active: false,
            header: Vec::new(),
            address: 0,
            remaining: 0,
            data: Vec::new(),
            blocks: Vec::new(),
            frame_bytes: 0,
        }
    }
    pub(crate) fn discard(&mut self) {
        self.active = false;
        self.header.clear();
        self.remaining = 0;
        self.data.clear();
        self.blocks.clear();
        self.frame_bytes = 0;
    }
    fn byte(&mut self, byte: u8, events: &mut Vec<ExportEvent>) {
        if !self.active {
            return;
        }
        if self.remaining > 0 {
            self.data.push(byte);
            self.remaining -= 1;
            if self.remaining == 0 {
                let complete = self.address <= self.counter_address
                    && self.address as usize + self.data.len() >= self.counter_address as usize + 2;
                self.blocks.push(Block {
                    address: self.address,
                    data: std::mem::take(&mut self.data),
                });
                if complete {
                    events.push(ExportEvent::Frame(std::mem::take(&mut self.blocks)));
                    self.discard();
                }
            }
        } else {
            self.header.push(byte);
            if self.header.len() == 4 {
                self.address = u16::from_le_bytes([self.header[0], self.header[1]]);
                let length = u16::from_le_bytes([self.header[2], self.header[3]]) as usize;
                self.header.clear();
                self.frame_bytes += length + 4;
                if length == 0
                    || length % 2 != 0
                    || self.address % 2 != 0
                    || self.address as usize + length > 65536
                    || self.frame_bytes > MAX_FRAME
                    || self.blocks.len() >= 8192
                {
                    self.discard();
                    events.push(ExportEvent::Reset);
                } else {
                    self.remaining = length;
                }
            }
        }
    }
    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Vec<ExportEvent> {
        let mut events = Vec::new();
        for &byte in bytes {
            if byte == 0x55 {
                self.sync = (self.sync + 1).min(8);
                continue;
            }
            let run = std::mem::take(&mut self.sync);
            if run >= 4 {
                if run <= 7 {
                    for _ in 0..run - 4 {
                        self.byte(0x55, &mut events);
                    }
                }
                if self.active
                    && (!self.blocks.is_empty() || !self.header.is_empty() || self.remaining > 0)
                {
                    events.push(ExportEvent::Reset);
                }
                self.discard();
                self.active = true;
            } else {
                for _ in 0..run {
                    self.byte(0x55, &mut events);
                }
            }
            self.byte(byte, &mut events);
        }
        events
    }
}
#[derive(Clone)]
pub(crate) struct Memory {
    pub(crate) bytes: Vec<u8>,
    pub(crate) valid: Vec<bool>,
}
impl Memory {
    pub(crate) fn new() -> Self {
        Self {
            bytes: vec![0; 65536],
            valid: vec![false; 65536],
        }
    }
    pub(crate) fn clear(&mut self) {
        self.valid.fill(false);
    }
    pub(crate) fn apply(&mut self, blocks: &[Block]) {
        for block in blocks {
            let range = block.address as usize..block.address as usize + block.data.len();
            self.bytes[range.clone()].copy_from_slice(&block.data);
            self.valid[range].fill(true);
        }
    }
    pub(crate) fn string(&self, address: u16, length: usize) -> Option<String> {
        let start = address as usize;
        if !self.valid[start..start + length].iter().all(|v| *v) {
            return None;
        }
        let bytes = &self.bytes[start..start + length];
        // DCS-BIOS StringAllocation writes a fixed byte width, padding unused
        // characters with spaces. A NUL is accepted for older peers, not required.
        let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
        let text = std::str::from_utf8(&bytes[..end]).ok()?;
        (!text.chars().any(char::is_control)).then(|| text.trim_end_matches(' ').to_owned())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Intent {
    Command(String),
    Led(bool),
}
pub struct Session {
    config: Config,
    catalogue: Catalogue,
    parser: Parser,
    memory: Memory,
    aircraft: Option<String>,
    counter: Option<u16>,
    last_frame: Option<u64>,
    feedback_seen: bool,
    baseline: Option<(u64, u32, u32)>,
    held: bool,
    armed: bool,
    pressed_sent: bool,
    desired_led: Option<bool>,
    led_value: Option<bool>,
    command_uncertain: bool,
    error: Option<String>,
    commands_sent: u64,
    malformed_frames: u64,
    event_overflow: u64,
}
impl Session {
    pub fn new(config: Config, catalogue: Catalogue) -> Self {
        let parser = Parser::new(catalogue.counter.address);
        Self {
            config,
            catalogue,
            parser,
            memory: Memory::new(),
            aircraft: None,
            counter: None,
            last_frame: None,
            feedback_seen: false,
            baseline: None,
            held: false,
            armed: false,
            pressed_sent: false,
            desired_led: None,
            led_value: None,
            command_uncertain: false,
            error: None,
            commands_sent: 0,
            malformed_frames: 0,
            event_overflow: 0,
        }
    }
    fn mission_active(&self) -> bool {
        self.aircraft
            .as_ref()
            .is_some_and(|a| self.catalogue.aircraft_names.contains(a))
    }
    pub fn feedback_fresh(&self, now_ms: u64) -> bool {
        self.mission_active()
            && self.feedback_seen
            && self
                .last_frame
                .is_some_and(|t| now_ms.saturating_sub(t) <= self.config.freshness_ms)
    }
    pub fn needs_baseline(&self) -> bool {
        self.baseline.is_none()
    }
    /// Neutralize an attempted press once. Even an uncertain press may have
    /// reached DCS; this compensating release is never a replay of that press.
    pub fn release(&mut self) -> Vec<Intent> {
        if self.pressed_sent {
            self.pressed_sent = false;
            vec![Intent::Command(format!("{COMMAND} 0\n"))]
        } else {
            Vec::new()
        }
    }
    pub fn panel_unavailable(&mut self) -> Vec<Intent> {
        self.baseline = None;
        self.armed = false;
        self.release()
    }
    pub fn event_overflow(&mut self) -> Vec<Intent> {
        self.event_overflow += 1;
        self.panel_unavailable()
    }
    fn invalidate(&mut self) -> Vec<Intent> {
        self.memory.clear();
        self.counter = None;
        self.last_frame = None;
        self.feedback_seen = false;
        self.panel_unavailable()
    }
    pub fn reset_stream(&mut self, now_ms: u64) -> Vec<Intent> {
        self.parser.discard();
        self.parser.sync = 0;
        self.aircraft = None;
        let mut intents = self.invalidate();
        intents.extend(self.update_led(now_ms));
        intents
    }
    fn update_led(&mut self, now_ms: u64) -> Vec<Intent> {
        let on =
            self.feedback_fresh(now_ms) && self.catalogue.feedback.read(&self.memory) == Some(1);
        if self.desired_led == Some(on) {
            Vec::new()
        } else {
            self.desired_led = Some(on);
            vec![Intent::Led(on)]
        }
    }
    pub fn refresh_led(&mut self, now_ms: u64) -> Vec<Intent> {
        self.desired_led = None;
        self.update_led(now_ms)
    }
    pub fn feed(&mut self, bytes: &[u8], now_ms: u64) -> Vec<Intent> {
        let mut intents = self.tick(now_ms);
        for event in self.parser.feed(bytes) {
            match event {
                ExportEvent::Reset => {
                    self.malformed_frames += 1;
                    self.aircraft = None;
                    intents.extend(self.invalidate());
                }
                ExportEvent::Frame(blocks) => {
                    let was_fresh = self.feedback_fresh(now_ms);
                    let Some(counter) = self.catalogue.counter.read_blocks(&blocks) else {
                        self.malformed_frames += 1;
                        self.aircraft = None;
                        intents.extend(self.invalidate());
                        intents.extend(self.update_led(now_ms));
                        continue;
                    };
                    let advance = self
                        .counter
                        .map(|previous| counter.wrapping_sub(previous) & 255);
                    if advance.is_some_and(|d| d >= 128) {
                        self.aircraft = None;
                        intents.extend(self.invalidate());
                        intents.extend(self.update_led(now_ms));
                        continue;
                    }
                    // Inspect a coherent candidate before committing writes. A
                    // repeated counter cannot overwrite current feedback or make
                    // an old datagram look fresh. Shutdown may publish an empty
                    // name without incrementing the counter, so still honor it.
                    let mut candidate = self.memory.clone();
                    candidate.apply(&blocks);
                    let observed_aircraft = candidate.string(
                        self.catalogue.aircraft_address,
                        self.catalogue.aircraft_length,
                    );
                    let aircraft = observed_aircraft
                        .clone()
                        .filter(|name| !name.is_empty() && name != "NONE");
                    if advance == Some(0) {
                        if self.aircraft.is_some()
                            && observed_aircraft.is_some()
                            && aircraft.is_none()
                        {
                            self.aircraft = None;
                            intents.extend(self.invalidate());
                        }
                        intents.extend(self.update_led(now_ms));
                        continue;
                    }
                    if aircraft != self.aircraft {
                        // Retain only writes from this complete update. Old aircraft
                        // values cannot be reinterpreted through a new memory map.
                        intents.extend(self.invalidate());
                        self.memory.apply(&blocks);
                        // Metadata has its own map; coherent unchanged name bytes
                        // may accompany partial string writes on aircraft changes.
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
                    self.feedback_seen = self.catalogue.feedback.read(&self.memory).is_some();
                    if !was_fresh && self.feedback_fresh(now_ms) {
                        intents.extend(self.panel_unavailable());
                    }
                }
            }
            intents.extend(self.update_led(now_ms));
        }
        intents
    }
    pub fn tick(&mut self, now_ms: u64) -> Vec<Intent> {
        let mut intents = Vec::new();
        if self
            .last_frame
            .is_some_and(|t| now_ms.saturating_sub(t) > self.config.freshness_ms)
        {
            // Remember the last accepted counter across silence. A frozen
            // exporter must not become fresh again by repeating that counter
            // after stale values and the input baseline have been cleared.
            let counter = self.counter;
            intents.extend(self.invalidate());
            self.counter = counter;
            self.parser.discard();
            self.parser.sync = 0;
        }
        intents.extend(self.update_led(now_ms));
        intents
    }
    pub fn baseline(
        &mut self,
        boot: u64,
        stream: u32,
        head: u32,
        pressed: bool,
        now_ms: u64,
    ) -> Vec<Intent> {
        let intents = self.release();
        self.baseline = Some((boot, stream, head));
        self.held = pressed;
        self.armed = !pressed && self.feedback_fresh(now_ms) && !self.command_uncertain;
        intents
    }
    pub fn event(&mut self, sequence: u32, pressed: bool, now_ms: u64) -> Vec<Intent> {
        let Some((boot, stream, previous)) = self.baseline else {
            return Vec::new();
        };
        let advance = sequence.wrapping_sub(previous);
        if advance == 0 || advance >= 0x80000000 {
            return Vec::new();
        }
        self.baseline = Some((boot, stream, sequence));
        if !self.feedback_fresh(now_ms) || self.command_uncertain {
            return self.panel_unavailable();
        }
        if pressed == self.held {
            return Vec::new();
        }
        self.held = pressed;
        if !pressed {
            self.armed = true;
            if self.pressed_sent {
                self.pressed_sent = false;
                return vec![Intent::Command(format!("{COMMAND} 0\n"))];
            }
        } else if self.armed {
            self.pressed_sent = true;
            return vec![Intent::Command(format!("{COMMAND} 1\n"))];
        }
        Vec::new()
    }
    pub fn command_result(&mut self, success: bool) -> Vec<Intent> {
        if success {
            self.commands_sent += 1;
            Vec::new()
        } else {
            self.command_uncertain = true;
            self.error=Some("The DCS-BIOS command could not be confirmed as sent. It may have applied and was not retried; stop and start the binding before sending more commands.".into());
            self.panel_unavailable()
        }
    }
    pub fn led_result(&mut self, on: bool, success: bool) {
        self.led_value = success.then_some(on);
    }
    pub fn state(&self, now_ms: u64) -> Value {
        let fresh = self.feedback_fresh(now_ms);
        json!({"configured":true,"running":true,"config":self.config.value(),"aircraft":self.aircraft,
            "missionActive":self.mission_active(),"feedbackFresh":fresh,
            "feedbackAgeMs":self.last_frame.map(|t|now_ms.saturating_sub(t)),
            "status":if self.command_uncertain {"error"} else if fresh {"active"} else if self.last_frame.is_some() || self.aircraft.is_some() {"stale"} else {"waiting"},
            "error":self.error,"eventOverflow":self.event_overflow,"commandsSent":self.commands_sent,
            "ledValue":self.led_value,"needsBaseline":self.needs_baseline(),"armed":self.armed&&fresh,
            "commandUncertain":self.command_uncertain,"malformedFrames":self.malformed_frames})
    }
}

pub struct Transport {
    receiver: UdpSocket,
    sender: UdpSocket,
    command: SocketAddrV4,
}
impl Transport {
    pub fn open(config: &Config) -> Result<Self, String> {
        Self::open_network(&config.network())
    }
    pub(crate) fn open_network(config: &Network) -> Result<Self, String> {
        let receiver = socket2::Socket::new(
            socket2::Domain::IPV4,
            socket2::Type::DGRAM,
            Some(socket2::Protocol::UDP),
        )
        .map_err(|e| e.to_string())?;
        receiver
            .set_reuse_address(true)
            .map_err(|e| e.to_string())?;
        receiver
            .bind(&SocketAddrV4::new(config.listen_address, config.listen_port).into())
            .map_err(|e| format!("Cannot listen for DCS-BIOS export: {e}"))?;
        if let Some(group) = config.multicast_group {
            receiver
                .join_multicast_v4(&group, &config.multicast_interface)
                .map_err(|e| format!("Cannot join DCS-BIOS multicast: {e}"))?;
        }
        receiver.set_nonblocking(true).map_err(|e| e.to_string())?;
        let sender = UdpSocket::bind(SocketAddrV4::new(
            if config.command_host.is_loopback() {
                Ipv4Addr::LOCALHOST
            } else {
                Ipv4Addr::UNSPECIFIED
            },
            0,
        ))
        .map_err(|e| e.to_string())?;
        Ok(Self {
            receiver: receiver.into(),
            sender,
            command: SocketAddrV4::new(config.command_host, config.command_port),
        })
    }
    pub fn local_addr(&self) -> Result<SocketAddr, String> {
        self.receiver.local_addr().map_err(|e| e.to_string())
    }
    pub fn receive(&self) -> Result<Option<Vec<u8>>, String> {
        let mut data = vec![0u8; 65536];
        match self.receiver.recv_from(&mut data) {
            Ok((count, source))
                if source.ip() == std::net::IpAddr::V4(*self.command.ip()) && count > 0 =>
            {
                data.truncate(count);
                Ok(Some(data))
            }
            Ok(_) => Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(format!("DCS-BIOS export receive failed: {e}")),
        }
    }
    pub fn send(&self, command: &str) -> Result<(), String> {
        if !matches!(command, "UFC_MASTER_CAUTION 0\n" | "UFC_MASTER_CAUTION 1\n") {
            return Err("Invalid DCS-BIOS master-caution command.".into());
        }
        self.send_bytes(command)
    }
    pub(crate) fn send_control(&self, command: &str, argument: &str) -> Result<(), String> {
        if command.is_empty()
            || command.len() > 64
            || !command
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || !(matches!(argument, "INC" | "DEC")
                || argument
                    .parse::<i32>()
                    .is_ok_and(|n| (-65535..=65535).contains(&n))
                    && argument.len() <= 6
                    && argument
                        .bytes()
                        .all(|b| b.is_ascii_digit() || b == b'+' || b == b'-'))
        {
            return Err("Invalid catalogue control command.".into());
        }
        self.send_bytes(&format!("{command} {argument}\n"))
    }
    fn send_bytes(&self, command: &str) -> Result<(), String> {
        let written = self
            .sender
            .send_to(command.as_bytes(), self.command)
            .map_err(|e| format!("DCS-BIOS command send failed: {e}"))?;
        if written != command.len() {
            return Err("DCS-BIOS command send was incomplete and will not be retried.".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dcs-bios")
    }
    fn configuration() -> Value {
        json!({"schemaVersion":1,"cataloguePath":fixtures(),
            "gatewayId":"stm32:0123456789abcdef01234567",
            "moduleId":"0123456789abcdef0123456789abcdef","button":0,"ledActionId":"led.set"})
    }
    fn documents() -> [Value; 4] {
        [
            "A-10C.json",
            "MetadataStart.json",
            "MetadataEnd.json",
            "AircraftAliases.json",
        ]
        .map(|name| read_json(&fixtures().join(name), MAX_DOCUMENT).unwrap())
    }
    fn catalogue(documents: &[Value; 4]) -> Result<Catalogue, String> {
        Catalogue::from_documents(&documents[0], &documents[1], &documents[2], &documents[3])
    }
    fn session() -> Session {
        Session::new(
            Config::parse(&configuration()).unwrap(),
            Catalogue::load(&fixtures()).unwrap(),
        )
    }
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../.research/dcs-bios-tests")
                .join(uuid::Uuid::new_v4().to_string());
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn write_block(bytes: &mut Vec<u8>, address: u16, data: &[u8]) {
        bytes.extend(address.to_le_bytes());
        bytes.extend((data.len() as u16).to_le_bytes());
        bytes.extend(data);
    }
    fn frame(name: Option<&str>, feedback: Option<bool>, counter: u8) -> Vec<u8> {
        let mut bytes = vec![0x55; 4];
        if let Some(name) = name {
            let mut data = [b' '; 24];
            data[..name.len()].copy_from_slice(name.as_bytes());
            write_block(&mut bytes, 0, &data);
        }
        if let Some(feedback) = feedback {
            write_block(&mut bytes, 0x1234, &[if feedback { 64 } else { 0 }, 0]);
        }
        write_block(&mut bytes, 0xfffe, &[counter, 0]);
        bytes
    }
    fn active(session: &mut Session) {
        assert!(session
            .feed(&frame(Some("A-10C"), Some(true), 1), 0)
            .contains(&Intent::Led(true)));
        assert!(session.feedback_fresh(0));
    }
    #[test]
    fn fixed_width_space_padded_exports_and_older_nul_strings_are_readable() {
        let mut s = session();
        assert!(s
            .feed(&frame(Some("A-10C"), Some(true), 1), 0)
            .contains(&Intent::Led(true)));
        assert_eq!(s.state(0)["aircraft"], "A-10C");
        let mut memory = Memory::new();
        memory.apply(&[Block {
            address: 100,
            data: b"251.000".to_vec(),
        }]);
        assert_eq!(memory.string(100, 7).as_deref(), Some("251.000"));
        memory.apply(&[Block {
            address: 110,
            data: b"A-10C\0  ".to_vec(),
        }]);
        assert_eq!(memory.string(110, 8).as_deref(), Some("A-10C"));
        assert!(
            memory.string(100, 8).is_none(),
            "Unknown trailing bytes cannot be guessed."
        );
    }
    fn led_catalogue() -> Value {
        json!({"status":"ready","catalogueRevision":1,"actions":[{
            "id":"led.set","label":"LED","endpoint":"led","semantics":"setter","readable":true,
            "parameters":[{"id":"enabled","label":"Enabled","type":"boolean"}]}]})
    }

    #[test]
    fn profile_defaults_round_trip_and_normalize_identity() {
        let mut value = configuration();
        value["gatewayId"] = json!("stm32:0123456789ABCDEF01234567");
        value["moduleId"] = json!("0123456789ABCDEF0123456789ABCDEF");
        let config = Config::parse(&value).unwrap();
        assert_eq!(config.command_host, Ipv4Addr::LOCALHOST);
        assert_eq!(
            (config.command_port, config.listen_port, config.freshness_ms),
            (7778, 5010, 1500)
        );
        assert_eq!(
            config.multicast_group,
            Some("239.255.50.10".parse().unwrap())
        );
        assert_eq!(config.gateway_id, "stm32:0123456789abcdef01234567");
        assert_eq!(Config::parse(&config.value()).unwrap(), config);
        value["button"] = json!(127);
        value["freshnessMs"] = json!(10000);
        value["multicastGroup"] = json!("");
        assert!(Config::parse(&value).unwrap().multicast_group.is_none());
    }
    #[test]
    fn profile_rejects_invalid_addresses_identities_and_bounds() {
        for (key, invalid) in [
            ("schemaVersion", json!(2)),
            ("cataloguePath", json!("relative/path")),
            ("cataloguePath", json!("/bad\npath")),
            ("gatewayId", json!("stm32:000000000000000000000000")),
            ("gatewayId", json!("stm32:ffffffffffffffffffffffff")),
            ("gatewayId", json!("other:0123456789abcdef01234567")),
            ("moduleId", json!("short")),
            ("moduleId", json!("ffffffffffffffffffffffffffffffff")),
            ("ledActionId", json!("bad action")),
            ("ledActionId", json!("")),
            ("button", json!(128)),
            ("button", json!(-1)),
            ("button", json!(0.5)),
            ("commandPort", json!(0)),
            ("listenPort", json!(65536)),
            ("listenPort", Value::Null),
            ("freshnessMs", json!(99)),
            ("freshnessMs", json!(10001)),
            ("freshnessMs", json!("1500")),
            ("commandHost", json!("::1")),
            ("commandHost", json!("0.0.0.0")),
            ("commandHost", json!("255.255.255.255")),
            ("commandHost", json!("239.255.50.10")),
            ("listenAddress", json!("239.255.50.10")),
            ("listenAddress", json!("255.255.255.255")),
            ("multicastInterface", json!("239.255.50.10")),
            ("multicastInterface", json!("bad")),
            ("multicastGroup", json!("127.0.0.1")),
            ("multicastGroup", json!("bad")),
            ("multicastGroup", Value::Null),
        ] {
            let mut value = configuration();
            value[key] = invalid;
            assert!(
                Config::parse(&value).is_err(),
                "accepted {key}: {}",
                value[key]
            );
        }
        assert!(Config::parse(&Value::Null).is_err());
    }
    #[test]
    fn profile_atomic_persistence_reports_corruption_and_preserves_existing_config() {
        let scratch = Scratch::new();
        let path = scratch.0.join("settings/dcs-bios.json");
        assert_eq!(Config::load(&path).unwrap(), None);
        let config = Config::parse(&configuration()).unwrap();
        config.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), Some(config.clone()));
        let mut changed = config.clone();
        changed.button = 1;
        changed.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), Some(changed));
        let mut invalid = config;
        invalid.catalogue_path = scratch.0.join("missing-catalogue");
        assert!(invalid.save(&path).is_err());
        assert_eq!(Config::load(&path).unwrap().unwrap().button, 1);
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
        fs::write(&path, "{broken").unwrap();
        assert!(Config::load(&path).is_err());
        fs::write(&path, configuration().to_string().repeat(100)).unwrap();
        assert!(Config::load(&path).is_err());
        fs::write(&path, "{}{}").unwrap();
        assert!(read_json(&path, 3).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    #[test]
    fn catalogue_resolves_installed_values_and_legacy_metadata_names() {
        let catalogue = Catalogue::load(&fixtures()).unwrap();
        assert_eq!(
            (
                catalogue.feedback.address,
                catalogue.feedback.mask,
                catalogue.feedback.shift
            ),
            (0x1234, 64, 6)
        );
        assert!(catalogue.aircraft_names.contains("A-10C_2"));
        assert!(!catalogue.aircraft_names.contains("OtherAircraft"));
        let scratch = Scratch::new();
        for name in ["A-10C.json", "AircraftAliases.json"] {
            fs::copy(fixtures().join(name), scratch.0.join(name)).unwrap();
        }
        fs::copy(
            fixtures().join("MetadataStart.json"),
            scratch.0.join("MetaDataStart.json"),
        )
        .unwrap();
        fs::copy(
            fixtures().join("MetadataEnd.json"),
            scratch.0.join("MetaDataEnd.json"),
        )
        .unwrap();
        assert!(Catalogue::load(&scratch.0).is_ok());
        fs::write(scratch.0.join("A-10C.json"), "{").unwrap();
        assert!(Catalogue::load(&scratch.0).is_err());
    }
    #[test]
    fn catalogue_requires_exact_named_momentary_control_and_boolean_output() {
        let mut docs = documents();
        docs[0]["UFC"][COMMAND]["inputs"] = json!([{"interface":"action"}]);
        assert!(catalogue(&docs).is_err());
        docs = documents();
        docs[0]["UFC"][FEEDBACK]["outputs"] = json!([]);
        assert!(catalogue(&docs).is_err());
        docs = documents();
        docs[0]["UFC"][FEEDBACK]["identifier"] = json!("other");
        assert!(catalogue(&docs).is_err());
        docs = documents();
        docs[0]["Duplicate"] = json!({FEEDBACK:docs[0]["UFC"][FEEDBACK]});
        assert!(catalogue(&docs).is_err());
        docs = documents();
        docs[0]["UFC"].as_object_mut().unwrap().remove(FEEDBACK);
        assert!(catalogue(&docs).unwrap_err().contains(FEEDBACK));
        docs = documents();
        docs[0]["UFC"] = json!(null);
        assert!(catalogue(&docs).is_err());
    }
    #[test]
    fn catalogue_rejects_malformed_masks_and_overlapping_metadata() {
        for (key, invalid) in [
            ("type", json!("string")),
            ("address", json!(1)),
            ("address", json!(65536)),
            ("address", json!(65534)),
            ("address", json!(0)),
            ("mask", json!(0)),
            ("mask", json!(65536)),
            ("mask", json!(160)),
            ("mask", json!(192)),
            ("shift_by", json!(16)),
            ("shift_by", json!(0)),
            ("max_value", json!(2)),
        ] {
            let mut docs = documents();
            docs[0]["UFC"][FEEDBACK]["outputs"][0][key] = invalid;
            assert!(catalogue(&docs).is_err(), "accepted feedback {key}");
        }
        for (key, invalid) in [
            ("type", json!("integer")),
            ("address", json!(1)),
            ("address", json!(65534)),
            ("max_length", json!(1)),
            ("max_length", json!(65)),
        ] {
            let mut docs = documents();
            docs[1]["Metadata"]["_ACFT_NAME"]["outputs"][0][key] = invalid;
            assert!(catalogue(&docs).is_err(), "accepted metadata {key}");
        }
        let mut docs = documents();
        docs[2]["Metadata"]["_UPDATE_COUNTER"]["outputs"][0]["mask"] = json!(65280);
        docs[2]["Metadata"]["_UPDATE_COUNTER"]["outputs"][0]["shift_by"] = json!(8);
        assert!(catalogue(&docs).is_err());
        docs = documents();
        docs[3] = json!({"A-10C":["Other"]});
        assert!(catalogue(&docs).is_err());
        docs[3] = json!({"A-10C":null});
        assert!(catalogue(&docs).is_err());
        docs[3] = json!({"A-10C":["A-10C",null]});
        assert!(catalogue(&docs).is_err());
        docs[3] = json!({"Bad\nName":["A-10C"],"A-10C":["A-10C"]});
        assert!(catalogue(&docs).is_err());
    }
    #[test]
    fn led_binding_accepts_boolean_and_mode_only_through_readable_descriptors() {
        let mut value = led_catalogue();
        let binding = validate_led(&value, "led.set").unwrap();
        assert_eq!(binding, LedBinding::Boolean);
        assert_eq!(binding.values(true), json!([true]));
        value["actions"][0]["parameters"][0] =
            json!({"id":"mode","label":"Mode","type":"enum","options":["off","on","auto"]});
        let binding = validate_led(&value, "led.set").unwrap();
        assert_eq!(binding, LedBinding::Mode);
        assert_eq!(binding.values(false), json!(["off"]));
        assert_eq!(binding.values(true), json!(["on"]));
        for (field, invalid) in [
            ("readable", json!(false)),
            ("semantics", json!("oneShot")),
            ("parameters", json!([])),
        ] {
            let mut changed = value.clone();
            changed["actions"][0][field] = invalid;
            assert!(validate_led(&changed, "led.set").is_err());
        }
        value["actions"][0]["parameters"][0]["options"] = json!(["on"]);
        assert!(validate_led(&value, "led.set").is_err());
        value["actions"][0]["parameters"][0]["type"] = json!("rgb");
        assert!(validate_led(&value, "led.set").is_err());
        value = led_catalogue();
        value["catalogueRevision"] = json!(0);
        assert!(validate_led(&value, "led.set").is_err());
        value["catalogueRevision"] = json!(1);
        value["status"] = json!("unsupported");
        assert!(validate_led(&value, "led.set").is_err());
        assert!(validate_led(&Value::Null, "led.set").is_err());
    }
    #[test]
    fn fragmented_sync_headers_payload_and_coalesced_updates_are_atomic() {
        let mut session = session();
        let bytes = frame(Some("A-10C"), Some(true), 1);
        for (index, byte) in bytes.iter().enumerate() {
            session.feed(&[*byte], 10);
            assert_eq!(session.feedback_fresh(10), index == bytes.len() - 1);
        }
        let mut updates = frame(None, Some(false), 2);
        updates.extend(frame(None, Some(true), 3));
        assert_eq!(
            session.feed(&updates, 20),
            vec![Intent::Led(false), Intent::Led(true)]
        );
        assert_eq!(session.state(20)["aircraft"], "A-10C");
    }
    #[test]
    fn sparse_exports_keep_confirmed_values_fresh_when_counter_advances() {
        let mut session = session();
        active(&mut session);
        assert!(session.feed(&frame(None, None, 2), 1000).is_empty());
        assert!(session.feedback_fresh(2000));
        assert_eq!(session.state(2000)["feedbackAgeMs"], 1000);
        assert_eq!(session.tick(2501), vec![Intent::Led(false)]);
        assert!(!session.feedback_fresh(2501));
        assert_eq!(session.state(2501)["status"], "stale");
    }
    #[test]
    fn complete_update_without_known_feedback_does_not_arm_or_turn_led_on() {
        let mut session = session();
        assert_eq!(
            session.feed(&frame(Some("A-10C"), None, 1), 0),
            vec![Intent::Led(false)]
        );
        session.baseline(1, 1, 0, false, 0);
        assert!(session.event(1, true, 0).is_empty());
        assert!(!session.feedback_fresh(0));
        assert_eq!(
            session.feed(&frame(None, Some(true), 2), 100),
            vec![Intent::Led(true)]
        );
        assert!(session.needs_baseline());
        assert!(session.event(2, false, 100).is_empty());
    }
    #[test]
    fn duplicate_counter_cannot_refresh_or_overwrite_feedback() {
        let mut session = session();
        active(&mut session);
        assert!(session.feed(&frame(None, Some(false), 1), 1000).is_empty());
        assert_eq!(session.state(1000)["feedbackAgeMs"], 1000);
        assert_eq!(session.tick(1501), vec![Intent::Led(false)]);
        assert!(!session.feedback_fresh(1501));
    }
    #[test]
    fn repeated_frozen_frames_stay_stale_until_the_counter_advances() {
        let mut s = session();
        active(&mut s);
        s.baseline(1, 1, 0, false, 0);
        for now in [1501, 1600, 1800, 3000, 4500] {
            s.feed(&frame(None, None, 1), now);
            s.feed(&frame(Some("A-10C"), Some(true), 1), now);
            assert!(!s.feedback_fresh(now));
            assert_eq!(s.state(now)["armed"], false);
        }
        assert!(s
            .feed(&frame(Some("A-10C"), Some(true), 2), 4501)
            .contains(&Intent::Led(true)));
        assert!(s.feedback_fresh(4501));
        assert!(s.needs_baseline());
    }
    #[test]
    fn counter_wraps_normally_and_backward_update_requires_fresh_recovery() {
        let mut session = session();
        session.feed(&frame(Some("A-10C"), Some(true), 255), 0);
        assert!(session.feed(&frame(None, None, 0), 10).is_empty());
        assert!(session.feedback_fresh(10));
        assert_eq!(
            session.feed(&frame(None, Some(false), 255), 20),
            vec![Intent::Led(false)]
        );
        assert!(session.needs_baseline());
        assert!(!session.feedback_fresh(20));
        session.feed(&frame(None, Some(true), 1), 30);
        assert!(!session.feedback_fresh(30));
        assert_eq!(
            session.feed(&frame(Some("A-10C"), Some(true), 2), 40),
            vec![Intent::Led(true)]
        );
        assert!(session.feedback_fresh(40));
    }
    #[test]
    fn shutdown_empty_name_with_duplicate_counter_deactivates_feedback() {
        let mut session = session();
        active(&mut session);
        assert_eq!(
            session.feed(&frame(Some(""), None, 1), 10),
            vec![Intent::Led(false)]
        );
        assert_eq!(session.state(10)["aircraft"], Value::Null);
        assert_eq!(session.state(10)["missionActive"], false);
        assert!(session.needs_baseline());
    }
    #[test]
    fn aircraft_changes_clear_feedback_and_discard_panel_history() {
        let mut session = session();
        active(&mut session);
        session.baseline(1, 1, 0, false, 0);
        assert_eq!(
            session.feed(&frame(Some("OtherAircraft"), Some(true), 2), 10),
            vec![Intent::Led(false)]
        );
        assert_eq!(session.state(10)["missionActive"], false);
        assert!(session.event(1, true, 10).is_empty());
        session.feed(&frame(Some("A-10C_2"), None, 3), 20);
        assert!(!session.feedback_fresh(20));
        assert_eq!(
            session.feed(&frame(None, Some(true), 4), 30),
            vec![Intent::Led(true)]
        );
        assert!(session.needs_baseline());
        assert!(session.event(2, false, 30).is_empty());
    }
    #[test]
    fn partial_aircraft_string_waits_for_complete_valid_utf8_and_termination() {
        let mut session = session();
        let mut bytes = vec![0x55; 4];
        write_block(&mut bytes, 0, b"A-10");
        write_block(&mut bytes, 0x1234, &[64, 0]);
        write_block(&mut bytes, 0xfffe, &[1, 0]);
        session.feed(&bytes, 0);
        assert!(!session.feedback_fresh(0));
        let mut bytes = vec![0x55; 4];
        let mut remaining = [b' '; 20];
        remaining[0] = b'C';
        remaining[1] = 0;
        write_block(&mut bytes, 4, &remaining);
        write_block(&mut bytes, 0x1234, &[64, 0]);
        write_block(&mut bytes, 0xfffe, &[2, 0]);
        assert_eq!(session.feed(&bytes, 10), vec![Intent::Led(true)]);
        for name in [[0xff; 24], [b'A'; 24]] {
            let mut bytes = vec![0x55; 4];
            write_block(&mut bytes, 0, &name);
            write_block(&mut bytes, 0xfffe, &[3, 0]);
            session.feed(&bytes, 20);
            assert!(!session.feedback_fresh(20));
        }
    }
    #[test]
    fn sync_recovery_rejects_partial_invalid_unaligned_and_overflow_blocks() {
        for header in [[0, 0, 0, 0], [0, 0, 1, 0], [1, 0, 2, 0], [254, 255, 4, 0]] {
            let mut session = session();
            active(&mut session);
            let mut bytes = vec![0x55; 4];
            bytes.extend(header);
            assert_eq!(session.feed(&bytes, 10), vec![Intent::Led(false)]);
            assert_eq!(session.state(10)["malformedFrames"], 1);
            assert!(session
                .feed(&frame(Some("A-10C"), Some(true), 2), 20)
                .contains(&Intent::Led(true)));
        }
        let mut session = session();
        active(&mut session);
        let mut broken = vec![0x55; 4];
        broken.extend([0, 0, 24, 0, b'A']);
        session.feed(&broken, 1);
        session.feed(&frame(Some("A-10C"), Some(true), 2), 2);
        assert_eq!(session.state(2)["malformedFrames"], 1);
        assert!(session.feedback_fresh(2));
    }
    #[test]
    fn parser_bounds_total_frame_size_and_recovers_from_long_sync_runs() {
        let mut session = session();
        let mut bytes = vec![0x55; 8];
        for _ in 0..8193 {
            write_block(&mut bytes, 0x2000, &[0, 0]);
        }
        session.feed(&bytes, 0);
        assert_eq!(session.state(0)["malformedFrames"], 1);
        assert!(session
            .feed(&frame(Some("A-10C"), Some(true), 1), 1)
            .contains(&Intent::Led(true)));
        let mut parser = Parser::new(0xfffe);
        let mut bytes = vec![0x55; 4];
        for _ in 0..3 {
            write_block(&mut bytes, 0, &vec![0; 65534]);
        }
        assert!(parser
            .feed(&bytes)
            .iter()
            .any(|event| matches!(event, ExportEvent::Reset)));
    }
    #[test]
    fn payload_trailing_sync_bytes_and_fragmented_next_sync_are_unambiguous() {
        let mut parser = Parser::new(0xfffe);
        let mut bytes = vec![0x55; 4];
        write_block(&mut bytes, 0xfffe, &[0x55, 0x55]);
        assert!(parser.feed(&bytes).is_empty());
        assert!(parser.feed(&[0x55, 0x55]).is_empty());
        let mut next = vec![0x55, 0x55];
        write_block(&mut next, 0xfffe, &[0x56, 0]);
        let events = parser.feed(&next);
        assert_eq!(events.len(), 2);
        match &events[0] {
            ExportEvent::Frame(blocks) => assert_eq!(blocks[0].data, vec![0x55, 0x55]),
            ExportEvent::Reset => panic!("lost valid trailing data"),
        }
    }
    #[test]
    fn short_press_release_remain_ordered_and_allow_other_control_sequence_gaps() {
        let mut session = session();
        active(&mut session);
        session.baseline(5, 6, 10, false, 0);
        assert_eq!(
            session.event(12, true, 10),
            vec![Intent::Command("UFC_MASTER_CAUTION 1\n".into())]
        );
        session.command_result(true);
        assert!(session.event(12, true, 10).is_empty());
        assert!(session.event(11, false, 10).is_empty());
        assert_eq!(
            session.event(15, false, 10),
            vec![Intent::Command("UFC_MASTER_CAUTION 0\n".into())]
        );
        session.command_result(true);
        assert!(session.event(16, false, 10).is_empty());
        assert_eq!(session.state(10)["commandsSent"], 2);
    }
    #[test]
    fn held_startup_and_recovery_require_release_before_arming() {
        let mut session = session();
        assert!(session.event(1, true, 0).is_empty());
        active(&mut session);
        session.baseline(1, 1, 10, true, 0);
        assert!(session.event(11, true, 1).is_empty());
        assert!(session.event(12, false, 2).is_empty());
        assert_eq!(
            session.event(13, true, 3),
            vec![Intent::Command("UFC_MASTER_CAUTION 1\n".into())]
        );
        assert_eq!(
            session.tick(1501),
            vec![
                Intent::Command("UFC_MASTER_CAUTION 0\n".into()),
                Intent::Led(false)
            ]
        );
        assert!(session.event(14, false, 1502).is_empty());
        session.feed(&frame(Some("A-10C"), Some(true), 2), 1510);
        assert!(session.event(15, true, 1511).is_empty());
        session.baseline(1, 1, 15, true, 1511);
        assert!(session.event(16, false, 1512).is_empty());
        assert_eq!(
            session.event(17, true, 1513),
            vec![Intent::Command("UFC_MASTER_CAUTION 1\n".into())]
        );
    }
    #[test]
    fn stale_event_and_route_or_ring_reset_do_not_replay_historical_commands() {
        let mut session = session();
        active(&mut session);
        session.baseline(1, 1, u32::MAX, false, 0);
        assert_eq!(session.event(0, true, 1).len(), 1);
        assert_eq!(
            session.panel_unavailable(),
            vec![Intent::Command("UFC_MASTER_CAUTION 0\n".into())]
        );
        assert!(session.event(1, false, 2).is_empty());
        session.baseline(2, 2, 1, true, 2);
        assert!(session.event(2, false, 2).is_empty());
        session.event_overflow();
        assert_eq!(session.state(2)["eventOverflow"], 1);
        assert!(session.needs_baseline());
        session.baseline(2, 2, 3, false, 2);
        assert!(session.event(4, true, 1501).is_empty());
        assert!(session.needs_baseline());
        assert_eq!(session.reset_stream(1501), vec![Intent::Led(false)]);
        assert_eq!(session.state(1501)["aircraft"], Value::Null);
    }
    #[test]
    fn uncertain_command_is_explicit_and_never_retried_until_new_session() {
        let mut session = session();
        active(&mut session);
        session.baseline(1, 1, 0, false, 0);
        assert_eq!(session.event(1, true, 1).len(), 1);
        assert_eq!(
            session.command_result(false),
            vec![Intent::Command("UFC_MASTER_CAUTION 0\n".into())]
        );
        assert!(session.command_result(false).is_empty());
        session.baseline(1, 1, 1, true, 1);
        assert!(session.event(2, false, 2).is_empty());
        assert!(session.event(3, true, 3).is_empty());
        assert_eq!(session.state(3)["commandUncertain"], true);
        assert_eq!(session.state(3)["status"], "error");
        assert_eq!(session.state(3)["commandsSent"], 0);
        assert!(session.state(3)["error"]
            .as_str()
            .unwrap()
            .contains("was not retried"));
    }
    #[test]
    fn led_acknowledgements_do_not_claim_electrical_feedback_and_failed_setter_is_not_replayed() {
        let mut session = session();
        active(&mut session);
        assert_eq!(session.state(0)["ledValue"], Value::Null);
        session.led_result(true, false);
        assert!(session.tick(1).is_empty());
        assert_eq!(session.state(1)["ledValue"], Value::Null);
        assert_eq!(session.refresh_led(1), vec![Intent::Led(true)]);
        session.led_result(true, true);
        assert_eq!(session.state(1)["ledValue"], true);
        assert_eq!(session.reset_stream(2), vec![Intent::Led(false)]);
        session.led_result(false, true);
        assert_eq!(session.state(2)["ledValue"], false);
    }
    #[test]
    fn every_disarm_neutralizes_an_attempted_press_once_and_new_baseline_does_not_repeat_it() {
        for reason in 0..7 {
            let mut session = session();
            active(&mut session);
            session.baseline(1, 1, 0, false, 0);
            assert_eq!(
                session.event(1, true, 1),
                vec![Intent::Command("UFC_MASTER_CAUTION 1\n".into())]
            );
            session.command_result(true);
            let cleanup = match reason {
                0 => session.panel_unavailable(),
                1 => session.event_overflow(),
                2 => session.release(),
                3 => session.reset_stream(2),
                4 => session.feed(&frame(Some("OtherAircraft"), None, 2), 2),
                5 => session.tick(1501),
                _ => session.baseline(2, 2, 100, true, 2),
            };
            assert_eq!(
                cleanup
                    .iter()
                    .filter(|intent| matches!(intent, Intent::Command(_)))
                    .collect::<Vec<_>>(),
                vec![&Intent::Command("UFC_MASTER_CAUTION 0\n".into())],
                "missing cleanup for disarm {reason}"
            );
            assert!(session.release().is_empty());
            assert!(session.panel_unavailable().is_empty());
            assert!(session.baseline(2, 2, 100, true, 2).is_empty());
        }
    }
    #[test]
    fn udp_unicast_receives_exports_and_sends_exact_named_commands() {
        let peer = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let reserve = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = reserve.local_addr().unwrap().port();
        drop(reserve);
        let mut value = configuration();
        value["commandPort"] = json!(peer.local_addr().unwrap().port());
        value["listenAddress"] = json!("127.0.0.1");
        value["listenPort"] = json!(port);
        value["multicastGroup"] = json!("");
        let config = Config::parse(&value).unwrap();
        let transport = Transport::open(&config).unwrap();
        assert_eq!(transport.local_addr().unwrap().port(), port);
        assert!(transport.receive().unwrap().is_none());
        transport.send("UFC_MASTER_CAUTION 1\n").unwrap();
        let mut bytes = [0; 128];
        let count = peer.recv(&mut bytes).unwrap();
        assert_eq!(&bytes[..count], b"UFC_MASTER_CAUTION 1\n");
        assert!(transport.send("UFC_MASTER_CAUTION TOGGLE\n").is_err());
        let update = frame(Some("A-10C"), Some(true), 1);
        peer.send_to(&update, transport.local_addr().unwrap())
            .unwrap();
        let start = std::time::Instant::now();
        let received = loop {
            if let Some(bytes) = transport.receive().unwrap() {
                break bytes;
            }
            assert!(start.elapsed() < Duration::from_secs(1));
            std::thread::yield_now();
        };
        assert_eq!(received, update);
        peer.send_to(&[], transport.local_addr().unwrap()).unwrap();
        assert!(transport.receive().unwrap().is_none());
        let mut remote = config.clone();
        remote.command_host = "127.0.0.2".parse().unwrap();
        drop(transport);
        let remote = Transport::open(&remote).unwrap();
        peer.send_to(&update, remote.local_addr().unwrap()).unwrap();
        assert!(remote.receive().unwrap().is_none());
    }
}
