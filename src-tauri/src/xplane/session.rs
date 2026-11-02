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
    /// Selection changes preserve evidence for still-selected datarefs. Newly
    /// selected components start unknown, even when their retired index replies.
    fn select_channels(&mut self, channels: Vec<datarefs::Channel>) {
        let offset = GUARDS.len() + PATH_BYTES;
        let retained: BTreeMap<_, _> = self.channels.iter().enumerate()
            .filter_map(|(i, channel)| self.values.get(&(offset + i))
                .map(|value| (channel.dataref.as_str(), *value))).collect();
        let mut values: BTreeMap<_, _> = self.values.iter()
            .filter(|(index, _)| **index < offset).map(|(index, value)| (*index, *value)).collect();
        for (i, channel) in channels.iter().enumerate() {
            if let Some(value) = retained.get(channel.dataref.as_str()) {
                values.insert(offset + i, *value);
            }
        }
        self.values = values;
        self.channels = channels;
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
