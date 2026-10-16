use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AxisMode {
    Bipolar,
    Unipolar,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SourceKind {
    Button,
    Axis {
        #[serde(rename = "axisMode")]
        axis_mode: AxisMode,
        fallback: f64,
    },
    Hat,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InputValue {
    Button(bool),
    Number(f64),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceControl {
    pub id: String,
    #[serde(flatten)]
    pub kind: SourceKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceDefinition {
    pub source_id: String,
    pub name: String,
    pub publication_hash: String,
    pub controls: Vec<SourceControl>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceToken {
    pub source_id: String,
    pub session_id: String,
    pub generation: u64,
    pub publication_hash: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceChange {
    pub control_id: String,
    pub value: InputValue,
}

/// Callbacks only update bounded in-memory source state; no network I/O or mapping run lock.
pub trait SourceSink: Send + Sync {
    fn register(&self, definition: SourceDefinition) -> Result<(), String>;
    fn baseline(
        &self,
        token: &SourceToken,
        sequence: u64,
        changes: &[SourceChange],
    ) -> Result<(), String>;
    fn changes(
        &self,
        token: &SourceToken,
        sequence: u64,
        changes: &[SourceChange],
    ) -> Result<(), String>;
    fn heartbeat(&self, _token: &SourceToken) -> Result<(), String> {
        Ok(())
    }
    fn loss(&self, token: &SourceToken) -> Result<(), String>;
}

pub fn quantize_axis(value: f64, mode: AxisMode, fallback: f64) -> Result<i32, String> {
    let min = if mode == AxisMode::Bipolar { -1.0 } else { 0.0 };
    if !fallback.is_finite() || !(min..=1.0).contains(&fallback) {
        return Err("Invalid axis fallback.".into());
    }
    let value = if value.is_finite() {
        value.clamp(min, 1.0)
    } else {
        fallback
    };
    Ok((value * if value < 0.0 { 32768.0 } else { 32767.0 }).round() as i32)
}
impl SourceControl {
    pub fn fallback(&self) -> InputValue {
        match self.kind {
            SourceKind::Button => InputValue::Button(false),
            SourceKind::Hat => InputValue::Number(-1.0),
            SourceKind::Axis { fallback, .. } => InputValue::Number(fallback),
        }
    }
    pub fn accepts(&self, value: &InputValue) -> bool {
        match (&self.kind, value) {
            (SourceKind::Button, InputValue::Button(_)) => true,
            (SourceKind::Hat, InputValue::Number(n)) => {
                n.is_finite() && n.fract() == 0.0 && (-1.0..=7.0).contains(n)
            }
            (SourceKind::Axis { axis_mode, .. }, InputValue::Number(n)) => {
                n.is_finite()
                    && (if *axis_mode == AxisMode::Bipolar {
                        -1.0
                    } else {
                        0.0
                    }..=1.0)
                        .contains(n)
            }
            _ => false,
        }
    }
}
