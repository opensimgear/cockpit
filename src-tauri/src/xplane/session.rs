struct Session {
    channels: Vec<std::sync::Arc<datarefs::Channel>>,
    values: BTreeMap<usize, (f64, Instant)>,
    freshness: Duration,
    version: u64,
    context: Option<String>,
    clock: Option<f64>,
    advanced: Option<Instant>,
    baseline: bool,
    epoch: u64,
    exhausted: bool,
    counter: u16,
    status: String,
}
impl Session {
    fn new(v: &Value) -> Self {
        Self {
            channels: legacy_channels()
                .into_iter()
                .map(std::sync::Arc::new)
                .collect(),
            values: BTreeMap::new(),
            freshness: Duration::from_millis(v["freshnessMs"].as_u64().unwrap()),
            version: v["version"].as_u64().unwrap(),
            context: None,
            clock: None,
            advanced: None,
            baseline: false,
            epoch: 1,
            exhausted: false,
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
        if let Some(epoch) = self.epoch.checked_add(1) {
            self.epoch = epoch;
        } else {
            self.exhausted = true;
        }
        self.status = if self.exhausted {
            "X-Plane session epoch exhausted; reactivate the reader.".into()
        } else {
            status.into()
        };
    }
    fn ingest(&mut self, pairs: Vec<(usize, f64)>, now: Instant) {
        if self.exhausted {
            return;
        }
        for (index, value) in pairs {
            self.values.insert(index, (value, now));
        }
    }
    /// Unchanged datarefs keep their original capture evidence.
    fn select_channels(&mut self, channels: Vec<datarefs::Channel>) {
        self.select_shared_channels(channels.into_iter().map(std::sync::Arc::new).collect());
    }
    fn select_shared_channels(&mut self, channels: Vec<std::sync::Arc<datarefs::Channel>>) {
        let offset = GUARDS.len() + PATH_BYTES;
        let retained: BTreeMap<_, _> = self
            .channels
            .iter()
            .enumerate()
            .filter_map(|(i, channel)| {
                self.values
                    .get(&(offset + i))
                    .map(|value| (channel.dataref.as_str(), *value))
            })
            .collect();
        let mut values: BTreeMap<_, _> = self
            .values
            .iter()
            .filter(|(index, _)| **index < offset)
            .map(|(index, value)| (*index, *value))
            .collect();
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
            .filter(|(_, at)| {
                now.checked_duration_since(*at)
                    .is_some_and(|age| age < self.freshness)
            })
            .map(|(v, _)| *v)
            .filter(|v| v.is_finite())
    }
    /// Expired evidence cannot keep waking an otherwise idle producer.
    fn deadline(&self, now: Instant) -> Option<Instant> {
        self.values
            .values()
            .map(|(_, at)| *at)
            .chain(self.advanced)
            .filter_map(|at| at.checked_add(self.freshness))
            .filter(|at| *at > now)
            .min()
    }
    fn native_snapshot(
        &mut self,
        now: Instant,
        captured_at: Option<Instant>,
        sequence: u64,
        producer_id: &str,
        initializing: bool,
    ) -> (NativeSnapshot, bool) {
        let mut context = Vec::new();
        let mut required = (0..GUARDS.len()).collect::<Vec<_>>();
        for i in 0..PATH_BYTES {
            let index = GUARDS.len() + i;
            let Some(n) = self
                .get(index, now)
                .filter(|n| n.fract() == 0. && (0. ..=255.).contains(n))
            else {
                break;
            };
            required.push(index);
            context.push(n as u8);
            if n == 0. {
                break;
            }
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
        let engines = self
            .get(4, now)
            .filter(|n| n.fract() == 0. && (0. ..=8.).contains(n));
        let guards = !self.exhausted
            && version == Some(self.version)
            && self.get(1, now) == Some(0.)
            && self.get(2, now) == Some(0.)
            && engines.is_some()
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
                self.advanced = self.values.get(&3).map(|(_, at)| *at);
                self.counter = self.counter.wrapping_add(1);
            }
            self.context = aircraft;
            self.clock = clock;
        }
        if self
            .advanced
            .is_some_and(|at| now.saturating_duration_since(at) >= self.freshness)
        {
            self.reset("Simulator clock stopped; awaiting a new baseline.");
            rotate = true;
        }
        let fresh = guards && !rotate && self.baseline && !initializing && !self.exhausted;
        let valid_until = if fresh {
            required
                .iter()
                .filter_map(|i| self.values.get(i).map(|(_, at)| *at))
                .chain(self.advanced)
                .filter_map(|at| at.checked_add(self.freshness))
                .min()
        } else {
            None
        };
        let offset = GUARDS.len() + PATH_BYTES;
        let channels = self
            .channels
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let engine_available = match c.id.as_str() {
                    "engine_1_rpm" => engines.is_some_and(|n| n >= 1.),
                    "engine_2_rpm" => engines.is_some_and(|n| n >= 2.),
                    _ => c
                        .dataref
                        .strip_prefix("sim/cockpit2/engine/indicators/engine_speed_rpm[")
                        .and_then(|tail| tail.strip_suffix(']'))
                        .and_then(|index| index.parse::<usize>().ok())
                        .is_none_or(|index| {
                            engines.is_some_and(|engines| index < engines as usize)
                        }),
                };
                let value = self
                    .get(offset + i, now)
                    .filter(|n| {
                        fresh
                            && engine_available
                            && c.minimum.is_none_or(|min| *n >= min)
                            && c.maximum.is_none_or(|max| *n <= max)
                            && (c.kind != "boolean" || *n == 0. || *n == 1.)
                            && (!c.integer || n.fract() == 0.)
                    })
                    .map(|n| {
                        if c.kind == "boolean" {
                            NativeValue::Boolean(n == 1.)
                        } else {
                            NativeValue::Number(n)
                        }
                    });
                (
                    c.id.clone(),
                    NativeSample {
                        value,
                        captured_at: self.values.get(&(offset + i)).map(|(_, at)| *at),
                    },
                )
            })
            .collect();
        if self.exhausted {
            self.status = "X-Plane session epoch exhausted; reactivate the reader.".into();
        } else if initializing {
            self.status = "Subscribing to selected X-Plane datarefs in bounded batches.".into();
        } else if fresh {
            self.status =
                "Receiving X-Plane RREF data; live simulator qualification is not recorded.".into();
        } else if !rotate
            && (self.context.is_some()
                || !self.values.is_empty()
                || self.status.starts_with("Subscribing"))
        {
            self.status = "Waiting for a fresh advancing simulator baseline (check UDP, pause/replay and aircraft).".into();
        }
        (
            NativeSnapshot {
                epoch: self.epoch,
                aircraft: self.context.clone(),
                counter: self.counter,
                capture_sequence: sequence,
                captured_at,
                producer_id: producer_id.to_owned(),
                freshness: self.freshness,
                valid_until,
                advanced_at: self.advanced,
                fresh,
                status: self.status.clone(),
                channels: std::sync::Arc::new(channels),
            },
            rotate,
        )
    }
    #[cfg(test)]
    fn snapshot(&mut self, now: Instant, outputs: &[Value]) -> (ExportSnapshot, Value, bool) {
        let captured_at = self.values.values().map(|(_, at)| *at).max();
        let (native, rotate) = self.native_snapshot(now, captured_at, 0, "test", false);
        let (snapshot, status) = native.project(now, outputs, &[]);
        (snapshot, status, rotate)
    }
}

#[cfg(test)]
mod native_state_tests {
    use super::*;
    include!("state_tests.rs");
}
