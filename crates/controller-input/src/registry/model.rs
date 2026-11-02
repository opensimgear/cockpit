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
    fn same_interface(&self, other: &Self) -> bool {
        self.backend == other.backend
            && self.vendor_id == other.vendor_id
            && self.product_id == other.product_id
            && self.serial == other.serial
            && self.collection == other.collection
            && self.path == other.path
    }
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
    let mut controls = controls.iter().collect::<Vec<_>>();
    controls.sort_by(|a, b| a.id.cmp(&b.id));
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&controls).expect("serializable controls"))
    )
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedSource {
    pub id: String,
    pub identity: Identity,
    pub name: String,
    pub role: String,
}

/// One read-only descriptor capture under the registry lock. Values and event
/// cursors stay in the input service; inventory consumers cannot acquire input.
#[derive(Clone, Debug)]
pub struct ResourceDescriptor {
    pub saved: SavedSource,
    pub online: bool,
    pub generation: u64,
    pub baseline_ready: bool,
    pub controls: Vec<Control>,
    pub diagnostics: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ResourceInventory {
    pub session: String,
    pub backend_status: String,
    pub diagnostic: String,
    pub sources: Vec<ResourceDescriptor>,
}
#[derive(Clone)]
struct Source {
    saved: SavedSource,
    online: bool,
    generation: u64,
    sequence: u64,
    captured_us: u64,
    cursor: u64,
    controls: Vec<Control>,
    values: BTreeMap<String, Sample>,
    diagnostics: Vec<String>,
}
impl Source {
    fn ready(&self) -> bool {
        self.online
            && self.controls.iter().any(|c| c.kind != Kind::Diagnostic)
            && self
                .controls
                .iter()
                .filter(|c| c.kind != Kind::Diagnostic)
                .all(|c| self.values.contains_key(&c.id))
    }
    fn event(&self, reason: Reason) -> Event {
        Event {
            cursor: 0,
            source: self.saved.id.clone(),
            generation: self.generation,
            sequence: self.sequence.max(1),
            captured_us: 0,
            reason,
            values: self.values.clone(),
        }
    }
    fn summary(&self) -> Value {
        json!({"id":self.saved.id,"name":self.saved.name,"role":self.saved.role,
            "origin":"host-controller","backend":self.saved.identity.backend,
            "vendorId":self.saved.identity.vendor_id,"productId":self.saved.identity.product_id,
            "osgHid":self.saved.identity.vendor_id==0xcafe && self.saved.identity.product_id==0x4010,
            "online":self.online,"generation":self.generation.to_string(),"sequence":self.sequence.to_string(),
            "controlCount":self.controls.len(),"baselineReady":self.online && self.controls.iter().any(|c|c.kind!=Kind::Diagnostic)
                && self.controls.iter().filter(|c| c.kind!=Kind::Diagnostic).all(|c| self.values.contains_key(&c.id)),
            "identityConfidence":if self.saved.identity.serial.is_some() {"serial"} else {"local-path"}})
    }
}
