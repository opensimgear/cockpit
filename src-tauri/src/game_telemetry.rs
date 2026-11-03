//! Assetto shared-memory dialects. Acquisition,
//! decoding and freshness are separate; fixture input uses the production parser.
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
#[path = "assetto_schema.rs"]
mod schema;
#[path = "game_telemetry/capture.rs"]
mod capture;
#[path = "game_telemetry/session.rs"]
mod session;
#[path = "game_telemetry/project.rs"]
mod project;
pub(crate) use capture::{CaptureOwner, CaptureSet, Page, PageCapture};
pub(crate) use session::Session;

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
// EVO's 2026-03-31 interface uses char metadata and fixed-size graphics
// substructures, rather than the original UTF-16 static car specification.
pub(crate) const EVO_CAR_MODEL_OFFSET: usize = 3086;
pub(crate) const EVO_GRAPHICS_SIZE: usize = EVO_CAR_MODEL_OFFSET + 33;
pub(crate) const EVO_TRACK_OFFSET: usize = 136;
pub(crate) const EVO_STATIC_SIZE: usize = EVO_TRACK_OFFSET + 33;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Game { Corsa, Competizione, Evo, Rally }
impl Game {
    pub fn from_id(id: &str) -> Result<Self, String> {
        match id {
            "assetto-corsa" => Ok(Self::Corsa),
            "assetto-corsa-competizione" => Ok(Self::Competizione),
            "assetto-corsa-evo" => Ok(Self::Evo),
            PROVIDER => Ok(Self::Rally),
            _ => Err("Unknown Assetto telemetry provider.".into()),
        }
    }
    pub fn id(self) -> &'static str {
        match self { Self::Corsa => "assetto-corsa", Self::Competizione => "assetto-corsa-competizione", Self::Evo => "assetto-corsa-evo", Self::Rally => PROVIDER }
    }
    pub fn source_id(self, pid: u32) -> String {
        format!("{}-pid-{pid}", if self == Self::Rally { "acr" } else { self.id() })
    }
    pub fn mapping_prefix(self) -> &'static str {
        if self == Self::Evo { "Local\\acevo_pmf" } else { "Local\\acpmf" }
    }
    pub fn page_lengths(self) -> (usize, usize, usize) {
        match self {
            Self::Evo => (PHYSICS_EXTENDED_SIZE, EVO_GRAPHICS_SIZE, EVO_STATIC_SIZE),
            Self::Rally => (PHYSICS_EXTENDED_SIZE, GRAPHICS_SIZE, STATIC_SIZE),
            _ => (PHYSICS_SIZE, GRAPHICS_SIZE, STATIC_SIZE),
        }
    }
    pub fn full_page_lengths(self) -> (usize, usize, usize) {
        schema::page_lengths(self)
    }
}

pub(crate) fn catalogue() -> Value {
    catalogue_for_game(PROVIDER)
}
pub(crate) fn catalogue_for_game(game_id: &str) -> Value {
    let game = match Game::from_id(game_id) { Ok(game) => game, Err(_) => return Value::Null };
    let channel = |id, label, kind, unit, minimum, maximum, derived| json!({"id":id,"label":label,"type":kind,"unit":unit,"minimum":minimum,"maximum":maximum,"derived":derived});
    let mut catalogue = json!({"provider":game.id(),"revision":2,"commands":[],"channels":[
        channel("rpm","Engine RPM","integer","rpm",0,100000,false),
        channel("speed_kmh","Speed","number","km/h",0,2000,false),
        channel("gear","Gear (-1 reverse, 0 neutral)","integer","",-1,31,true),
        channel("max_rpm","Maximum RPM","integer","rpm",1,100000,false),
        channel("rev_ratio","RPM / maximum RPM","number","ratio",0,100000,true)
    ]});
    for channel in catalogue["channels"].as_array_mut().unwrap() {
        channel["group"] = json!("summary");
        channel["category"] = json!("summary");
        channel["sourceGame"] = json!(game.id());
        channel["qualification"] = json!("legacy-compatible");
    }
    catalogue["channels"].as_array_mut().unwrap().extend(schema::catalogue(game));
    crate::game_channels::catalogue(game.id(), &mut catalogue);
    catalogue
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
    /// Native evidence is present only for pages captured by the live owner.
    pub capture: Option<CaptureSet>,
}
impl RawSample {
    pub fn parse_json(value: &Value) -> Result<Self, String> {
        Self::parse_json_for_game(value, Game::Rally)
    }
    pub fn parse_json_for_game(value: &Value, game: Game) -> Result<Self, String> {
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
            capture: None,
        };
        parse_for_game(&sample, game)?;
        Ok(sample)
    }
}

pub(crate) trait Source: Send {
    fn poll(&mut self) -> Result<Option<RawSample>, String>;
    fn close(&mut self);
}
pub(crate) struct FixtureSource {
    queue: VecDeque<RawSample>,
    game: Game,
}
impl Default for FixtureSource { fn default() -> Self { Self::new_for_game(Game::Rally) } }
impl FixtureSource {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn new_for_game(game: Game) -> Self { Self { queue: VecDeque::new(), game } }
    pub fn push(&mut self, sample: RawSample) -> Result<(), String> {
        parse_for_game(&sample, self.game)?;
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
    pub game: Game,
    pub epoch: u64,
    pub fresh: bool,
    pub age_ms: Option<u64>,
    pub status: String,
    pub source: Option<SourceIdentity>,
    pub context: Option<Context>,
    pub packet_id: Option<u32>,
    pub capture: Option<CaptureSet>,
    pub freshness_ms: u64,
    pub maximum_input: Option<&'static str>,
    pub channels: BTreeMap<String, Channel>,
    pub error: Option<String>,
}
impl Snapshot {
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
        json!({"provider":self.game.id(),"epoch":self.epoch,"fresh":self.fresh,"ageMs":self.age_ms,"status":self.status,
            "source":source,"context":context,"packetId":self.packet_id,"channels":channels,"error":self.error})
    }
}

pub(crate) struct Reading {
    game: Game,
    pub packet_id: u32,
    pub status: i32,
    pub maximum_input: Option<&'static str>,
    pub context: Context,
    pub channels: BTreeMap<String, Channel>,
}
impl Reading {
    /// Live ACR capture (2026-10-14): graphics status and version strings
    /// remain zero while driving. This dialect has no trustworthy state flag;
    /// require car/stage identity and let Session qualify packet progression.
    pub(crate) fn sparse_status(&self) -> bool {
        self.game == Game::Rally && self.status == 0
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
    parse_for_game(sample, Game::Rally)
}
pub(crate) fn parse_for_game(sample: &RawSample, game: Game) -> Result<Reading, String> {
    let mut reading = parse_prefix(sample, game)?;
    schema::decode(sample, game, &reading.context, &mut reading.channels);
    Ok(reading)
}
/// Acquisition validates only required context and legacy fields between copies;
/// expanded decoding happens once after the stable raw snapshot is selected.
pub(crate) fn validate_for_game(sample: &RawSample, game: Game) -> Result<(), String> {
    parse_prefix(sample, game).map(|_| ())
}
fn parse_prefix(sample: &RawSample, game: Game) -> Result<Reading, String> {
    let (_, graphics_size, static_size) = game.page_lengths();
    for (name, page, minimum) in [
        ("physics", &sample.physics, PHYSICS_SIZE),
        ("graphics", &sample.graphics, graphics_size),
        ("static", &sample.statics, static_size),
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
    fn narrow_text(page: &[u8], offset: usize, count: usize) -> Result<String, String> {
        let bytes = &page[offset..offset + count];
        let bytes = &bytes[..bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len())];
        let text = std::str::from_utf8(bytes).map_err(|_| "Invalid UTF-8 telemetry context.")?;
        if text.chars().any(char::is_control) { return Err("Telemetry context contains control characters.".into()); }
        Ok(text.trim_end().into())
    }
    let status = integer(&sample.graphics, 4);
    if !(0..=3).contains(&status) {
        return Err("Unknown telemetry game status.".into());
    }
    let context = if game == Game::Evo { Context {
        shared_memory_version: narrow_text(&sample.statics, 0, 15)?,
        game_version: narrow_text(&sample.statics, 15, 15)?,
        car_model: narrow_text(&sample.graphics, EVO_CAR_MODEL_OFFSET, 33)?,
        track: narrow_text(&sample.statics, EVO_TRACK_OFFSET, 33)?,
    }} else { Context {
        shared_memory_version: text(&sample.statics, 0, 15)?,
        game_version: text(&sample.statics, 30, 15)?,
        car_model: text(&sample.statics, 68, 33)?,
        track: text(&sample.statics, 134, 33)?,
    }};
    let rpm = integer(&sample.physics, 20);
    let gear = integer(&sample.physics, 16);
    // Prefer the current limit, which can change with the car's engine state.
    // Sparse ACR builds leave the static maximum at zero. Never infer a limit
    // from observed RPM or reinterpret invalid integer bytes as a float.
    // The EVO and observed Rally layouts explicitly define int32 currentMaxRpm.
    // AC/ACC use their static maxRpm; bytes at 588 are not interpreted for them.
    let current_maximum = (matches!(game, Game::Evo | Game::Rally) && sample.physics.len() >= PHYSICS_EXTENDED_SIZE)
        .then(|| integer(&sample.physics, PHYSICS_CURRENT_MAX_RPM_OFFSET))
        .filter(|maximum| (1..=100000).contains(maximum));
    let maximum = current_maximum.unwrap_or_else(|| if game == Game::Evo { 0 } else { integer(&sample.statics, 412) });
    let speed = f32::from_le_bytes(sample.physics[28..32].try_into().unwrap()) as f64;
    let valid_rpm = (0..=100000).contains(&rpm);
    let valid_maximum = (1..=100000).contains(&maximum);
    let maximum_input = valid_maximum.then_some(if current_maximum.is_some() {
        "physics.currentMaxRpm"
    } else { "static.maxRpm" });
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
        game,
        packet_id: integer(&sample.physics, 0) as u32,
        status,
        maximum_input,
        context,
        channels,
    })
}

#[cfg(test)]
#[path = "game_telemetry/parser_tests.rs"]
mod tests;
