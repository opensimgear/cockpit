/// Native evidence is retained without constructing or cloning JSON payloads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum NativeValue {
    Number(f64),
    Boolean(bool),
}
impl NativeValue {
    fn json(self) -> Value {
        match self {
            Self::Number(n) => json!(n),
            Self::Boolean(b) => json!(b),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct NativeSample {
    pub value: Option<NativeValue>,
    pub captured_at: Option<Instant>,
    pub capture_sequence: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct NativeSnapshot {
    pub epoch: u64,
    pub aircraft: Option<String>,
    pub counter: u16,
    pub capture_sequence: u64,
    pub captured_at: Option<Instant>,
    pub producer_id: String,
    pub freshness: Duration,
    pub valid_until: Option<Instant>,
    pub advanced_at: Option<Instant>,
    pub fresh: bool,
    pub status: String,
    pub channels: std::sync::Arc<BTreeMap<String, NativeSample>>,
}
impl NativeSnapshot {
    pub(crate) fn fresh_at(&self, now: Instant) -> bool {
        self.fresh
            && self.valid_until.is_some_and(|until| now < until)
            && self.captured_at.is_some_and(|at| at <= now)
            && self.advanced_at.is_some_and(|at| at <= now)
    }
    pub(crate) fn age_ms(&self, now: Instant) -> Option<u64> {
        self.advanced_at
            .and_then(|at| now.checked_duration_since(at))
            .map(|age| u64::try_from(age.as_millis()).unwrap_or(u64::MAX))
    }
    fn sample_at(&self, sample: &NativeSample, now: Instant) -> Option<NativeValue> {
        sample
            .captured_at
            .and_then(|at| now.checked_duration_since(at))
            .filter(|age| *age < self.freshness)
            .and(sample.value)
    }
    /// Projection never changes baseline, capture evidence, counters or epochs.
    pub(crate) fn project(
        &self,
        now: Instant,
        outputs: &[Value],
        inspection: &[String],
    ) -> (ExportSnapshot, Value) {
        self.project_with(now, outputs, inspection, None)
    }
    /// A reader's work is bounded by its own selection, regardless of the
    /// component count retained for other readers of the same native source.
    pub(crate) fn project_selected(
        &self,
        now: Instant,
        outputs: &[Value],
        inspection: &[String],
    ) -> (ExportSnapshot, Value) {
        let selected = outputs
            .iter()
            .filter_map(|output| output["channel"].as_str())
            .chain(inspection.iter().map(String::as_str))
            .collect();
        self.project_with(now, outputs, inspection, Some(selected))
    }
    fn project_with(
        &self,
        now: Instant,
        outputs: &[Value],
        inspection: &[String],
        selection: Option<BTreeSet<&str>>,
    ) -> (ExportSnapshot, Value) {
        let fresh = self.fresh_at(now);
        let mut channels = serde_json::Map::new();
        let mut ready = BTreeMap::new();
        let ids: Box<dyn Iterator<Item = &str> + '_> = match selection.as_ref() {
            Some(ids) => Box::new(ids.iter().copied()),
            None => Box::new(self.channels.keys().map(String::as_str)),
        };
        for id in ids {
            let value = fresh
                .then(|| {
                    self.channels
                        .get(id)
                        .and_then(|sample| self.sample_at(sample, now))
                })
                .flatten();
            if let Some(value) = value {
                ready.insert(id, value);
            }
            channels.insert(
                id.to_owned(),
                json!({"status":if value.is_some(){"ready"}else{"unavailable"},
                "value":value.map(NativeValue::json)}),
            );
        }
        let mut values: BTreeMap<String, Value> = outputs
            .iter()
            .filter_map(|output| {
                Some((
                    output["id"].as_str()?.to_owned(),
                    ready.get(output["channel"].as_str()?)?.json(),
                ))
            })
            .collect();
        for id in inspection {
            if let Some(value) = ready.get(id.as_str()) {
                values.insert(id.clone(), value.json());
            }
        }
        let age_ms = self.age_ms(now);
        let status = if self.fresh && !fresh {
            "X-Plane native evidence expired; awaiting fresh simulator data."
        } else {
            &self.status
        };
        (
            ExportSnapshot {
                epoch: self.epoch,
                aircraft: self.aircraft.clone(),
                mission_active: fresh,
                fresh,
                age_ms,
                counter: Some(self.counter),
                values,
                malformed_frames: 0,
            },
            json!({"fresh":fresh,"status":status,"ageMs":age_ms,"channels":channels,"verified":false}),
        )
    }
}
impl osg_streams::RetainedBytes for NativeSnapshot {
    fn retained_bytes(&self) -> usize {
        // Count the entire shared map for each retained record (conservative even
        // when Arc storage is shared). The per-entry allowance covers B-tree
        // node slack, links, allocator metadata, and Arc allocation overhead.
        std::mem::size_of::<Self>()
            + 64
            + self.producer_id.capacity()
            + self.status.capacity()
            + self.aircraft.as_ref().map_or(0, String::capacity)
            + self
                .channels
                .iter()
                .map(|(id, _)| id.capacity() + std::mem::size_of::<(String, NativeSample)>() + 128)
                .sum::<usize>()
    }
}
