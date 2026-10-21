//! Native typed feedback. Host routing is authoritative; samples never call SourceSink.
use crate::publication::{ensure, id};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
pub const FEEDBACK_STALE_MS: u64 = 1000;
pub const FEEDBACK_BYTES: usize = 128 * 1024;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FeedbackValue {
    Boolean(bool),
    Number(f64),
    Text(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeedbackStatus {
    Fresh,
    Unknown,
    Stale,
    Disconnected,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeedbackProvenance {
    Requested,
    Local,
    HostAccepted,
    HostSubmitted,
    GameConfirmed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeedbackSource {
    Host,
    Simulator,
    Script,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeedbackMode {
    Snapshot,
    Delta,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeedbackRange {
    pub min: f64,
    pub max: f64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeedbackBinding {
    pub channel_id: String,
    pub output_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeedbackToken {
    pub instance_id: String,
    pub publication_hash: String,
    pub session_id: String,
    pub generation: u64,
    pub feedback_generation: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeedbackSample {
    pub channel_id: String,
    pub output_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<FeedbackValue>,
    pub status: FeedbackStatus,
    pub provenance: FeedbackProvenance,
    pub source_kind: FeedbackSource,
    pub source_age_ms: u64,
    pub unit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<FeedbackRange>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum FeedbackKind {
    Boolean,
    Number,
    Text,
}
#[derive(Clone, Debug, PartialEq)]
pub struct FeedbackDescriptor {
    pub channel_id: String,
    pub output_ids: Vec<String>,
    pub kind: FeedbackKind,
    pub unit: Option<String>,
    pub range: Option<FeedbackRange>,
}
pub fn feedback_descriptors(document: &Value) -> Result<Vec<FeedbackDescriptor>, String> {
    let feedback = document["ui"]["feedbackWidgets"]
        .as_array()
        .filter(|v| v.len() <= 64)
        .ok_or("Invalid published feedback descriptor budget.")?;
    let widgets = document["presentation"]["widgets"]
        .as_array()
        .ok_or("Missing published feedback presentation.")?;
    let mut result = BTreeMap::new();
    for f in feedback {
        let channel = f["channelId"]
            .as_str()
            .filter(|v| id(v))
            .ok_or("Invalid published feedback channel.")?;
        let widget = widgets
            .iter()
            .find(|w| {
                w["feedbackIds"]
                    .as_array()
                    .is_some_and(|refs| refs.contains(&f["id"]))
            })
            .ok_or("Unbound published feedback channel.")?;
        let component_port = if widget["kind"] == "component" {
            let definition = document["presentation"]["definitions"].as_array().and_then(|definitions| definitions.iter().find(|d| d["id"] == widget["config"]["definitionId"] && d["version"] == widget["config"]["definitionVersion"])).ok_or("Unknown feedback widget definition.")?;
            let index = widget["feedbackIds"].as_array().and_then(|ids| ids.iter().position(|id| *id == f["id"])).ok_or("Unknown widget feedback port.")?;
            let port = definition["outputs"].get(index).ok_or("Unknown widget feedback port.")?;
            ensure(port["kind"] == f["kind"], "Widget feedback port kind mismatch.")?;
            Some(port)
        } else { None };
        let (kind, unit, range) = if let Some(port) = component_port {
            let kind = match f["kind"].as_str() { Some("text") => FeedbackKind::Text, Some("indicator") => FeedbackKind::Boolean, Some("gauge") => FeedbackKind::Number, _ => return Err("Unsupported widget feedback kind.".into()) };
            let unit = port.get("unit").map(|v| { let unit = v.as_str().ok_or("Invalid widget feedback unit.")?; ensure(text(unit, 120), "Invalid widget feedback unit.")?; Ok::<String, String>(if unit.trim().is_empty() { "ratio".into() } else { unit.into() }) }).transpose()?;
            let range = port.get("range").map(|v| { let r: FeedbackRange = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?; ensure(kind == FeedbackKind::Number && r.min.is_finite() && r.max.is_finite() && r.min < r.max, "Invalid widget feedback range.")?; Ok::<FeedbackRange, String>(r) }).transpose()?;
            (kind, unit, range)
        } else { match f["kind"].as_str() {
            Some("text") => (FeedbackKind::Text, None, None),
            Some("indicator") => (FeedbackKind::Boolean, None, None),
            Some("gauge") => {
                let min = widget["config"]["min"]
                    .as_f64()
                    .filter(|v| v.is_finite())
                    .ok_or("Invalid feedback range.")?;
                let max = widget["config"]["max"]
                    .as_f64()
                    .filter(|v| v.is_finite() && *v > min)
                    .ok_or("Invalid feedback range.")?;
                let unit = widget["config"]["unit"]
                    .as_str()
                    .ok_or("Missing feedback unit.")?;
                ensure(text(unit, 120), "Invalid published feedback unit.")?;
                (
                    FeedbackKind::Number,
                    Some(if unit.trim().is_empty() {
                        "ratio".into()
                    } else {
                        unit.into()
                    }),
                    Some(FeedbackRange { min, max }),
                )
            }
            _ => return Err("Unsupported published feedback kind.".into()),
        } };
        let output = f["id"]
            .as_str()
            .filter(|v| id(v))
            .ok_or("Invalid published feedback output ID.")?;
        let descriptor = FeedbackDescriptor {
            channel_id: channel.into(),
            output_ids: vec![output.into()],
            kind,
            unit,
            range,
        };
        if let Some(previous) = result.get_mut(channel) {
            let previous: &mut FeedbackDescriptor = previous;
            ensure(
                previous.kind == descriptor.kind
                    && previous.unit == descriptor.unit
                    && previous.range == descriptor.range,
                "A feedback channel has incompatible widget types, units or ranges.",
            )?;
            ensure(
                !previous.output_ids.contains(&output.to_string()),
                "Repeated published feedback output ID.",
            )?;
            previous.output_ids.push(output.into());
        } else {
            result.insert(channel.to_string(), descriptor);
        }
    }
    Ok(result.into_values().collect())
}
fn text(value: &str, max: usize) -> bool {
    value.encode_utf16().count() <= max && !value.chars().any(|c| c <= '\u{1f}')
}
pub fn validate_feedback_samples(
    schema: &[FeedbackDescriptor],
    bindings: &[FeedbackBinding],
    samples: &[FeedbackSample],
) -> Result<(), String> {
    ensure(samples.len() <= 64, "Feedback value budget exceeded.")?;
    let mut seen = BTreeSet::new();
    for sample in samples {
        ensure(
            id(&sample.channel_id) && id(&sample.output_id) && seen.insert(&sample.channel_id),
            "Invalid or repeated feedback channel/output ID.",
        )?;
        let descriptor = schema
            .iter()
            .find(|d| d.channel_id == sample.channel_id)
            .ok_or("Unknown feedback channel.")?;
        ensure(
            descriptor.output_ids.contains(&sample.output_id)
                && bindings
                    .iter()
                    .any(|b| b.channel_id == sample.channel_id && b.output_id == sample.output_id),
            "Wrong or unbound feedback output ID.",
        )?;
        ensure(
            sample.source_age_ms <= 86_400_000
                && !sample.unit.trim().is_empty()
                && text(&sample.unit, 120),
            "Invalid feedback source age/unit.",
        )?;
        ensure(
            descriptor.unit.as_ref().is_none_or(|u| u == &sample.unit)
                && descriptor.range == sample.range,
            "Feedback unit/range differs from the immutable publication.",
        )?;
        ensure(
            sample.provenance != FeedbackProvenance::GameConfirmed
                || sample.source_kind == FeedbackSource::Simulator,
            "Only supported simulator telemetry can be game-confirmed.",
        )?;
        ensure(
            match sample.status {
                FeedbackStatus::Fresh => sample.value.is_some(),
                FeedbackStatus::Unknown | FeedbackStatus::Disconnected => sample.value.is_none(),
                FeedbackStatus::Stale => true,
            },
            "Feedback availability/value mismatch.",
        )?;
        if let Some(value) = &sample.value {
            ensure(
                match (&descriptor.kind, value) {
                    (FeedbackKind::Boolean, FeedbackValue::Boolean(_)) => true,
                    (FeedbackKind::Text, FeedbackValue::Text(v)) => text(v, 256),
                    (FeedbackKind::Number, FeedbackValue::Number(v)) => {
                        v.is_finite()
                            && descriptor
                                .range
                                .as_ref()
                                .is_some_and(|r| r.min <= *v && *v <= r.max)
                    }
                    _ => false,
                },
                "Feedback value does not match the published type/range.",
            )?;
        }
    }
    Ok(())
}
impl FeedbackSample {
    pub fn aged(&self, elapsed: u64) -> Self {
        let mut sample = self.clone();
        sample.source_age_ms = sample.source_age_ms.saturating_add(elapsed).min(86_400_000);
        if sample.status == FeedbackStatus::Fresh && sample.source_age_ms >= FEEDBACK_STALE_MS {
            sample.status = FeedbackStatus::Stale;
        }
        sample
    }
}
#[derive(Clone)]
struct Stored {
    sample: FeedbackSample,
    updated: u64,
    revision: u64,
}
#[derive(Clone, Default)]
pub(crate) struct FeedbackBook {
    pub generation: u64,
    pub sequence: u64,
    pub updated: Option<u64>,
    pub bindings: Vec<FeedbackBinding>,
    schema: Vec<FeedbackDescriptor>,
    values: BTreeMap<String, Stored>,
}
#[derive(Clone, Default)]
pub(crate) struct FeedbackCursor {
    pub initialized: bool,
    pub generation: u64,
    pub sequence: u64,
}
impl FeedbackBook {
    pub fn configure(
        &mut self,
        schema: Vec<FeedbackDescriptor>,
        bindings: &[FeedbackBinding],
        now: u64,
    ) -> Result<(), String> {
        ensure(bindings.len() <= 64, "Feedback binding budget exceeded.")?;
        let mut seen = BTreeSet::new();
        for b in bindings {
            ensure(
                id(&b.output_id)
                    && seen.insert(&b.channel_id)
                    && schema.iter().any(|d| {
                        d.channel_id == b.channel_id && d.output_ids.contains(&b.output_id)
                    }),
                "Invalid, repeated or unknown feedback binding.",
            )?;
        }
        if self.generation > 0 && self.bindings == bindings && self.schema == schema {
            return Ok(());
        }
        let generation = self
            .generation
            .checked_add(1)
            .ok_or("Feedback generation exhausted.")?;
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Feedback sequence exhausted.")?;
        self.generation = generation;
        self.sequence = sequence;
        self.updated = None;
        self.schema = schema;
        self.bindings = bindings.to_vec();
        self.values.clear();
        self.fill_unknown(now);
        Ok(())
    }
    fn unknown(&self, binding: &FeedbackBinding, status: FeedbackStatus) -> FeedbackSample {
        let d = self
            .schema
            .iter()
            .find(|d| d.channel_id == binding.channel_id)
            .unwrap();
        FeedbackSample {
            channel_id: binding.channel_id.clone(),
            output_id: binding.output_id.clone(),
            value: None,
            status,
            provenance: FeedbackProvenance::Local,
            source_kind: FeedbackSource::Host,
            source_age_ms: 0,
            unit: d.unit.clone().unwrap_or_else(|| {
                if d.kind == FeedbackKind::Text {
                    "text".into()
                } else {
                    "state".into()
                }
            }),
            range: d.range.clone(),
        }
    }
    fn fill_unknown(&mut self, now: u64) {
        for b in &self.bindings {
            self.values.insert(
                b.channel_id.clone(),
                Stored {
                    sample: self.unknown(b, FeedbackStatus::Unknown),
                    updated: now,
                    revision: self.sequence,
                },
            );
        }
    }
    pub fn publish(
        &mut self,
        samples: &[FeedbackSample],
        mode: FeedbackMode,
        now: u64,
    ) -> Result<u64, String> {
        validate_feedback_samples(&self.schema, &self.bindings, samples)?;
        ensure(
            self.updated
                .is_none_or(|time| now.saturating_sub(time) >= 34),
            "Feedback rate exceeds30Hz.",
        )?;
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Feedback sequence exhausted.")?;
        self.sequence = sequence;
        self.updated = Some(now);
        if mode == FeedbackMode::Snapshot {
            self.values.clear();
            self.fill_unknown(now);
        }
        for sample in samples {
            self.values.insert(
                sample.channel_id.clone(),
                Stored {
                    sample: sample.aged(0),
                    updated: now,
                    revision: sequence,
                },
            );
        }
        Ok(sequence)
    }
    pub fn reset(&mut self, now: u64) -> Result<(), String> {
        let generation = self
            .generation
            .checked_add(1)
            .ok_or("Feedback generation exhausted.")?;
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Feedback sequence exhausted.")?;
        self.generation = generation;
        self.sequence = sequence;
        self.updated = None;
        self.values.clear();
        self.fill_unknown(now);
        Ok(())
    }
    pub fn disconnect(&mut self, now: u64) {
        if let Some(sequence) = self.sequence.checked_add(1) {
            self.sequence = sequence;
            self.updated = None;
            for b in &self.bindings {
                self.values.insert(
                    b.channel_id.clone(),
                    Stored {
                        sample: self.unknown(b, FeedbackStatus::Disconnected),
                        updated: now,
                        revision: sequence,
                    },
                );
            }
        } else {
            self.values.clear();
        }
    }
    pub fn deliver(&self, cursor: &mut FeedbackCursor, identity: &Value, now: u64) -> Value {
        let snapshot = !cursor.initialized || cursor.generation != self.generation;
        let values = self
            .values
            .values()
            .filter(|v| snapshot || v.revision > cursor.sequence)
            .map(|v| v.sample.aged(now.saturating_sub(v.updated)))
            .collect::<Vec<_>>();
        let mut frame = identity.clone();
        frame["version"] = serde_json::json!(1);
        frame["type"] = serde_json::json!("feedback");
        frame["sequence"] = serde_json::json!(self.sequence.to_string());
        frame["bindingGeneration"] = serde_json::json!(self.generation.to_string());
        frame["mode"] = serde_json::json!(if snapshot { "snapshot" } else { "delta" });
        frame["values"] = serde_json::json!(values);
        if snapshot {
            frame["bindings"] = serde_json::json!(self.bindings);
        }
        cursor.initialized = true;
        cursor.generation = self.generation;
        cursor.sequence = self.sequence;
        frame
    }
}
#[derive(Clone, Default)]
pub(crate) struct FeedbackAccumulator {
    cursor: FeedbackCursor,
    bindings: Vec<FeedbackBinding>,
    values: BTreeMap<String, FeedbackSample>,
    received: u64,
}
impl FeedbackAccumulator {
    pub fn apply(
        &mut self,
        frame: &Value,
        identity: &Value,
        schema: &[FeedbackDescriptor],
        now: u64,
    ) -> Result<Option<Vec<FeedbackSample>>, String> {
        ensure(
            serde_json::to_vec(frame).map_err(|e| e.to_string())?.len() <= FEEDBACK_BYTES,
            "Feedback envelope exceeds its bounded transfer budget.",
        )?;
        crate::hub::exact(
            frame,
            &[
                "version",
                "type",
                "clientId",
                "sessionId",
                "generation",
                "controllerInstanceId",
                "publicationHash",
                "sequence",
                "bindingGeneration",
                "mode",
                "values",
            ],
            &["bindings"],
        )?;
        ensure(
            frame["version"] == 1 && frame["type"] == "feedback",
            "Unsupported feedback version/type.",
        )?;
        for key in [
            "clientId",
            "sessionId",
            "generation",
            "controllerInstanceId",
            "publicationHash",
        ] {
            ensure(
                frame[key] == identity[key] && !frame[key].is_null(),
                "Stale or wrongly routed feedback identity.",
            )?;
        }
        let sequence = crate::hub::counter(&frame["sequence"])?;
        let generation = crate::hub::counter(&frame["bindingGeneration"])?;
        let mode: FeedbackMode =
            serde_json::from_value(frame["mode"].clone()).map_err(|_| "Invalid feedback mode.")?;
        ensure(
            !self.cursor.initialized || generation >= self.cursor.generation,
            "Stale feedback configuration generation.",
        )?;
        let bindings = if mode == FeedbackMode::Snapshot {
            serde_json::from_value::<Vec<FeedbackBinding>>(frame["bindings"].clone())
                .map_err(|_| "Snapshot must include feedback bindings.")?
        } else {
            ensure(
                frame.get("bindings").is_none()
                    && self.cursor.initialized
                    && generation == self.cursor.generation,
                "Feedback delta requires the current snapshot.",
            )?;
            self.bindings.clone()
        };
        let mut seen = BTreeSet::new();
        ensure(
            bindings.len() <= 64
                && bindings.iter().all(|b| {
                    id(&b.channel_id)
                        && id(&b.output_id)
                        && seen.insert(&b.channel_id)
                        && schema.iter().any(|d| {
                            d.channel_id == b.channel_id && d.output_ids.contains(&b.output_id)
                        })
                }),
            "Invalid feedback snapshot bindings.",
        )?;
        let samples: Vec<FeedbackSample> = serde_json::from_value(frame["values"].clone())
            .map_err(|_| "Invalid typed feedback values.")?;
        validate_feedback_samples(schema, &bindings, &samples)?;
        if self.cursor.initialized && generation == self.cursor.generation {
            ensure(sequence >= self.cursor.sequence, "Stale feedback sequence.")?;
            if sequence == self.cursor.sequence {
                return Ok(None);
            }
        }
        let mut values = if mode == FeedbackMode::Snapshot {
            BTreeMap::new()
        } else {
            self.values
                .iter()
                .map(|(id, s)| (id.clone(), s.aged(now.saturating_sub(self.received))))
                .collect()
        };
        for sample in samples {
            values.insert(sample.channel_id.clone(), sample.aged(0));
        }
        ensure(
            mode != FeedbackMode::Snapshot || values.len() == bindings.len(),
            "Incomplete feedback snapshot.",
        )?;
        self.cursor = FeedbackCursor {
            initialized: true,
            generation,
            sequence,
        };
        self.bindings = bindings;
        self.values = values;
        self.received = now;
        Ok(Some(self.values.values().cloned().collect()))
    }
}
#[cfg(test)]
#[path = "feedback_tests.rs"]
mod tests;
