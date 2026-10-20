//! Developer-derived Assetto Corsa Rally shared-memory prefixes. Acquisition,
//! decoding and freshness are separate; fixture input uses the production parser.
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};

pub(crate) const PROVIDER: &str = "assetto-corsa-rally";
pub(crate) const MAX_PAGE: usize = 4096;
const CHANNELS: [&str; 5] = ["rpm", "speed_kmh", "gear", "max_rpm", "rev_ratio"];
pub(crate) const PHYSICS_SIZE: usize = 32;
// ACR publishes the current engine limit as an int32 in its extended physics
// page. Keep the original prefix valid for older sources and authored fixtures.
pub(crate) const PHYSICS_CURRENT_MAX_RPM_OFFSET: usize = 588;
pub(crate) const PHYSICS_EXTENDED_SIZE: usize = PHYSICS_CURRENT_MAX_RPM_OFFSET + 4;
pub(crate) const GRAPHICS_SIZE: usize = 8;
pub(crate) const STATIC_SIZE: usize = 416;

pub(crate) fn catalogue() -> Value {
    let channel = |id, label, kind, unit, minimum, maximum, derived| json!({"id":id,"label":label,"type":kind,"unit":unit,"minimum":minimum,"maximum":maximum,"derived":derived});
    json!({"provider":PROVIDER,"revision":1,"commands":[],"channels":[
        channel("rpm","Engine RPM","integer","rpm",0,100000,false),
        channel("speed_kmh","Speed","number","km/h",0,2000,false),
        channel("gear","Gear (-1 reverse, 0 neutral)","integer","",-1,31,true),
        channel("max_rpm","Maximum RPM","integer","rpm",1,100000,false),
        channel("rev_ratio","RPM / maximum RPM","number","ratio",0,100000,true)
    ]})
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceIdentity {
    pub id: String,
    pub epoch: u64,
}
impl SourceIdentity {
    fn valid(&self) -> bool {
        self.epoch > 0
            && !self.id.is_empty()
            && self.id.len() <= 64
            && self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    }
}
#[derive(Clone, Debug)]
pub(crate) struct RawSample {
    pub source: SourceIdentity,
    pub physics: Vec<u8>,
    pub graphics: Vec<u8>,
    pub statics: Vec<u8>,
}
impl RawSample {
    pub fn parse_json(value: &Value) -> Result<Self, String> {
        fn page(value: &Value) -> Result<Vec<u8>, String> {
            if let Some(bytes) = value.as_array() {
                if bytes.len() > MAX_PAGE {
                    return Err("Telemetry page exceeds 4096 bytes.".into());
                }
                return bytes
                    .iter()
                    .map(|v| {
                        v.as_u64()
                            .filter(|n| *n <= 255)
                            .map(|n| n as u8)
                            .ok_or_else(|| {
                                "Telemetry pages must contain integer bytes from 0 to 255.".into()
                            })
                    })
                    .collect();
            }
            let hex = value
                .as_str()
                .ok_or("Telemetry pages must be byte arrays.")?;
            if hex.len() > MAX_PAGE * 2
                || hex.len() % 2 != 0
                || !hex.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err("Invalid bounded telemetry hex page.".into());
            }
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    let text = std::str::from_utf8(pair).unwrap();
                    u8::from_str_radix(text, 16).map_err(|_| "Invalid telemetry hex.".into())
                })
                .collect()
        }
        if value.to_string().len() > 64 * 1024 {
            return Err("Telemetry input exceeds 64 KiB.".into());
        }
        let object = value
            .as_object()
            .ok_or("Telemetry input must be an object.")?;
        if object.keys().any(|key| {
            ![
                "sourceId",
                "sourceEpoch",
                "physics",
                "graphics",
                "staticPage",
            ]
            .contains(&key.as_str())
        }) {
            return Err("Unknown telemetry frame field.".into());
        }
        let id = value
            .get("sourceId")
            .map_or(Some("synthetic"), Value::as_str)
            .ok_or("Invalid telemetry source ID.")?;
        let epoch = if let Some(text) = value["sourceEpoch"].as_str() {
            if text.is_empty() || text.len() > 20 || !text.bytes().all(|b| b.is_ascii_digit()) {
                return Err("Invalid telemetry source epoch.".into());
            }
            text.parse::<u64>()
                .map_err(|_| "Invalid telemetry source epoch.")?
        } else {
            value["sourceEpoch"]
                .as_u64()
                .ok_or("Invalid telemetry source epoch.")?
        };
        let sample = Self {
            source: SourceIdentity {
                id: id.into(),
                epoch,
            },
            physics: page(&value["physics"])?,
            graphics: page(&value["graphics"])?,
            statics: page(&value["staticPage"])?,
        };
        parse(&sample)?;
        Ok(sample)
    }
}

pub(crate) trait Source: Send {
    fn poll(&mut self) -> Result<Option<RawSample>, String>;
    fn close(&mut self);
}
#[derive(Default)]
pub(crate) struct FixtureSource {
    queue: VecDeque<RawSample>,
}
impl FixtureSource {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn push(&mut self, sample: RawSample) -> Result<(), String> {
        parse(&sample)?;
        if self.queue.len() >= 256 {
            return Err("The telemetry fixture queue is full.".into());
        }
        self.queue.push_back(sample);
        Ok(())
    }
}
impl Source for FixtureSource {
    fn poll(&mut self) -> Result<Option<RawSample>, String> {
        Ok(self.queue.pop_front())
    }
    fn close(&mut self) {
        self.queue.clear();
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Context {
    pub shared_memory_version: String,
    pub game_version: String,
    pub car_model: String,
    pub track: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChannelStatus {
    Ready,
    Unknown,
    Stale,
}
impl ChannelStatus {
    fn name(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Unknown => "unknown",
            Self::Stale => "stale",
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Channel {
    pub value: Option<Value>,
    pub status: ChannelStatus,
    pub error: Option<String>,
}
impl Channel {
    fn ready(value: Value) -> Self {
        Self {
            value: Some(value),
            status: ChannelStatus::Ready,
            error: None,
        }
    }
    fn unknown(error: &str) -> Self {
        Self {
            value: None,
            status: ChannelStatus::Unknown,
            error: Some(error.into()),
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub epoch: u64,
    pub fresh: bool,
    pub age_ms: Option<u64>,
    pub status: String,
    pub source: Option<SourceIdentity>,
    pub context: Option<Context>,
    pub packet_id: Option<u32>,
    pub channels: BTreeMap<String, Channel>,
    pub error: Option<String>,
}
impl Snapshot {
    pub fn values(&self) -> BTreeMap<String, Value> {
        if !self.fresh {
            return BTreeMap::new();
        }
        self.channels
            .iter()
            .filter_map(|(id, channel)| {
                channel
                    .value
                    .as_ref()
                    .filter(|_| channel.status == ChannelStatus::Ready)
                    .map(|value| (id.clone(), value.clone()))
            })
            .collect()
    }
    pub fn value(&self) -> Value {
        let channels: BTreeMap<_, _> = self
            .channels
            .iter()
            .map(|(id, c)| {
                (
                    id,
                    json!({"value":c.value,"status":c.status.name(),"error":c.error}),
                )
            })
            .collect();
        let context=self.context.as_ref().map(|c|json!({"sharedMemoryVersion":c.shared_memory_version,"gameVersion":c.game_version,"carModel":c.car_model,"track":c.track}));
        let source = self
            .source
            .as_ref()
            .map(|s| json!({"id":s.id,"epoch":s.epoch.to_string()}));
        json!({"provider":PROVIDER,"epoch":self.epoch,"fresh":self.fresh,"ageMs":self.age_ms,"status":self.status,
            "source":source,"context":context,"packetId":self.packet_id,"channels":channels,"error":self.error})
    }
}

pub(crate) struct Reading {
    pub packet_id: u32,
    pub status: i32,
    pub context: Context,
    pub channels: BTreeMap<String, Channel>,
}
impl Reading {
    /// Live ACR capture (2026-10-14): graphics status and version strings
    /// remain zero while driving. This dialect has no trustworthy state flag;
    /// require car/stage identity and let Session qualify packet progression.
    pub(crate) fn sparse_status(&self) -> bool {
        self.status == 0
            && self.context.shared_memory_version.is_empty()
            && self.context.game_version.is_empty()
            && !self.context.car_model.is_empty()
            && !self.context.track.is_empty()
            && ["rpm", "gear", "speed_kmh"].iter().all(|id| {
                self.channels
                    .get(*id)
                    .is_some_and(|c| c.status == ChannelStatus::Ready)
            })
    }
    pub(crate) fn telemetry_active(&self) -> bool {
        self.status == 2 || self.sparse_status()
    }
}
pub(crate) fn parse(sample: &RawSample) -> Result<Reading, String> {
    for (name, page, minimum) in [
        ("physics", &sample.physics, PHYSICS_SIZE),
        ("graphics", &sample.graphics, GRAPHICS_SIZE),
        ("static", &sample.statics, STATIC_SIZE),
    ] {
        if page.len() < minimum || page.len() > MAX_PAGE {
            return Err(format!("Invalid {name} telemetry page length."));
        }
    }
    if !sample.source.valid() {
        return Err("Invalid telemetry source identity/epoch.".into());
    }
    fn integer(page: &[u8], offset: usize) -> i32 {
        i32::from_le_bytes(page[offset..offset + 4].try_into().unwrap())
    }
    fn text(page: &[u8], offset: usize, count: usize) -> Result<String, String> {
        let units: Vec<_> = page[offset..offset + count * 2]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|unit| *unit != 0)
            .collect();
        let text = String::from_utf16(&units).map_err(|_| "Invalid UTF-16 telemetry context.")?;
        if text.chars().any(char::is_control) {
            return Err("Telemetry context contains control characters.".into());
        }
        Ok(text.trim_end().into())
    }
    let status = integer(&sample.graphics, 4);
    if !(0..=3).contains(&status) {
        return Err("Unknown telemetry game status.".into());
    }
    let context = Context {
        shared_memory_version: text(&sample.statics, 0, 15)?,
        game_version: text(&sample.statics, 30, 15)?,
        car_model: text(&sample.statics, 68, 33)?,
        track: text(&sample.statics, 134, 33)?,
    };
    let rpm = integer(&sample.physics, 20);
    let gear = integer(&sample.physics, 16);
    // Prefer the current limit, which can change with the car's engine state.
    // Sparse ACR builds leave the static maximum at zero. Never infer a limit
    // from observed RPM or reinterpret invalid integer bytes as a float.
    let current_maximum = (sample.physics.len() >= PHYSICS_EXTENDED_SIZE)
        .then(|| integer(&sample.physics, PHYSICS_CURRENT_MAX_RPM_OFFSET))
        .filter(|maximum| (1..=100000).contains(maximum));
    let maximum = current_maximum.unwrap_or_else(|| integer(&sample.statics, 412));
    let speed = f32::from_le_bytes(sample.physics[28..32].try_into().unwrap()) as f64;
    let valid_rpm = (0..=100000).contains(&rpm);
    let valid_maximum = (1..=100000).contains(&maximum);
    let mut channels = BTreeMap::new();
    channels.insert(
        "rpm".into(),
        if valid_rpm {
            Channel::ready(json!(rpm))
        } else {
            Channel::unknown("Invalid engine RPM.")
        },
    );
    channels.insert(
        "gear".into(),
        if (0..=32).contains(&gear) {
            Channel::ready(json!(gear - 1))
        } else {
            Channel::unknown("Invalid raw gear.")
        },
    );
    channels.insert(
        "speed_kmh".into(),
        if speed.is_finite() && (0.0..=2000.0).contains(&speed) {
            Channel::ready(json!(speed))
        } else {
            Channel::unknown("Invalid vehicle speed.")
        },
    );
    channels.insert(
        "max_rpm".into(),
        if valid_maximum {
            Channel::ready(json!(maximum))
        } else {
            Channel::unknown("Maximum RPM is unavailable or invalid.")
        },
    );
    channels.insert(
        "rev_ratio".into(),
        if valid_rpm && valid_maximum {
            Channel::ready(json!(rpm as f64 / maximum as f64))
        } else {
            Channel::unknown("RPM ratio needs valid RPM and maximum RPM.")
        },
    );
    Ok(Reading {
        packet_id: integer(&sample.physics, 0) as u32,
        status,
        context,
        channels,
    })
}

pub(crate) struct Session {
    freshness_ms: u64,
    epoch: u64,
    source: Option<SourceIdentity>,
    context: Option<Context>,
    packet_id: Option<u32>,
    last_seen: Option<u64>,
    qualified: bool,
    live: bool,
    ever_fresh: bool,
    status: String,
    channels: BTreeMap<String, Channel>,
    error: Option<String>,
}
impl Session {
    pub fn new(freshness_ms: u64) -> Result<Self, String> {
        if !(100..=10000).contains(&freshness_ms) {
            return Err("Telemetry freshness must be 100..10000 milliseconds.".into());
        }
        Ok(Self {
            freshness_ms,
            epoch: 1,
            source: None,
            context: None,
            packet_id: None,
            last_seen: None,
            qualified: false,
            live: false,
            ever_fresh: false,
            status: "waiting".into(),
            channels: BTreeMap::new(),
            error: None,
        })
    }
    fn disarm(&mut self, status: &str) {
        self.epoch = self.epoch.saturating_add(1);
        self.qualified = false;
        self.channels.clear();
        self.status = status.into();
    }
    pub fn reset(&mut self) {
        self.disarm("waiting");
        self.source = None;
        self.context = None;
        self.packet_id = None;
        self.last_seen = None;
        self.live = false;
        self.ever_fresh = false;
        self.error = None;
    }
    pub fn tick(&mut self, now_ms: u64) -> Snapshot {
        if self.qualified
            && self
                .last_seen
                .is_some_and(|last| now_ms < last || now_ms - last > self.freshness_ms)
        {
            self.disarm("stale");
        }
        self.snapshot(now_ms)
    }
    pub fn feed(&mut self, sample: &RawSample, now_ms: u64) -> Snapshot {
        self.tick(now_ms);
        let reading = match parse(sample) {
            Ok(reading) => reading,
            Err(error) => return self.failed(error, now_ms),
        };
        let scope_changed = self.source.as_ref() != Some(&sample.source)
            || self.context.as_ref() != Some(&reading.context);
        if scope_changed {
            self.disarm(if reading.telemetry_active() {
                "waiting"
            } else {
                "inactive"
            });
            self.source = Some(sample.source.clone());
            self.live = reading.telemetry_active();
            self.context = Some(reading.context);
            self.packet_id = Some(reading.packet_id);
            self.last_seen = None;
            self.ever_fresh = false;
            self.error = None;
            return self.snapshot(now_ms);
        }
        if !reading.telemetry_active() {
            if self.live || self.qualified || self.status != "inactive" {
                self.disarm("inactive");
            }
            self.live = false;
            self.packet_id = Some(reading.packet_id);
            self.error = None;
            return self.snapshot(now_ms);
        }
        if !self.live {
            self.disarm("waiting");
            self.live = true;
            self.packet_id = Some(reading.packet_id);
            self.last_seen = None;
            self.error = None;
            return self.snapshot(now_ms);
        }
        let advance = reading.packet_id.wrapping_sub(self.packet_id.unwrap());
        if advance == 0 {
            return self.snapshot(now_ms);
        }
        if advance >= 0x80000000 {
            self.disarm("waiting");
            self.packet_id = Some(reading.packet_id);
            self.last_seen = None;
            self.error = None;
            return self.snapshot(now_ms);
        }
        self.packet_id = Some(reading.packet_id);
        self.last_seen = Some(now_ms);
        self.qualified = true;
        self.ever_fresh = true;
        self.channels = reading.channels;
        self.status = "active".into();
        self.error = None;
        self.snapshot(now_ms)
    }
    fn failed(&mut self, error: String, now_ms: u64) -> Snapshot {
        if self.status != "error" {
            self.disarm("error");
        }
        self.error = Some(error);
        self.snapshot(now_ms)
    }
    pub fn poll(&mut self, source: &mut dyn Source, now_ms: u64) -> Snapshot {
        match source.poll() {
            Ok(Some(sample)) => self.feed(&sample, now_ms),
            Ok(None) => self.tick(now_ms),
            Err(error) => {
                source.close();
                self.failed(error, now_ms)
            }
        }
    }
    pub fn snapshot(&self, now_ms: u64) -> Snapshot {
        let fresh = self.qualified
            && self.live
            && self
                .last_seen
                .is_some_and(|last| now_ms >= last && now_ms - last <= self.freshness_ms);
        let channels = CHANNELS
            .iter()
            .map(|id| {
                let channel = if fresh {
                    self.channels
                        .get(*id)
                        .cloned()
                        .unwrap_or_else(|| Channel::unknown("No current channel value."))
                } else {
                    Channel {
                        value: None,
                        status: if self.ever_fresh {
                            ChannelStatus::Stale
                        } else {
                            ChannelStatus::Unknown
                        },
                        error: None,
                    }
                };
                (id.to_string(), channel)
            })
            .collect();
        Snapshot {
            epoch: self.epoch,
            fresh,
            age_ms: self.last_seen.map(|last| now_ms.saturating_sub(last)),
            status: if !fresh && self.qualified {
                "stale".into()
            } else {
                self.status.clone()
            },
            source: self.source.clone(),
            context: self.context.clone(),
            packet_id: self.packet_id,
            channels,
            error: self.error.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> RawSample {
        RawSample {
            source: SourceIdentity {
                id: "fixture".into(),
                epoch: 1,
            },
            physics: include_bytes!("../../tests/fixtures/acr/live.physics.bin").to_vec(),
            graphics: include_bytes!("../../tests/fixtures/acr/live.graphics.bin").to_vec(),
            statics: include_bytes!("../../tests/fixtures/acr/live.static.bin").to_vec(),
        }
    }
    fn advance(sample: &mut RawSample, packet: u32) {
        sample.physics[..4].copy_from_slice(&packet.to_le_bytes());
    }
    #[test]
    fn observed_acr_sparse_status_qualifies_progression_without_inventing_metadata() {
        let mut s = sample();
        s.graphics[4..8].fill(0);
        s.statics[..60].fill(0);
        s.statics[412..416].fill(0);
        let mut session = Session::new(100).unwrap();
        assert!(parse(&s).unwrap().sparse_status());
        assert!(!session.feed(&s, 0).fresh);
        advance(&mut s, 11);
        let snapshot = session.feed(&s, 1);
        assert!(snapshot.fresh);
        assert_eq!(snapshot.values()["rpm"], json!(6300));
        assert!(!snapshot.values().contains_key("max_rpm"));
        assert!(!session.feed(&s, 102).fresh);
        for status in [1i32, 3] {
            s.graphics[4..8].copy_from_slice(&status.to_le_bytes());
            advance(&mut s, 12 + status as u32);
            assert!(!session.feed(&s, 103).fresh);
        }
        s.graphics[4..8].fill(0);
        s.statics[68..134].fill(0);
        assert!(!parse(&s).unwrap().telemetry_active());
        s = sample();
        s.graphics[4..8].fill(0);
        assert!(!parse(&s).unwrap().telemetry_active());
    }
    #[test]
    fn authored_prefixes_decode_real_fields_and_typed_catalogue() {
        let reading = parse(&sample()).unwrap();
        assert_eq!(reading.packet_id, 10);
        assert_eq!(reading.status, 2);
        assert_eq!(reading.channels["rpm"].value, Some(json!(6300)));
        assert_eq!(reading.channels["gear"].value, Some(json!(4)));
        assert_eq!(reading.channels["speed_kmh"].value, Some(json!(124.25)));
        assert_eq!(reading.channels["max_rpm"].value, Some(json!(7500)));
        assert_eq!(reading.channels["rev_ratio"].value, Some(json!(0.84)));
        assert_eq!(reading.context.car_model, "research-rally");
        let catalogue = catalogue();
        assert!(catalogue["commands"].as_array().unwrap().is_empty());
        assert_eq!(catalogue["channels"].as_array().unwrap().len(), 5);
        let mut s = sample();
        s.physics = include_bytes!("../../tests/fixtures/acr/reverse.physics.bin").to_vec();
        assert_eq!(parse(&s).unwrap().channels["gear"].value, Some(json!(-1)));
        s.physics = include_bytes!("../../tests/fixtures/acr/neutral.physics.bin").to_vec();
        assert_eq!(parse(&s).unwrap().channels["gear"].value, Some(json!(0)));
    }
    #[test]
    fn parser_rejects_short_oversized_invalid_identity_status_and_utf16() {
        for field in ["physics", "graphics", "static"] {
            let mut s = sample();
            let page = match field {
                "physics" => &mut s.physics,
                "graphics" => &mut s.graphics,
                _ => &mut s.statics,
            };
            page.clear();
            assert!(parse(&s).is_err());
        }
        let mut s = sample();
        s.physics.resize(MAX_PAGE + 1, 0);
        assert!(parse(&s).is_err());
        for id in ["", "invalid id", "\n", "a".repeat(65).as_str()] {
            let mut s = sample();
            s.source.id = id.into();
            assert!(parse(&s).is_err());
        }
        let mut s = sample();
        s.source.epoch = 0;
        assert!(parse(&s).is_err());
        s = sample();
        s.graphics[4..8].copy_from_slice(&99i32.to_le_bytes());
        assert!(parse(&s).is_err());
        s = sample();
        s.statics[68..70].copy_from_slice(&0xd800u16.to_le_bytes());
        assert!(parse(&s).is_err());
        s = sample();
        s.statics[68..70].copy_from_slice(&10u16.to_le_bytes());
        assert!(parse(&s).is_err());
    }
    #[test]
    fn invalid_numeric_fields_are_independently_unknown_and_maximum_is_never_guessed() {
        let mut s = sample();
        s.physics[20..24].copy_from_slice(&(-1i32).to_le_bytes());
        let reading = parse(&s).unwrap();
        assert_eq!(reading.channels["rpm"].status, ChannelStatus::Unknown);
        assert_eq!(reading.channels["gear"].status, ChannelStatus::Ready);
        assert!(reading.channels["rev_ratio"].value.is_none());
        s = sample();
        s.statics[412..416].fill(0);
        let reading = parse(&s).unwrap();
        assert!(reading.channels["max_rpm"].value.is_none());
        assert!(reading.channels["rev_ratio"].value.is_none());
        assert!(reading.channels["rpm"].value.is_some());
        s = sample();
        s.physics[16..20].copy_from_slice(&(-1i32).to_le_bytes());
        assert!(parse(&s).unwrap().channels["gear"].value.is_none());
        for speed in [f32::NAN, f32::INFINITY, -1.0, 2001.0] {
            s = sample();
            s.physics[28..32].copy_from_slice(&speed.to_le_bytes());
            assert!(parse(&s).unwrap().channels["speed_kmh"].value.is_none());
        }
    }
    #[test]
    fn current_physics_limit_overrides_static_maximum_and_supports_sparse_acr() {
        let mut s = sample();
        s.physics.resize(PHYSICS_EXTENDED_SIZE, 0);
        for maximum in [1i32, 8500, 100000] {
            s.physics[PHYSICS_CURRENT_MAX_RPM_OFFSET..PHYSICS_EXTENDED_SIZE]
                .copy_from_slice(&maximum.to_le_bytes());
            for static_maximum in [0i32, 7500] {
                s.statics[412..416].copy_from_slice(&static_maximum.to_le_bytes());
                let reading = parse(&s).unwrap();
                assert_eq!(reading.channels["max_rpm"].value, Some(json!(maximum)));
                assert_eq!(
                    reading.channels["rev_ratio"].value,
                    Some(json!(6300.0 / maximum as f64))
                );
            }
        }
        s.physics[PHYSICS_CURRENT_MAX_RPM_OFFSET..PHYSICS_EXTENDED_SIZE]
            .copy_from_slice(&8500i32.to_le_bytes());
        s.statics[..60].fill(0);
        s.graphics[4..8].fill(0);
        let reading = parse(&s).unwrap();
        assert!(reading.sparse_status());
        assert_eq!(reading.channels["max_rpm"].value, Some(json!(8500)));
    }
    #[test]
    fn invalid_current_limits_fall_back_only_to_valid_static_integer_limits() {
        let mut s = sample();
        s.physics.resize(PHYSICS_EXTENDED_SIZE, 0);
        for maximum in [
            0i32,
            -1,
            100001,
            i32::MIN,
            i32::MAX,
            8500f32.to_bits() as i32,
            f32::NAN.to_bits() as i32,
            f32::INFINITY.to_bits() as i32,
        ] {
            s.physics[PHYSICS_CURRENT_MAX_RPM_OFFSET..PHYSICS_EXTENDED_SIZE]
                .copy_from_slice(&maximum.to_le_bytes());
            s.statics[412..416].copy_from_slice(&7500i32.to_le_bytes());
            let reading = parse(&s).unwrap();
            assert_eq!(reading.channels["max_rpm"].value, Some(json!(7500)));
            assert_eq!(reading.channels["rev_ratio"].value, Some(json!(0.84)));
            for static_maximum in [0i32, -1, 100001] {
                s.statics[412..416].copy_from_slice(&static_maximum.to_le_bytes());
                let reading = parse(&s).unwrap();
                assert_eq!(reading.channels["max_rpm"].status, ChannelStatus::Unknown);
                assert!(reading.channels["max_rpm"].value.is_none());
                assert!(reading.channels["rev_ratio"].value.is_none());
                assert_eq!(reading.channels["rpm"].value, Some(json!(6300)));
            }
        }
    }
    #[test]
    fn incomplete_physics_extensions_use_legacy_static_maximum_without_partial_reads() {
        for length in PHYSICS_SIZE..PHYSICS_EXTENDED_SIZE {
            let mut s = sample();
            s.physics.resize(length, 0xff);
            let reading = parse(&s).unwrap();
            assert_eq!(reading.channels["max_rpm"].value, Some(json!(7500)));
            s.statics[412..416].fill(0);
            assert!(parse(&s).unwrap().channels["max_rpm"].value.is_none());
        }
    }
    #[test]
    fn current_limit_obeys_packet_freshness_and_is_not_retained_when_missing() {
        let mut s = sample();
        s.physics.resize(PHYSICS_EXTENDED_SIZE, 0);
        s.statics[412..416].fill(0);
        s.physics[PHYSICS_CURRENT_MAX_RPM_OFFSET..PHYSICS_EXTENDED_SIZE]
            .copy_from_slice(&8500i32.to_le_bytes());
        let mut session = Session::new(100).unwrap();
        assert!(!session.feed(&s, 0).fresh);
        advance(&mut s, 11);
        assert_eq!(session.feed(&s, 1).values()["max_rpm"], json!(8500));
        s.physics[PHYSICS_CURRENT_MAX_RPM_OFFSET..PHYSICS_EXTENDED_SIZE].fill(0);
        // A changed limit without a new physics packet does not update channels.
        assert_eq!(session.feed(&s, 50).values()["max_rpm"], json!(8500));
        advance(&mut s, 12);
        let missing = session.feed(&s, 51);
        assert!(missing.fresh);
        assert_eq!(missing.channels["max_rpm"].status, ChannelStatus::Unknown);
        assert!(!missing.values().contains_key("rev_ratio"));
        s.physics[PHYSICS_CURRENT_MAX_RPM_OFFSET..PHYSICS_EXTENDED_SIZE]
            .copy_from_slice(&9000i32.to_le_bytes());
        s.statics[68..70].copy_from_slice(&(b'X' as u16).to_le_bytes());
        assert!(!session.feed(&s, 52).fresh);
        advance(&mut s, 13);
        assert_eq!(session.feed(&s, 53).values()["max_rpm"], json!(9000));
        let stale = session.feed(&s, 154);
        assert!(!stale.fresh);
        assert_eq!(stale.channels["max_rpm"].status, ChannelStatus::Stale);
        assert!(stale.values().is_empty());
    }
    #[test]
    fn baseline_progression_freeze_timeout_recovery_and_u32_wrap_are_guarded() {
        assert!(Session::new(99).is_err());
        assert!(Session::new(10001).is_err());
        let mut session = Session::new(100).unwrap();
        let mut s = sample();
        assert!(!session.feed(&s, 0).fresh);
        assert!(!session.feed(&s, 1).fresh);
        advance(&mut s, 11);
        let fresh = session.feed(&s, 2);
        assert!(fresh.fresh);
        assert_eq!(fresh.values().len(), 5);
        let epoch = fresh.epoch;
        assert!(!session.tick(103).fresh);
        let stale_epoch = session.snapshot(103).epoch;
        assert!(stale_epoch > epoch);
        for time in [104, 205, 400] {
            s.physics[20..24].copy_from_slice(&9000i32.to_le_bytes());
            let frozen = session.feed(&s, time);
            assert!(!frozen.fresh);
            assert_eq!(frozen.packet_id, Some(11));
            assert_eq!(frozen.epoch, stale_epoch);
            assert!(frozen.values().is_empty());
        }
        advance(&mut s, 12);
        assert!(session.feed(&s, 401).fresh);
        advance(&mut s, 0);
        assert!(!session.feed(&s, 402).fresh);
        assert!(!session.feed(&s, 403).fresh);
        advance(&mut s, 1);
        assert!(session.feed(&s, 404).fresh);
        session.reset();
        advance(&mut s, u32::MAX);
        assert!(!session.feed(&s, 500).fresh);
        advance(&mut s, 0);
        assert!(session.feed(&s, 501).fresh);
        assert!(!session.tick(499).fresh);
    }
    #[test]
    fn pause_replay_off_context_and_process_changes_need_current_baselines() {
        let mut session = Session::new(1000).unwrap();
        let mut s = sample();
        session.feed(&s, 0);
        advance(&mut s, 11);
        session.feed(&s, 1);
        for status in [3i32, 1, 0] {
            s.graphics[4..8].copy_from_slice(&status.to_le_bytes());
            let inactive = session.feed(&s, 2);
            assert!(!inactive.fresh);
            assert_eq!(inactive.status, "inactive");
            let epoch = inactive.epoch;
            assert_eq!(session.feed(&s, 3).epoch, epoch);
        }
        s.graphics[4..8].copy_from_slice(&2i32.to_le_bytes());
        assert!(!session.feed(&s, 4).fresh);
        advance(&mut s, 12);
        assert!(session.feed(&s, 5).fresh);
        s.statics[134..136].copy_from_slice(&(b'X' as u16).to_le_bytes());
        assert!(!session.feed(&s, 6).fresh);
        advance(&mut s, 13);
        assert!(session.feed(&s, 7).fresh);
        s.source.epoch = 2;
        assert!(!session.feed(&s, 8).fresh);
        advance(&mut s, 14);
        let fresh = session.feed(&s, 9);
        assert!(fresh.fresh);
        assert_eq!(fresh.value()["source"]["epoch"], "2");
        assert_eq!(fresh.value()["channels"]["rpm"]["status"], "ready");
        assert_eq!(fresh.value()["context"]["track"], "Xesearch-stage");
    }
    #[test]
    fn raw_byte_arrays_and_optional_hex_use_the_same_bounded_parser_and_fixture_source() {
        let s = sample();
        let frame = json!({"sourceEpoch":"1","physics":s.physics,"graphics":s.graphics,"staticPage":s.statics});
        let parsed = RawSample::parse_json(&frame).unwrap();
        assert_eq!(parsed.source.id, "synthetic");
        let mut source = FixtureSource::new();
        source.push(parsed.clone()).unwrap();
        let mut session = Session::new(100).unwrap();
        assert!(!session.poll(&mut source, 0).fresh);
        let mut next = parsed.clone();
        advance(&mut next, 11);
        source.push(next).unwrap();
        assert!(session.poll(&mut source, 1).fresh);
        assert!(!session.poll(&mut source, 102).fresh);
        for (field, value) in [
            ("sourceEpoch", json!("0")),
            ("sourceEpoch", json!("-1")),
            ("sourceEpoch", json!("18446744073709551616")),
            ("physics", json!([256])),
            ("physics", json!([1.2])),
            ("physics", json!("0G")),
            ("physics", json!("0")),
            ("extra", json!(true)),
        ] {
            let mut v = frame.clone();
            v[field] = value;
            assert!(RawSample::parse_json(&v).is_err(), "{field}");
        }
        let mut hex = frame.clone();
        hex["sourceEpoch"] = json!(1);
        hex["physics"] = json!(s
            .physics
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>());
        assert!(RawSample::parse_json(&hex).is_ok());
        for _ in 0..256 {
            source.push(parsed.clone()).unwrap();
        }
        assert!(source.push(parsed).is_err());
        source.close();
        assert!(source.poll().unwrap().is_none());
    }
    #[test]
    fn acquisition_errors_disarm_without_refreshing_frozen_source_data() {
        struct Broken(bool);
        impl Source for Broken {
            fn poll(&mut self) -> Result<Option<RawSample>, String> {
                Err("source gone".into())
            }
            fn close(&mut self) {
                self.0 = true;
            }
        }
        let mut session = Session::new(100).unwrap();
        let mut s = sample();
        session.feed(&s, 0);
        advance(&mut s, 11);
        session.feed(&s, 1);
        let mut broken = Broken(false);
        let failed = session.poll(&mut broken, 2);
        assert!(!failed.fresh);
        assert_eq!(failed.status, "error");
        assert!(broken.0);
        assert_eq!(failed.error.as_deref(), Some("source gone"));
        assert!(!session.feed(&s, 3).fresh);
        advance(&mut s, 12);
        assert!(session.feed(&s, 4).fresh);
        s.graphics[4..8].copy_from_slice(&99i32.to_le_bytes());
        let invalid = session.feed(&s, 5);
        assert!(!invalid.fresh);
        assert_eq!(invalid.status, "error");
        assert_eq!(session.feed(&s, 6).epoch, invalid.epoch);
    }
}
