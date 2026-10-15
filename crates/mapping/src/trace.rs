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
    Dispatch {
        requests: Vec<(String, crate::script::Invocation)>,
    },
    ScriptReply {
        id: String,
        sequence: Counter,
        epoch: Counter,
        transaction: crate::script::Transaction,
    },
    ScriptFault {
        id: String,
        reason: String,
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
    #[serde(default)]
    pub metadata: serde_json::Value,
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
        trace.validate_fingerprints()?;
        Ok(trace)
    }
    pub fn validate_fingerprints(&self) -> Result<(), String> {
        if self.metadata.is_null() {
            return Ok(());
        }
        if self.metadata["profileSha256"] != fingerprint(&self.profile)?
            || self
                .profile
                .scripts
                .iter()
                .any(|s| self.metadata["scripts"][&s.id] != digest(s.source.as_bytes()))
        {
            return Err("Trace profile or script fingerprint differs.".into());
        }
        Ok(())
    }
    pub fn replay(&self) -> Result<Vec<Recorded>, String> {
        self.replay_checked(|_, _| Ok(()))
    }
    /// Optional verifier executes the same invocation in an isolated logic worker.
    pub fn replay_checked(
        &self,
        mut verify: impl FnMut(&crate::script::Script, &crate::script::Invocation) -> Result<(), String>,
    ) -> Result<Vec<Recorded>, String> {
        if self.schema != 1 || self.steps.len() > 4096 {
            return Err("Unsupported trace schema or step count.".into());
        }
        self.validate_fingerprints()?;
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
                Step::Dispatch { requests } => {
                    let actual = runtime.script_requests();
                    if actual.len() != requests.len() {
                        return Err("Trace script dispatch count differs.".into());
                    }
                    for ((id, request), (expected_id, expected)) in actual.iter().zip(requests) {
                        let mut expected = expected.clone();
                        expected.ticket.producer.session = request.ticket.producer.session.clone();
                        expected.ticket.producer.run_generation =
                            request.ticket.producer.run_generation;
                        if id != expected_id
                            || serde_json::to_value(request).unwrap()
                                != serde_json::to_value(&expected).unwrap()
                        {
                            return Err("Trace script input/state/timing differs.".into());
                        }
                        let script = runtime
                            .engine()
                            .unwrap()
                            .profile()
                            .scripts
                            .iter()
                            .find(|s| s.id == *id)
                            .ok_or("Unknown trace script.")?;
                        verify(script, request)?;
                    }
                }
                Step::ScriptReply {
                    id,
                    sequence,
                    epoch,
                    transaction,
                } => {
                    let ticket = runtime
                        .engine()
                        .and_then(|e| e.script_inflight(id))
                        .ok_or("No dispatched trace callback.")?
                        .ticket
                        .clone();
                    if ticket.sequence != *sequence || ticket.epoch != *epoch {
                        return Err("Stale trace reply.".into());
                    }
                    runtime.script_reply(id, &ticket, transaction.clone())?;
                }
                Step::ScriptFault { id, reason } => runtime.script_fault(id, reason),
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

/// Capture stops explicitly at its bound; it never silently drops a trace prefix.
#[derive(Default)]
pub struct Capture {
    pub trace: Option<Trace>,
    pub error: Option<String>,
    bytes: usize,
}
impl Capture {
    pub fn start(profile: Profile, assignments: Vec<Assignment>) -> Self {
        let metadata = serde_json::json!({"profileSha256":fingerprint(&profile).unwrap(),"scripts":profile.scripts.iter().map(|s|(s.id.clone(),digest(s.source.as_bytes()))).collect::<std::collections::BTreeMap<_,_>>(),"declaredCapabilities":profile.roles,"nativeCapabilities":[],"timerPolicy":"complete timer set replaced on transaction commit; positive relative microseconds","coalescing":"adjacent clock advances only; input and output edges retained"});
        let trace = Trace {
            schema: 1,
            metadata,
            profile,
            assignments,
            steps: Vec::new(),
        };
        let bytes = serde_json::to_vec(&trace).unwrap().len();
        Self {
            trace: Some(trace),
            error: None,
            bytes,
        }
    }
    pub fn push(&mut self, step: Step) {
        let Some(trace) = self.trace.as_mut() else {
            return;
        };
        if self.error.is_some() {
            return;
        }
        if matches!(step, Step::Advance { .. })
            && matches!(trace.steps.last(), Some(Step::Advance { .. }))
        {
            self.bytes = self.bytes.saturating_sub(
                serde_json::to_vec(&trace.steps.pop().unwrap())
                    .unwrap()
                    .len()
                    + 1,
            );
        }
        self.bytes += serde_json::to_vec(&step).unwrap().len() + 1;
        if trace.steps.len() >= 4096 || self.bytes > TRACE_LIMIT {
            self.error = Some(
                "Trace capture exceeded 4096 steps or 4 MiB; start a shorter recording.".into(),
            );
            return;
        }
        trace.steps.push(step);
    }
    pub fn export(&self) -> Result<Vec<u8>, String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let bytes = serde_json::to_vec(self.trace.as_ref().ok_or("No recorded trace.")?)
            .map_err(|e| e.to_string())?;
        if bytes.len() > TRACE_LIMIT {
            return Err("Trace exceeds 4 MiB.".into());
        }
        Ok(bytes)
    }
}

pub fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
pub fn fingerprint(profile: &Profile) -> Result<String, String> {
    Ok(digest(
        &serde_json::to_vec(profile).map_err(|e| e.to_string())?,
    ))
}
