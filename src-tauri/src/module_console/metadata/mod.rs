//! Strict decoder for compact unsolicited committed-configuration metadata.
//! The console actor supplies the receive timestamp; this module never reads a clock.
use serde::{Deserialize, Serialize};
use std::time::Instant;

pub(crate) const MAX_LINE_BYTES: usize = 384;
const INVALID: &str = "Invalid unsolicited USB metadata frame.";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Kind {
    Baseline,
    Change,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct State {
    pub generation: u32,
    pub saved_generation: u32,
    pub owner: u32,
    pub claim_expires_ms: u32,
    pub dirty: bool,
    pub pending_apply: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    boot: u32,
    sequence: u32,
    kind: Kind,
    revision: u32,
    exhausted: bool,
    state: State,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Frame {
    pub boot: u32,
    pub sequence: u32,
    pub kind: Kind,
    pub revision: u32,
    pub exhausted: bool,
    pub state: State,
    pub received_at: Instant,
}

#[derive(Clone, Debug)]
struct Position {
    sequence: u32,
    revision: u32,
    generation: u32,
    saved_generation: u32,
    exhausted: bool,
}

/// One decoder belongs to one verified console boot and ordered CDC connection.
pub(crate) struct Decoder {
    expected_boot: u32,
    position: Option<Position>,
}

impl Decoder {
    pub(crate) fn new(expected_boot: u32) -> Self {
        Self { expected_boot, position: None }
    }

    pub(crate) fn decode(&mut self, line: &str, received_at: Instant) -> Result<Frame, String> {
        if line.len() > MAX_LINE_BYTES || line.chars().any(char::is_control) {
            return Err(INVALID.into());
        }
        let json = line.strip_prefix("@meta ").ok_or(INVALID)?;
        let envelope: Envelope = serde_json::from_str(json).map_err(|_| INVALID)?;
        if envelope.boot == 0 || envelope.boot != self.expected_boot
            || envelope.sequence == 0 || envelope.revision == 0
            || envelope.sequence == u32::MAX && !envelope.exhausted
            || envelope.state.generation == 0
            || !valid_saved_generation(&envelope.state)
            || envelope.state.owner == 0 && envelope.state.claim_expires_ms != 0
        {
            return Err(INVALID.into());
        }

        match (&self.position, envelope.kind) {
            (None, Kind::Baseline) if envelope.sequence == 1 => {}
            (Some(previous), Kind::Change) if !previous.exhausted => {
                if previous.sequence.checked_add(1) != Some(envelope.sequence)
                    || envelope.state.generation < previous.generation
                    || envelope.state.saved_generation < previous.saved_generation
                    || !(envelope.revision > previous.revision
                        || envelope.exhausted && envelope.revision == previous.revision)
                {
                    return Err(INVALID.into());
                }
            }
            _ => return Err(INVALID.into()),
        }

        let frame = Frame {
            boot: envelope.boot,
            sequence: envelope.sequence,
            kind: envelope.kind,
            revision: envelope.revision,
            exhausted: envelope.exhausted,
            state: envelope.state,
            received_at,
        };
        self.position = Some(Position {
            sequence: frame.sequence,
            revision: frame.revision,
            generation: frame.state.generation,
            saved_generation: frame.state.saved_generation,
            exhausted: frame.exhausted,
        });
        Ok(frame)
    }
}

fn valid_saved_generation(state: &State) -> bool {
    state.saved_generation <= state.generation
        || state.pending_apply && state.generation.checked_add(1) == Some(state.saved_generation)
}

#[cfg(test)]
mod tests;
