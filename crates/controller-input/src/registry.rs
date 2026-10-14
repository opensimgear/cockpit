//! A bounded host-controller inventory. OS paths are observations, never portable IDs.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_SOURCES: usize = 256;
pub const MAX_CONTROLS: usize = 256;
pub const PAGE_SIZE: usize = 32;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Identity {
    pub backend: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub serial: Option<String>,
    pub collection: String,
    pub fingerprint: String,
    pub path: String,
}
impl Identity {
    fn compatible(&self, other: &Self) -> bool {
        self.backend == other.backend
            && self.vendor_id == other.vendor_id
            && self.product_id == other.product_id
            && self.collection == other.collection
            && self.fingerprint == other.fingerprint
    }
    fn serial_match(&self, other: &Self) -> bool {
        self.compatible(other)
            && self.serial.as_ref().is_some_and(|s| !s.is_empty())
            && self.serial == other.serial
    }
    fn valid(&self) -> bool {
        !self.path.is_empty()
            && self.path.len() <= 2048
            && self.backend.len() <= 32
            && self.collection.len() <= 128
            && self.fingerprint.len() == 64
            && self
                .serial
                .as_ref()
                .is_none_or(|s| !s.is_empty() && s.len() <= 256)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Button,
    Axis,
    Hat,
    Diagnostic,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Control {
    pub id: String,
    pub kind: Kind,
    pub usage_page: u16,
    pub usage: u16,
    pub report_id: u8,
    pub collection: u16,
    pub data_index: u16,
    pub bits: u16,
    pub min: i64,
    pub max: i64,
    pub has_null: bool,
}
impl Control {
    pub fn new(page: u16, usage: u16, report: u8, collection: u16, index: u16) -> Self {
        let kind = match (page, usage) {
            (9, _) => Kind::Button,
            (1, 0x39) => Kind::Hat,
            (1, 0x30..=0x38) | (2, 0xba..=0xbb) | (2, 0xc4..=0xc6) | (2, 0xc8) => Kind::Axis,
            _ => Kind::Diagnostic,
        };
        Self {
            id: format!("hid:{page:04x}:{usage:04x}:{report}:{collection}:{index}"),
            kind,
            usage_page: page,
            usage,
            report_id: report,
            collection,
            data_index: index,
            bits: 1,
            min: 0,
            max: 1,
            has_null: false,
        }
    }
    /// Sign extend exactly the declared HID field width before normalization.
    pub fn decode(&self, raw: u32) -> Result<Sample, String> {
        if self.kind == Kind::Diagnostic {
            return Ok(Sample {
                native: i64::from(raw),
                normalized: None,
            });
        }
        if self.bits == 0 || self.bits > 32 || self.max < self.min {
            return Err("Invalid native control range.".into());
        }
        let mask = if self.bits == 32 {
            u32::MAX
        } else {
            (1u32 << self.bits) - 1
        };
        let value = raw & mask;
        let native = if self.min < 0 && value & (1 << (self.bits - 1)) != 0 {
            i64::from(value) - (1i64 << self.bits)
        } else {
            i64::from(value)
        };
        let in_range = native >= self.min && native <= self.max;
        let normalized = match self.kind {
            Kind::Diagnostic => None,
            Kind::Hat if !in_range && self.has_null => None,
            Kind::Hat if in_range && [4, 8].contains(&(self.max - self.min + 1)) => {
                Some((native - self.min) * (8 / (self.max - self.min + 1)))
            }
            Kind::Button if in_range => Some(i64::from(native != 0)),
            Kind::Axis if in_range && self.max > self.min => {
                let (low, span) = if self.min < 0 {
                    (-32768, 65535)
                } else {
                    (0, 32767)
                };
                Some(low + (native - self.min) * span / (self.max - self.min))
            }
            _ => {
                return Err(format!(
                    "Unsupported or out-of-range value for {}.",
                    self.id
                ))
            }
        };
        Ok(Sample { native, normalized })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Sample {
    pub native: i64,
    pub normalized: Option<i64>,
}
#[derive(Clone, Debug)]
pub struct Definition {
    pub identity: Identity,
    pub name: String,
    pub controls: Vec<Control>,
    pub diagnostics: Vec<String>,
}
pub fn fingerprint(controls: &[Control]) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(controls).expect("serializable controls"))
    )
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedSource {
    pub id: String,
    pub identity: Identity,
    pub name: String,
    pub role: String,
}
#[derive(Clone)]
struct Source {
    saved: SavedSource,
    online: bool,
    generation: u64,
    sequence: u64,
    controls: Vec<Control>,
    values: BTreeMap<String, Sample>,
    diagnostics: Vec<String>,
}
impl Source {
    fn summary(&self) -> Value {
        json!({"id":self.saved.id,"name":self.saved.name,"role":self.saved.role,
            "origin":"host-controller","backend":self.saved.identity.backend,
            "vendorId":self.saved.identity.vendor_id,"productId":self.saved.identity.product_id,
            "osgHid":self.saved.identity.vendor_id==0xcafe && self.saved.identity.product_id==0x4010,
            "online":self.online,"generation":self.generation.to_string(),"sequence":self.sequence.to_string(),
            "controlCount":self.controls.len(),"baselineReady":self.online && !self.controls.is_empty()
                && self.controls.iter().filter(|c| c.kind!=Kind::Diagnostic).all(|c| self.values.contains_key(&c.id)),
            "identityConfidence":if self.saved.identity.serial.is_some() {"serial"} else {"local-path"}})
    }
}
pub struct Registry {
    sources: BTreeMap<String, Source>,
    pub backend_status: String,
    pub diagnostic: String,
    pub session: String,
    pub dirty: bool,
}
impl Default for Registry {
    fn default() -> Self {
        Self {
            sources: BTreeMap::new(),
            backend_status: "starting".into(),
            diagnostic: String::new(),
            session: uuid::Uuid::new_v4().to_string(),
            dirty: false,
        }
    }
}
impl Registry {
    pub fn restore(&mut self, records: Vec<SavedSource>) -> Result<(), String> {
        if records.len() > MAX_SOURCES {
            return Err("Too many remembered controllers.".into());
        }
        let mut sources = BTreeMap::new();
        for saved in records {
            if uuid::Uuid::parse_str(&saved.id).is_err()
                || !saved.identity.valid()
                || !valid_label(&saved.name)
                || !valid_role(&saved.role)
                || sources.contains_key(&saved.id)
            {
                return Err("Invalid remembered controller identity or metadata.".into());
            }
            sources.insert(
                saved.id.clone(),
                Source {
                    saved,
                    online: false,
                    generation: 0,
                    sequence: 0,
                    controls: vec![],
                    values: BTreeMap::new(),
                    diagnostics: vec![],
                },
            );
        }
        self.sources = sources;
        Ok(())
    }
    pub fn saved(&self) -> Vec<SavedSource> {
        self.sources.values().map(|s| s.saved.clone()).collect()
    }
    pub fn connect(&mut self, mut device: Definition) -> Result<String, String> {
        if device.identity.vendor_id == 0x1234 && device.identity.product_id == 0xbead {
            return Err("vJoy virtual outputs are excluded from physical input.".into());
        }
        device.identity.fingerprint = fingerprint(&device.controls);
        let unique: BTreeSet<_> = device.controls.iter().map(|c| &c.id).collect();
        if !device.identity.valid()
            || device.controls.len() > MAX_CONTROLS
            || unique.len() != device.controls.len()
            || device.controls.iter().any(|c| {
                c.id.len() > 128
                    || (c.kind != Kind::Diagnostic && (c.bits == 0 || c.bits > 32 || c.max < c.min))
            })
        {
            return Err("Unsupported controller descriptor or inventory limit.".into());
        }
        let same_path = self
            .sources
            .iter()
            .find(|(_, s)| {
                s.saved.identity.compatible(&device.identity)
                    && s.saved.identity.serial == device.identity.serial
                    && s.saved.identity.path == device.identity.path
            })
            .map(|(id, _)| id.clone());
        let serials: Vec<_> = self
            .sources
            .iter()
            .filter(|(_, s)| s.saved.identity.serial_match(&device.identity))
            .map(|(id, s)| (id.clone(), s.online))
            .collect();
        // A live duplicate serial or multiple remembered candidates never merges automatically.
        let id = same_path.or_else(|| {
            if serials.len() == 1 && !serials[0].1 {
                Some(serials[0].0.clone())
            } else {
                None
            }
        });
        let id = if let Some(id) = id {
            id
        } else {
            if self.sources.len() >= MAX_SOURCES {
                return Err("Forget an offline controller before adding another.".into());
            }
            let id = uuid::Uuid::new_v4().to_string();
            let name = if valid_label(&device.name) {
                device.name
            } else {
                format!(
                    "Controller {:04X}:{:04X}",
                    device.identity.vendor_id, device.identity.product_id
                )
            };
            self.sources.insert(
                id.clone(),
                Source {
                    saved: SavedSource {
                        id: id.clone(),
                        identity: device.identity.clone(),
                        name,
                        role: "controller".into(),
                    },
                    online: false,
                    generation: 0,
                    sequence: 0,
                    controls: vec![],
                    values: BTreeMap::new(),
                    diagnostics: vec![],
                },
            );
            id
        };
        let source = self.sources.get_mut(&id).unwrap();
        if !source.online {
            source.generation += 1;
            source.sequence = 0;
            source.values.clear();
        }
        source.online = true;
        source.saved.identity = device.identity;
        source.controls = device.controls;
        source.diagnostics = device
            .diagnostics
            .into_iter()
            .take(16)
            .map(|s| s.chars().take(256).collect())
            .collect();
        self.dirty = true;
        Ok(id)
    }
    pub fn disconnect(&mut self, id: &str) {
        if let Some(s) = self.sources.get_mut(id) {
            s.online = false;
            s.values.clear();
        }
    }
    pub fn disconnect_all(&mut self) {
        for s in self.sources.values_mut() {
            s.online = false;
            s.values.clear();
        }
    }
    pub fn fault(&mut self, id: &str, message: &str) {
        if let Some(s) = self.sources.get_mut(id) {
            s.generation += 1;
            s.sequence = 0;
            s.values.clear();
            s.diagnostics = vec![message.chars().take(256).collect()];
        }
    }
    /// Commit a whole report under one registry lock, including implicit button releases.
    pub fn report(
        &mut self,
        id: &str,
        report: u8,
        data: &BTreeMap<u16, u32>,
    ) -> Result<(), String> {
        let s = self.sources.get_mut(id).ok_or("Unknown controller.")?;
        if !s.online {
            return Err("Controller is offline.".into());
        }
        let mut update = BTreeMap::new();
        for control in s.controls.iter().filter(|c| c.report_id == report) {
            let value = data
                .get(&control.data_index)
                .copied()
                .or_else(|| (control.kind == Kind::Button).then_some(0));
            if let Some(value) = value {
                update.insert(control.id.clone(), control.decode(value)?);
            }
        }
        s.values.extend(update);
        s.sequence += 1;
        Ok(())
    }
    pub fn list(&self, offset: usize) -> Value {
        json!({"session":self.session,"backendStatus":self.backend_status,"diagnostic":self.diagnostic,
            "sources":self.sources.values().skip(offset).take(PAGE_SIZE).map(Source::summary).collect::<Vec<_>>(),
            "total":self.sources.len(),"offset":offset,"limit":PAGE_SIZE})
    }
    pub fn inspect(&self, id: &str, offset: usize) -> Result<Value, String> {
        let s = self.sources.get(id).ok_or("Unknown controller.")?;
        Ok(
            json!({"source":s.summary(),"identity":s.saved.identity,"diagnostics":s.diagnostics,
            "controls":s.controls.iter().skip(offset).take(PAGE_SIZE).map(|c| json!({"capability":c,"value":s.values.get(&c.id)})).collect::<Vec<_>>(),
            "offset":offset,"total":s.controls.len(),"limit":PAGE_SIZE}),
        )
    }
    pub fn update(&mut self, id: &str, name: &str, role: &str) -> Result<(), String> {
        if !valid_label(name) || !valid_role(role) {
            return Err("Use a name of 1–80 characters and a supported role.".into());
        }
        let s = self.sources.get_mut(id).ok_or("Unknown controller.")?;
        s.saved.name = name.trim().into();
        s.saved.role = role.into();
        self.dirty = true;
        Ok(())
    }
    pub fn forget(&mut self, id: &str) -> Result<(), String> {
        if self.sources.get(id).ok_or("Unknown controller.")?.online {
            return Err("Disconnect the controller before forgetting it.".into());
        }
        self.sources.remove(id);
        self.dirty = true;
        Ok(())
    }
    /// Reassign only an offline record to a compatible live observation, preserving its local UUID.
    pub fn reassign(&mut self, remembered: &str, observed: &str) -> Result<(), String> {
        let previous = self
            .sources
            .get(remembered)
            .ok_or("Unknown remembered controller.")?;
        let next = self
            .sources
            .get(observed)
            .ok_or("Unknown observed controller.")?;
        if previous.online
            || !next.online
            || !previous.saved.identity.compatible(&next.saved.identity)
        {
            return Err(
                "Reassignment requires an offline identity and a compatible connected controller."
                    .into(),
            );
        }
        let mut replacement = next.clone();
        replacement.saved.id = remembered.into();
        replacement.saved.name = previous.saved.name.clone();
        replacement.saved.role = previous.saved.role.clone();
        replacement.generation = previous.generation + 1;
        replacement.sequence = 0;
        replacement.values.clear();
        self.sources.remove(observed);
        self.sources.insert(remembered.into(), replacement);
        self.dirty = true;
        Ok(())
    }
    pub fn id_for_path(&self, path: &str) -> Option<String> {
        self.sources
            .iter()
            .find(|(_, s)| s.online && s.saved.identity.path == path)
            .map(|(id, _)| id.clone())
    }
}
fn valid_label(value: &str) -> bool {
    !value.trim().is_empty() && value.chars().count() <= 80 && !value.chars().any(char::is_control)
}
fn valid_role(value: &str) -> bool {
    [
        "controller",
        "joystick",
        "wheelbase",
        "pedals",
        "throttle",
        "panel",
    ]
    .contains(&value)
}
