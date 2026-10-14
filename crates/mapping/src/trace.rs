//! Bounded synthetic trace replay through the same production engine and recording sink.
use crate::{
    engine::{Assignment, Counter, Frame, Source},
    profile::{Origin, Profile, Value},
    runtime::{Recorded, Runtime},
};
use serde::{Deserialize, Serialize};
pub const TRACE_LIMIT: usize = 4_194_304;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Step {
    Input {
        frame: Frame,
    },
    Advance {
        at: Counter,
    },
    Timer {
        node: String,
        id: String,
        at: Counter,
        value: Value,
    },
    Stop,
    Replace {
        profile: Profile,
        assignments: Vec<Assignment>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Trace {
    pub schema: u32,
    pub profile: Profile,
    pub assignments: Vec<Assignment>,
    pub steps: Vec<Step>,
}
impl Trace {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > TRACE_LIMIT {
            return Err("Mapping trace exceeds 4 MiB.".into());
        }
        let trace: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if trace.schema != 1 || trace.steps.len() > 4096 {
            return Err("Unsupported trace schema or more than 4096 steps.".into());
        }
        trace.profile.compile()?;
        Ok(trace)
    }
    pub fn replay(&self) -> Result<Vec<Recorded>, String> {
        if self.schema != 1 || self.steps.len() > 4096 {
            return Err("Unsupported trace schema or step count.".into());
        }
        let mut runtime = Runtime::replay_session();
        runtime.start(self.profile.clone(), self.assignments.clone())?;
        let mut cursor = Counter(0);
        let mut result = Vec::new();
        for step in &self.steps {
            drain(&runtime, &mut cursor, &mut result)?;
            match step {
                Step::Input { frame } => runtime.accept(frame.clone())?,
                Step::Advance { at } => runtime.advance(*at)?,
                Step::Timer {
                    node,
                    id,
                    at,
                    value,
                } => {
                    let token = runtime.engine().ok_or("No mapping run.")?.token(node)?;
                    runtime.schedule(token, id.clone(), *at, *value)?;
                }
                Step::Stop => runtime.stop(),
                Step::Replace {
                    profile,
                    assignments,
                } => {
                    runtime.start(profile.clone(), assignments.clone())?;
                }
            }
        }
        runtime.stop();
        drain(&runtime, &mut cursor, &mut result)?;
        Ok(result)
    }
}
fn drain(
    runtime: &Runtime,
    cursor: &mut Counter,
    result: &mut Vec<Recorded>,
) -> Result<(), String> {
    loop {
        let page = runtime.recording.page(*cursor, 32)?;
        if page.gap {
            return Err("Recording overflow during replay.".into());
        }
        if page.records.is_empty() {
            break;
        }
        *cursor = page.next;
        result.extend(page.records);
        if result.len() > 65_536 {
            return Err("Replay report limit reached.".into());
        }
    }
    Ok(())
}
pub fn synthetic(id: &str) -> Source {
    Source {
        origin: Origin::Synthetic,
        id: id.into(),
    }
}
#[cfg(test)]
#[path = "trace_tests.rs"]
mod tests;
