//! X-Plane 11/12 read-only RREF UDP provider. No simulator files are changed.
use crate::dcs_profile::ExportSnapshot;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    net::{Ipv4Addr, SocketAddrV4, UdpSocket},
    time::{Duration, Instant},
};

pub(crate) const PROVIDER: &str = "x-plane";
#[path = "xplane_datarefs.rs"]
mod datarefs;
const PATH_BYTES: usize = 128;
// 1024 selected components plus the fixed identity guards fit in one bounded
// response even when the simulator coalesces them into a single UDP datagram.
const MAX_PACKET: usize = 16 * 1024;
const UNSUBSCRIBED: &str = "Unsubscribed X-Plane dataref index.";

struct Channel {
    id: &'static str,
    label: &'static str,
    dataref: &'static str,
    unit: &'static str,
    kind: &'static str,
    min: f64,
    max: f64,
}
const CHANNELS: &[Channel] = &[
    Channel {
        id: "airspeed_kts",
        label: "Indicated airspeed",
        dataref: "sim/flightmodel/position/indicated_airspeed",
        unit: "kt",
        kind: "number",
        min: 0.,
        max: 3000.,
    },
    Channel {
        id: "altitude_ft",
        label: "Indicated altitude",
        dataref: "sim/cockpit2/gauges/indicators/altitude_ft_pilot",
        unit: "ft",
        kind: "number",
        min: -3000.,
        max: 300000.,
    },
    Channel {
        id: "heading_deg",
        label: "Magnetic heading",
        dataref: "sim/flightmodel/position/mag_psi",
        unit: "deg",
        kind: "number",
        min: 0.,
        max: 360.,
    },
    Channel {
        id: "pitch_deg",
        label: "Pitch",
        dataref: "sim/flightmodel/position/theta",
        unit: "deg",
        kind: "number",
        min: -180.,
        max: 180.,
    },
    Channel {
        id: "roll_deg",
        label: "Roll",
        dataref: "sim/flightmodel/position/phi",
        unit: "deg",
        kind: "number",
        min: -180.,
        max: 180.,
    },
    Channel {
        id: "vertical_speed_fpm",
        label: "Vertical speed",
        dataref: "sim/cockpit2/gauges/indicators/vvi_fpm_pilot",
        unit: "ft/min",
        kind: "number",
        min: -100000.,
        max: 100000.,
    },
    Channel {
        id: "engine_1_rpm",
        label: "Engine 1 RPM",
        dataref: "sim/cockpit2/engine/indicators/engine_speed_rpm[0]",
        unit: "rpm",
        kind: "number",
        min: 0.,
        max: 100000.,
    },
    Channel {
        id: "engine_2_rpm",
        label: "Engine 2 RPM",
        dataref: "sim/cockpit2/engine/indicators/engine_speed_rpm[1]",
        unit: "rpm",
        kind: "number",
        min: 0.,
        max: 100000.,
    },
    Channel {
        id: "gear_ratio",
        label: "Gear deployment (first gear)",
        dataref: "sim/flightmodel2/gear/deploy_ratio[0]",
        unit: "ratio",
        kind: "number",
        min: 0.,
        max: 1.,
    },
    Channel {
        id: "flaps_ratio",
        label: "Flap handle",
        dataref: "sim/cockpit2/controls/flap_ratio",
        unit: "ratio",
        kind: "number",
        min: 0.,
        max: 1.,
    },
    Channel {
        id: "gear_unsafe",
        label: "Gear unsafe",
        dataref: "sim/cockpit2/annunciators/gear_unsafe",
        unit: "",
        kind: "boolean",
        min: 0.,
        max: 1.,
    },
    Channel {
        id: "stall_warning",
        label: "Stall warning",
        dataref: "sim/cockpit2/annunciators/stall_warning",
        unit: "",
        kind: "boolean",
        min: 0.,
        max: 1.,
    },
    Channel {
        id: "low_fuel",
        label: "Low fuel",
        dataref: "sim/cockpit2/annunciators/fuel_quantity",
        unit: "",
        kind: "boolean",
        min: 0.,
        max: 1.,
    },
    Channel {
        id: "landing_lights",
        label: "Landing lights switch",
        dataref: "sim/cockpit/electrical/landing_lights_on",
        unit: "",
        kind: "boolean",
        min: 0.,
        max: 1.,
    },
    Channel {
        id: "beacon_lights",
        label: "Beacon switch",
        dataref: "sim/cockpit/electrical/beacon_lights_on",
        unit: "",
        kind: "boolean",
        min: 0.,
        max: 1.,
    },
];
const GUARDS: &[&str] = &[
    "sim/version/xplane_internal_version",
    "sim/time/paused",
    "sim/time/is_in_replay",
    "sim/time/total_running_time_sec",
    "sim/aircraft/engine/acf_num_engines",
];

pub(crate) fn catalogue() -> Value {
    json!({"provider":PROVIDER,"revision":2,"commands":[],"channels":legacy_channels().iter().map(datarefs::Channel::value).collect::<Vec<_>>()})
}
fn legacy_channels() -> Vec<datarefs::Channel> {
    CHANNELS.iter().map(|c| datarefs::Channel {
        id:c.id.into(), label:c.label.into(), dataref:c.dataref.into(), unit:c.unit.into(), kind:c.kind.into(),
        minimum:Some(c.min), maximum:Some(c.max), integer:false,
        metadata:json!({"nativeField":c.dataref,"group":c.dataref.rsplit_once('/').map(|(group,_)|group).unwrap_or("sim"),"supported":true,"readOnly":true,"legacyAlias":true,"source":"https://developer.x-plane.com/sdk/XPLMDataAccess/"})
    }).collect()
}
/// Complete numeric metadata comes from the selected installation; no UDP
/// subscription is created while browsing its catalogue.
pub(crate) fn catalogue_for_installation(executable: &std::path::Path) -> Result<Value, String> {
    let native = datarefs::load(executable)?;
    let available = native.iter().filter(|c| c.metadata["supported"] == true)
        .map(|c| c.dataref.as_str()).collect::<BTreeSet<_>>();
    let mut channels = legacy_channels();
    for channel in &mut channels {
        channel.metadata["supported"] = json!(available.contains(channel.dataref.as_str()));
    }
    channels.extend(native);
    Ok(json!({"provider":PROVIDER,"revision":2,"commands":[],"channels":channels.iter().map(datarefs::Channel::value).collect::<Vec<_>>(),"catalogueSource":"Resources/plugins/DataRefs.txt","transport":"RREF","transportPrecision":"All RREF responses are IEEE-754 float32, including integer and double datarefs."}))
}
pub(crate) fn defaults(version: u64) -> Value {
    json!({"source":"rref","host":"127.0.0.1","port":49000,"listenPort":0,"updateHz":10,"freshnessMs":1500,"version":version})
}
pub(crate) fn validate_connection(v: &Value) -> Result<(), String> {
    if v["source"] != "rref" || !matches!(v["version"].as_u64(), Some(11 | 12)) {
        return Err("Choose X-Plane 11 or 12 and the RREF UDP source.".into());
    }
    // Local process monitoring is required; remote UDP cannot prove its producer.
    if v["host"]
        .as_str()
        .and_then(|s| s.parse::<Ipv4Addr>().ok())
        .is_none_or(|ip| !ip.is_loopback())
    {
        return Err(
            "Use a local IPv4 loopback address for the monitored X-Plane installation.".into(),
        );
    }
    for (key, min, max) in [
        ("port", 1, 65535),
        ("listenPort", 0, 65535),
        ("updateHz", 1, 20),
        ("freshnessMs", 100, 10000),
    ] {
        if v[key].as_u64().is_none_or(|n| n < min || n > max) {
            return Err(format!("Invalid X-Plane {key} ({min}–{max})."));
        }
    }
    if v["freshnessMs"].as_u64().unwrap() * v["updateHz"].as_u64().unwrap() < 2000 {
        return Err("Freshness must allow at least two requested updates.".into());
    }
    Ok(())
}

fn subscriptions() -> Vec<String> {
    subscriptions_for(&legacy_channels())
}
fn subscriptions_for(channels: &[datarefs::Channel]) -> Vec<String> {
    GUARDS
        .iter()
        .map(|s| s.to_string())
        .chain((0..PATH_BYTES).map(|i| format!("sim/aircraft/view/acf_relative_path[{i}]")))
        .chain(channels.iter().map(|c| c.dataref.clone()))
        .collect()
}
fn request(index: i32, hz: i32, path: &str) -> Vec<u8> {
    let mut bytes = vec![0; 413];
    bytes[..5].copy_from_slice(b"RREF\0");
    bytes[5..9].copy_from_slice(&hz.to_le_bytes());
    bytes[9..13].copy_from_slice(&index.to_le_bytes());
    bytes[13..13 + path.len()].copy_from_slice(path.as_bytes());
    bytes
}
fn decode(bytes: &[u8], base: i32, count: usize) -> Result<Vec<(usize, f64)>, String> {
    if bytes.len() < 13
        || bytes.len() > MAX_PACKET
        || &bytes[..5] != b"RREF,"
        || (bytes.len() - 5) % 8 != 0
    {
        return Err("Malformed X-Plane RREF packet.".into());
    }
    let mut seen = BTreeSet::new();
    bytes[5..]
        .chunks_exact(8)
        .map(|pair| {
            let wire = i32::from_le_bytes(pair[..4].try_into().unwrap());
            let index = wire
                .checked_sub(base)
                .and_then(|n| usize::try_from(n).ok())
                .filter(|n| *n < count)
                .ok_or(UNSUBSCRIBED)?;
            let number = f32::from_le_bytes(pair[4..].try_into().unwrap()) as f64;
            if !number.is_finite() || !seen.insert(index) {
                return Err("Non-finite or duplicate X-Plane dataref.".into());
            }
            Ok((index, number))
        })
        .collect()
}

struct Session {
    channels: Vec<datarefs::Channel>,
    values: BTreeMap<usize, (f64, Instant)>,
    freshness: Duration,
    version: u64,
    context: Option<String>,
    clock: Option<f64>,
    advanced: Option<Instant>,
    baseline: bool,
    epoch: u64,
    counter: u16,
    status: String,
}
impl Session {
    fn new(v: &Value) -> Self {
        Self {
            channels: legacy_channels(),
            values: BTreeMap::new(),
            freshness: Duration::from_millis(v["freshnessMs"].as_u64().unwrap()),
            version: v["version"].as_u64().unwrap(),
            context: None,
            clock: None,
            advanced: None,
            baseline: false,
            epoch: 1,
            counter: 0,
            status: "Waiting for X-Plane datarefs.".into(),
        }
    }
    fn reset(&mut self, status: &str) {
        self.values.clear();
        self.context = None;
        self.clock = None;
        self.advanced = None;
        self.baseline = false;
        self.epoch = self.epoch.wrapping_add(1);
        self.status = status.into();
    }
    fn ingest(&mut self, pairs: Vec<(usize, f64)>, now: Instant) {
        for (index, value) in pairs {
            self.values.insert(index, (value, now));
        }
    }
    fn get(&self, index: usize, now: Instant) -> Option<f64> {
        self.values
            .get(&index)
            .filter(|(_, at)| now.saturating_duration_since(*at) < self.freshness)
            .map(|(v, _)| *v)
    }
    fn snapshot(&mut self, now: Instant, outputs: &[Value]) -> (ExportSnapshot, Value, bool) {
        let mut context = Vec::new();
        for i in 0..PATH_BYTES {
            let Some(n) = self
                .get(GUARDS.len() + i, now)
                .filter(|n| n.fract() == 0. && (0. ..=255.).contains(n))
            else {
                break;
            };
            if n == 0. {
                context.push(0);
                break;
            }
            context.push(n as u8);
        }
        let aircraft = context
            .strip_suffix(&[0])
            .and_then(|s| std::str::from_utf8(s).ok())
            .filter(|s| !s.is_empty() && !s.chars().any(char::is_control))
            .map(str::to_owned);
        let version = self
            .get(0, now)
            .filter(|v| v.fract() == 0.)
            .map(|v| v as u64 / 10000);
        let clock = self.get(3, now).filter(|n| *n >= 0.);
        let guards = version == Some(self.version)
            && self.get(1, now) == Some(0.)
            && self.get(2, now) == Some(0.)
            && self
                .get(4, now)
                .is_some_and(|n| n.fract() == 0. && (0. ..=8.).contains(&n))
            && clock.is_some()
            && aircraft.is_some();
        let mut rotate = false;
        if !guards {
            if self.baseline || self.context.is_some() {
                self.reset("Waiting for live version, aircraft, pause/replay and clock datarefs.");
                rotate = true;
            }
        } else if self.context.is_some() && (self.context != aircraft || clock < self.clock) {
            self.reset("Aircraft or simulator session changed; awaiting a new baseline.");
            rotate = true;
        } else {
            if self.clock.is_some_and(|old| clock.unwrap() > old) {
                self.baseline = true;
                self.advanced = Some(now);
                self.counter = self.counter.wrapping_add(1);
            }
            self.context = aircraft.clone();
            // A frozen simulator clock must expire even if UDP continues arriving.
            if self.clock != clock {
                self.clock = clock;
            }
        }
        let clock_age = self
            .advanced
            .map(|at| now.saturating_duration_since(at).as_millis() as u64);
        if self
            .advanced
            .is_some_and(|at| now.saturating_duration_since(at) >= self.freshness)
        {
            self.reset("Simulator clock stopped; awaiting a new baseline.");
            rotate = true;
        }
        let fresh = guards && !rotate && self.baseline;
        let offset = GUARDS.len() + PATH_BYTES;
        let mut channels = serde_json::Map::new();
        let mut ready = BTreeMap::new();
        for (i, c) in self.channels.iter().enumerate() {
            let engines = self
                .get(4, now)
                .filter(|n| n.fract() == 0. && (0. ..=8.).contains(n));
            let engine_available = match c.id.as_str() {
                "engine_1_rpm" => engines.is_some_and(|n| n >= 1.),
                "engine_2_rpm" => engines.is_some_and(|n| n >= 2.),
                _ => c.dataref.strip_prefix("sim/cockpit2/engine/indicators/engine_speed_rpm[")
                    .and_then(|tail| tail.strip_suffix(']')).and_then(|index| index.parse::<usize>().ok())
                    .is_none_or(|index| engines.is_some_and(|engines| index < engines as usize)),
            };
            let n = self.get(offset + i, now).filter(|n| {
                c.minimum.is_none_or(|min| *n >= min) && c.maximum.is_none_or(|max| *n <= max)
                    && (c.kind != "boolean" || *n == 0. || *n == 1.)
                    && (!c.integer || n.fract() == 0.)
            });
            let value = n.filter(|_| fresh && engine_available).map(|n| {
                if c.kind == "boolean" {
                    json!(n == 1.)
                } else {
                    json!(n)
                }
            });
            if let Some(v) = &value {
                ready.insert(c.id.to_owned(), v.clone());
            }
            channels.insert(
                c.id.clone(),
                json!({"status":if value.is_some(){"ready"}else{"unavailable"},"value":value}),
            );
        }
        if fresh {
            self.status =
                "Receiving X-Plane RREF data; live simulator qualification is not recorded.".into();
        } else if !rotate {
            self.status = "Waiting for a fresh advancing simulator baseline (check UDP, pause/replay and aircraft).".into();
        }
        let values = outputs
            .iter()
            .filter_map(|o| {
                ready
                    .get(o["channel"].as_str()?)
                    .map(|v| (o["id"].as_str().unwrap().into(), v.clone()))
            })
            .collect();
        (
            ExportSnapshot {
                epoch: self.epoch,
                aircraft: self.context.clone(),
                mission_active: fresh,
                fresh,
                age_ms: clock_age,
                counter: Some(self.counter),
                values,
                malformed_frames: 0,
            },
            json!({"fresh":fresh,"status":self.status,"ageMs":clock_age,"channels":channels,"verified":false}),
            rotate,
        )
    }
}

pub(crate) struct Source {
    socket: UdpSocket,
    connection: Value,
    paths: Vec<String>,
    supported: Vec<bool>,
    base: i32,
    subscribed: Instant,
    session: Session,
    monitor: Box<dyn FnMut() -> Result<(), String> + Send>,
    outputs: Vec<Value>,
    catalogue: BTreeMap<String, datarefs::Channel>,
    inspection: Vec<String>,
    pending_subscriptions: VecDeque<Vec<u8>>,
    last_subscription_batch: Instant,
    initializing_subscriptions: bool,
}
impl Source {
    pub(crate) fn new(connection: &Value, outputs: Vec<Value>) -> Result<Self, String> {
        validate_connection(connection)?;
        if !cfg!(windows) {
            return Err("X-Plane native process monitoring is currently supported on Windows; Linux/macOS profiles can be edited but cannot start.".into());
        }
        let identity =
            crate::game_discovery::ProcessIdentity::parse(&connection["processIdentity"])?;
        let game = format!("x-plane-{}", connection["version"].as_u64().unwrap());
        use crate::game_discovery::Platform;
        crate::game_discovery::NativePlatform.validate(&game, &identity.path)?;
        crate::game_discovery::exclusive_xplane(&identity)?;
        let available =
            crate::xplane_installation::available(std::path::Path::new(&identity.path))?;
        for path in GUARDS
            .iter()
            .copied()
            .chain(Some("sim/aircraft/view/acf_relative_path"))
        {
            if !available.contains(path) {
                return Err(format!("Required X-Plane session dataref is unavailable: {path}. Repair the simulator installation."));
            }
        }
        let mut channels = legacy_channels();
        for channel in &mut channels {
            channel.metadata["supported"] = json!(available.contains(channel.dataref.split('[').next().unwrap()));
        }
        channels.extend(datarefs::load(std::path::Path::new(&identity.path))?);
        let catalogue = channels.into_iter().map(|c| (c.id.clone(), c)).collect();
        let selected = Self::selected(&catalogue, &outputs, &[])?;
        let supported = vec![true; GUARDS.len() + PATH_BYTES + selected.len()];
        let mut last_check = Instant::now();
        let mut monitor_error: Option<String> = None;
        Self::connect_selected(
            connection,
            outputs,
            supported,
            catalogue,
            selected,
            Box::new(move || {
                if let Some(error) = &monitor_error {
                    return Err(error.clone());
                }
                if last_check.elapsed() < Duration::from_millis(250) {
                    return Ok(());
                }
                last_check = Instant::now();
                use crate::game_discovery::Platform;
                let result = (|| {
                    if !crate::game_discovery::NativePlatform.alive(&identity)? {
                        return Err("Selected X-Plane process exited. Stop and reactivate its game profile.".to_owned());
                    }
                    crate::game_discovery::exclusive_xplane(&identity)
                })();
                if let Err(error) = &result {
                    monitor_error = Some(error.clone());
                }
                result
            }),
        )
    }
    fn connect(
        connection: &Value,
        outputs: Vec<Value>,
        supported: Vec<bool>,
        monitor: Box<dyn FnMut() -> Result<(), String> + Send>,
    ) -> Result<Self, String> {
        let channels = legacy_channels();
        let catalogue = channels.iter().map(|c| (c.id.clone(), c.clone())).collect();
        Self::connect_selected(connection, outputs, supported, catalogue, channels, monitor)
    }
    fn connect_selected(
        connection: &Value,
        outputs: Vec<Value>,
        supported: Vec<bool>,
        catalogue: BTreeMap<String, datarefs::Channel>,
        channels: Vec<datarefs::Channel>,
        monitor: Box<dyn FnMut() -> Result<(), String> + Send>,
    ) -> Result<Self, String> {
        validate_connection(connection)?;
        let socket = Self::socket(connection)?;
        let mut source = Self {
            socket,
            connection: connection.clone(),
            paths: subscriptions_for(&channels),
            supported,
            base: Self::base(),
            subscribed: Instant::now(),
            session: Session::new(connection),
            monitor,
            outputs,
            catalogue,
            inspection: vec![],
            pending_subscriptions: VecDeque::new(),
            last_subscription_batch: Instant::now() - Duration::from_millis(100),
            initializing_subscriptions: true,
        };
        source.session.channels = channels;
        source.subscribe(connection["updateHz"].as_i64().unwrap() as i32)?;
        Ok(source)
    }
    fn selected(catalogue: &BTreeMap<String, datarefs::Channel>, outputs: &[Value], inspection: &[String]) -> Result<Vec<datarefs::Channel>, String> {
        let mut selected = BTreeSet::new();
        for output in outputs {
            selected.insert(output["channel"].as_str().ok_or("Choose a numeric X-Plane source channel.")?.to_owned());
        }
        selected.extend(inspection.iter().cloned());
        if selected.len() > 1024 { return Err("Select at most 1024 X-Plane dataref components per reader.".into()); }
        selected.into_iter().map(|id| catalogue.get(&id)
            .filter(|c| c.metadata["supported"] == true).cloned()
            .ok_or_else(|| format!("Selected X-Plane dataref is unavailable in this installation: {id}."))).collect()
    }
    /// Retain profile subscriptions and add only the currently visible passive
    /// inspection page. Rotating indexes prevents late old-page replies from
    /// becoming values in the new page or aircraft epoch.
    pub(crate) fn set_inspection_channels(&mut self, ids: &[String]) -> Result<(), String> {
        if ids.len() > 512 { return Err("Inspect at most 512 X-Plane components per page.".into()); }
        let mut inspection = ids.to_vec();
        inspection.sort(); inspection.dedup();
        if inspection == self.inspection { return Ok(()); }
        let channels = Self::selected(&self.catalogue, &self.outputs, &inspection)?;
        self.subscribe(0)?;
        self.paths = subscriptions_for(&channels);
        self.supported = vec![true; self.paths.len()];
        self.base = Self::base();
        self.session.reset("X-Plane inspection selection changed; awaiting fresh datarefs.");
        self.session.channels = channels;
        self.inspection = inspection;
        self.initializing_subscriptions = true;
        self.subscribe(self.connection["updateHz"].as_i64().unwrap() as i32)
    }
    fn base() -> i32 {
        (uuid::Uuid::new_v4().as_u128() % 1_000_000_000) as i32 + 1
    }
    fn socket(v: &Value) -> Result<UdpSocket, String> {
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, v["listenPort"].as_u64().unwrap() as u16)).map_err(|e| format!("Cannot bind X-Plane UDP reply port; close conflicting listeners or choose another port: {e}"))?;
        socket2::SockRef::from(&socket)
            .set_recv_buffer_size(65536)
            .map_err(|e| e.to_string())?;
        socket
            .connect(SocketAddrV4::new(
                v["host"].as_str().unwrap().parse().unwrap(),
                v["port"].as_u64().unwrap() as u16,
            ))
            .map_err(|e| e.to_string())?;
        socket.set_nonblocking(true).map_err(|e| e.to_string())?;
        Ok(socket)
    }
    fn subscribe(&mut self, hz: i32) -> Result<(), String> {
        if hz == 0 {
            // A pending old positive request must never resurrect a page that
            // was replaced before its subscription batch reached the simulator.
            self.pending_subscriptions.retain(|request| request[5..9] == [0;4]);
        }
        if self.pending_subscriptions.len() + self.paths.len() > 8192 {
            return Err("X-Plane subscription changes exceeded the bounded request queue.".into());
        }
        for (i, path) in self
            .paths
            .iter()
            .enumerate()
            .filter(|(i, _)| self.supported[*i])
        {
            self.pending_subscriptions.push_back(request(self.base + i as i32, hz, path));
        }
        self.subscribed = Instant::now();
        self.flush_subscriptions(Instant::now())
    }
    fn flush_subscriptions(&mut self, now: Instant) -> Result<(), String> {
        if now.saturating_duration_since(self.last_subscription_batch) < Duration::from_millis(100) { return Ok(()); }
        // The simulator's own network documentation recommends at most 100
        // requests per 100 ms. Work is advanced by the regular poll loop.
        for _ in 0..100 {
            let Some(bytes) = self.pending_subscriptions.front() else { break; };
            self.socket.send(bytes).map_err(|e| format!("Cannot subscribe to X-Plane UDP datarefs: {e}"))?;
            self.pending_subscriptions.pop_front();
        }
        self.last_subscription_batch = now;
        if self.pending_subscriptions.is_empty() { self.initializing_subscriptions = false; }
        Ok(())
    }
    pub(crate) fn poll(&mut self, now: Instant) -> (ExportSnapshot, Value) {
        let result = (|| -> Result<(), String> {
            (self.monitor)()?;
            self.flush_subscriptions(now)?;
            if now.saturating_duration_since(self.subscribed) >= Duration::from_secs(2) {
                self.subscribe(self.connection["updateHz"].as_i64().unwrap() as i32)?;
                self.subscribed = now;
            }
            let mut bytes = [0; MAX_PACKET + 1];
            // Bound work per poll; no unbounded receive queue is copied into memory.
            for _ in 0..64 {
                match self.socket.recv(&mut bytes) {
                    Ok(n) => {
                        let values = match decode(&bytes[..n], self.base, self.paths.len()) {
                            Ok(values) => values,
                            // A dropped unsubscribe can leave an old subscription alive.
                            // Ignore its entire datagram without refreshing this session.
                            Err(error) if error == UNSUBSCRIBED => continue,
                            Err(error) => return Err(error),
                        };
                        if values.iter().any(|(i, _)| !self.supported[*i]) {
                            return Err("Received an unavailable X-Plane dataref index.".into());
                        }
                        self.session.ingest(values, now);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => {
                        return Err(format!(
                            "X-Plane UDP unavailable (check simulator port/firewall): {e}"
                        ))
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = &result {
            self.session.reset(error);
        }
        let (mut snapshot, mut status, rotate) = self.session.snapshot(now, &self.outputs);
        if self.initializing_subscriptions {
            snapshot.fresh = false;
            snapshot.mission_active = false;
            snapshot.values.clear();
            status["fresh"] = json!(false);
            status["status"] = json!("Subscribing to selected X-Plane datarefs in bounded batches.");
            for channel in status["channels"].as_object_mut().into_iter().flat_map(|channels| channels.values_mut()) {
                channel["status"] = json!("unavailable"); channel["value"] = Value::Null;
            }
        }
        if snapshot.fresh {
            for id in &self.inspection {
                if status["channels"][id]["status"] == "ready" {
                    snapshot.values.insert(id.clone(), status["channels"][id]["value"].clone());
                }
            }
        }
        let failed = result.is_err();
        if let Err(error) = result {
            status["status"] = json!(error);
        }
        if rotate || failed {
            let _ = self.subscribe(0);
            self.base = Self::base();
            self.initializing_subscriptions = true;
            let _ = self.subscribe(self.connection["updateHz"].as_i64().unwrap() as i32);
            self.subscribed = now;
        }
        (snapshot, status)
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        let _ = self.subscribe(0);
        // Do not block shutdown waiting for future poll ticks. Flush remaining
        // cancellations in bounded batches using the documented pacing.
        while !self.pending_subscriptions.is_empty() {
            std::thread::sleep(Duration::from_millis(100));
            if self.flush_subscriptions(Instant::now()).is_err() { break; }
        }
    }
}

#[cfg(test)]
mod tests;
