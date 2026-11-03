//! Decoder for the firmware's unsolicited `@events ` CDC lines.
//!
//! The console actor removes CRLF framing and captures `received_at` before
//! taking shared locks. This decoder never reads a clock, replays an edge, or
//! advances its continuity state after a rejected frame.
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::{sync::Arc, time::Instant};

pub(crate) const MAX_LINE_BYTES: usize = 4096 + 192;
const INVALID: &str = "Invalid unsolicited USB input frame.";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Kind {
    Baseline,
    Update,
    Loss,
}

#[derive(Clone, Debug)]
pub(crate) struct Frame {
    pub boot: u32,
    pub sequence: u32,
    pub kind: Kind,
    pub state: Arc<Value>,
    pub reason: Option<String>,
    pub exhausted: bool,
    pub received_at: Instant,
    pub sampled_at_ms: u32,
    pub stream: u32,
    pub head: u32,
    pub cursor: u32,
}

fn reason<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    // An omitted reason is valid; explicit JSON null is not a wire reason.
    String::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    boot: u32,
    sequence: u32,
    kind: Kind,
    #[serde(default, deserialize_with = "reason")]
    reason: Option<String>,
    #[serde(default)]
    exhausted: bool,
    state: Value,
}

#[derive(Clone, Debug)]
struct Position {
    sequence: u32,
    stream: u32,
    head: u32,
    cursor: u32,
    oldest: u32,
    overflow: u32,
    exhausted: bool,
}

/// One decoder belongs to one verified console boot and ordered CDC connection.
pub(crate) struct Decoder {
    expected_boot: u32,
    position: Option<Position>,
}

fn uint(value: &Value, minimum: u32) -> Result<u32, String> {
    value.as_u64().and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value >= minimum).ok_or_else(|| INVALID.into())
}

fn metadata(state: &Value) -> Result<(), String> {
    if ["events", "after", "next", "more"].iter().any(|key| state.get(key).is_some()) {
        return Err(INVALID.into());
    }
    crate::panel_events::validate(state, None)
}

impl Decoder {
    pub(crate) fn new(expected_boot: u32) -> Self {
        Self { expected_boot, position: None }
    }

    pub(crate) fn decode(&mut self, line: &str, received_at: Instant) -> Result<Frame, String> {
        if line.len() > MAX_LINE_BYTES || line.chars().any(char::is_control) {
            return Err(INVALID.into());
        }
        let json = line.strip_prefix("@events ").ok_or(INVALID)?;
        let envelope: Envelope = serde_json::from_str(json).map_err(|_| INVALID)?;
        if envelope.boot == 0 || envelope.boot != self.expected_boot || envelope.sequence == 0 {
            return Err(INVALID.into());
        }
        let expected_sequence = match &self.position {
            Some(position) => position.sequence.checked_add(1).ok_or(INVALID)?,
            None => 1,
        };
        if envelope.sequence != expected_sequence {
            return Err(INVALID.into());
        }
        let stream = uint(&envelope.state["stream"], 1)?;
        let head = uint(&envelope.state["head"], 0)?;
        let oldest = uint(&envelope.state["oldest"], 1)?;
        let overflow = uint(&envelope.state["overflow"], 0)?;
        let sampled_at_ms = uint(&envelope.state["sampledAtMs"], 0)?;
        let previous = self.position.as_ref();
        let same_stream = previous.is_some_and(|position| position.stream == stream);
        if same_stream && previous.is_some_and(|position| head < position.head
            || oldest < position.oldest || overflow < position.overflow) {
            return Err(INVALID.into());
        }
        let (cursor, exhausted) = match envelope.kind {
            Kind::Baseline => {
                if previous.is_some() || envelope.reason.is_some() || envelope.exhausted {
                    return Err(INVALID.into());
                }
                metadata(&envelope.state)?;
                (head, false)
            }
            Kind::Update => {
                let previous = previous.ok_or(INVALID)?;
                if !same_stream || previous.exhausted || envelope.exhausted
                    || !matches!(envelope.reason.as_deref(), None | Some("events" | "axes" | "health")) {
                    return Err(INVALID.into());
                }
                crate::panel_events::validate(&envelope.state, Some((stream, previous.cursor)))?;
                for event in envelope.state["events"].as_array().ok_or(INVALID)? {
                    uint(&event["capturedAtMs"], 0)?;
                }
                (uint(&envelope.state["next"], 0)?, false)
            }
            Kind::Loss => {
                let previous = previous.ok_or(INVALID)?;
                metadata(&envelope.state)?;
                match envelope.reason.as_deref() {
                    Some("scopeChanged") if stream > previous.stream && !envelope.exhausted => (head, false),
                    Some("eventGap") if same_stream && !previous.exhausted && !envelope.exhausted => (head, false),
                    Some("streamExhausted") if stream >= previous.stream && envelope.exhausted => (head, true),
                    _ => return Err(INVALID.into()),
                }
            }
        };
        let position = Position { sequence: envelope.sequence, stream, head, cursor, oldest, overflow, exhausted };
        let frame = Frame { boot: envelope.boot, sequence: envelope.sequence, kind: envelope.kind,
            state: Arc::new(envelope.state), reason: envelope.reason, exhausted, received_at,
            sampled_at_ms, stream, head, cursor };
        self.position = Some(position);
        Ok(frame)
    }
}

#[cfg(test)]
#[path = "push_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "push_vectors_tests.rs"]
mod vectors_tests;
