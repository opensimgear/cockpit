//! Typed phase of the existing mapping reducer. The clock, source ownership,
//! bounded scheduling and output lifecycle stay inside the mapping runtime.
use super::{
    Counter, Reason, Report, ReportReason, RunToken, TargetState, MAX_REPORTS, MAX_TIMERS,
};
use crate::{
    program::validation::{controller_kind, conversion, numeric, rounded, schema},
    program::*,
    signal::{ActionSemantics, PortContract, PortValue, ScalarSchema},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Change {
    pub port_id: String,
    pub sample: SignalSample,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Frame {
    pub source: String,
    pub generation: Counter,
    pub sequence: Counter,
    /// Original source evidence on the host monotonic clock. Admission supplies
    /// its current processing time separately; dequeuing never renews capture.
    pub captured_us: Counter,
    pub reason: Reason,
    pub changes: Vec<Change>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Dependency {
    pub endpoint: Endpoint,
    pub generation: Counter,
    pub epoch: Counter,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Token {
    pub run: RunToken,
    pub node_id: String,
    pub dependencies: Vec<Dependency>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Effect {
    pub run: RunToken,
    pub sequence: Counter,
    pub at_us: Counter,
    pub sink_id: String,
    pub endpoint: Endpoint,
    pub sample: SignalSample,
    pub reason: ReportReason,
    /// Captured when a desktop effect is produced, before a later source frame
    /// can replace its generation or validity epoch in the graph.
    pub producer: Option<Token>,
    /// Original native input observations, captured before a later source frame
    /// or liveness refresh can rebase the graph's processing clock.
    pub observations: Vec<Observation>,
    #[serde(skip)]
    pub native_proof: native::NativeProof,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Observation {
    pub endpoint: Endpoint,
    pub at_us: Counter,
    pub sequence: Counter,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScriptTicket {
    pub producer: Token,
    pub sequence: Counter,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptInvocation {
    pub node_id: String,
    pub ticket: ScriptTicket,
    pub at_us: Counter,
    pub callback: String,
    pub event_trigger: bool,
    pub inputs: BTreeMap<String, SignalSample>,
    pub parameters: BTreeMap<String, PortValue>,
    pub state: BTreeMap<String, PortValue>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScriptTimer {
    pub id: String,
    pub after_us: Counter,
    pub outputs: BTreeMap<String, SignalValue>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScriptTransaction {
    pub outputs: BTreeMap<String, SignalValue>,
    pub timers: Vec<ScriptTimer>,
    pub state: BTreeMap<String, PortValue>,
}
#[derive(Clone)]
struct PortState {
    sample: SignalSample,
    epoch: u64,
    observed: u64,
    captured: u64,
    captured_sequence: u64,
    proof: native::NativeProof,
}
#[derive(Clone, Default)]
struct SourceState {
    generation: u64,
    sequence: u64,
    observed: u64,
    available: bool,
}
#[derive(Default)]
struct NodeState {
    validity: Option<Token>,
    boolean: bool,
    previous: Option<SignalValue>,
    rule: crate::rules::State,
    script_state: BTreeMap<String, PortValue>,
    script_sequence: u64,
    script_started: bool,
    pending: VecDeque<ScriptInvocation>,
    inflight: Option<(ScriptTicket, bool)>,
    fault: bool,
    bindings: BTreeMap<u64, native::NativeProof>,
    rule_proof: native::NativeProof,
}
struct Timer {
    token: Token,
    outputs: BTreeMap<String, SignalValue>,
    allow_one_shot: bool,
    proof: native::NativeProof,
}
pub struct Phase {
    compiled: CompiledProgram,
    run: RunToken,
    clock: u64,
    active: bool,
    fault: Option<String>,
    sequence: u64,
    report_sequence: u64,
    sources: BTreeMap<String, SourceState>,
    values: BTreeMap<Endpoint, PortState>,
    states: BTreeMap<String, NodeState>,
    timers: BTreeMap<(u64, String, String), Timer>,
    effects: VecDeque<Effect>,
    reports: VecDeque<Report>,
    last: BTreeMap<String, SignalSample>,
    last_controllers: BTreeMap<String, TargetState>,
    desktop_neutrals: Option<BTreeMap<String, Vec<Observation>>>,
    desktop_rule_boundary: Option<Counter>,
    native: native::Ledger,
    causal: native::NativeProof,
}
fn same_status(a: &SignalSample, b: &SignalSample) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}
fn scalar(sample: &SignalSample) -> Option<&PortValue> {
    if let SignalSample::Known(SignalValue::Scalar(value)) = sample {
        Some(value)
    } else {
        None
    }
}
fn number(sample: &SignalSample) -> Option<f64> {
    if let Some(PortValue::Number(value)) = scalar(sample) {
        Some(*value)
    } else {
        None
    }
}
fn boolean(sample: &SignalSample) -> Option<bool> {
    if let Some(PortValue::Boolean(value)) = scalar(sample) {
        Some(*value)
    } else {
        None
    }
}
fn missing(inputs: &BTreeMap<String, SignalSample>) -> Option<SignalSample> {
    if inputs
        .values()
        .any(|v| matches!(v, SignalSample::Unavailable))
    {
        Some(SignalSample::Unavailable)
    } else if inputs.values().any(|v| matches!(v, SignalSample::Stale)) {
        Some(SignalSample::Stale)
    } else if inputs.values().any(|v| matches!(v, SignalSample::Unknown)) {
        Some(SignalSample::Unknown)
    } else {
        None
    }
}
pub fn accepts(contract: &PortContract, value: &SignalValue) -> bool {
    match(contract,value){
    (PortContract::State{schema,..}|PortContract::Event{schema},SignalValue::Scalar(value))=>schema.accepts(value),
    (PortContract::Invocation{parameters,..}|PortContract::Action{parameters,..},SignalValue::ActionPack(pack))=>parameters.len()==pack.arguments.len()&&parameters.iter().zip(&pack.arguments).all(|(parameter,value)|parameter.id==value.id&&parameter.schema.accepts(&value.value))&&match contract{PortContract::Action{display:Some(display),..}=>pack.arguments.first().is_some_and(|argument|matches!(&argument.value,PortValue::Text(value) if value.chars().count()<=(display.columns*display.rows)as usize)),_=>true},_=>false}
}
impl Phase {
    pub fn new(program: Program, generation: Counter, session: String) -> Result<Self, String> {
        Self::initialize(program, generation, session, 0)
    }
    /// Start a live phase on the monotonic admission clock before emitting output.
    pub fn new_at(
        program: Program,
        generation: Counter,
        session: String,
        now: Counter,
    ) -> Result<Self, String> {
        if now.0 == 0 || now.0 > u64::MAX - 600_000_000 {
            return Err("A live typed mapping needs a nonzero bounded admission clock.".into());
        }
        Self::initialize(program, generation, session, now.0)
    }
    fn initialize(
        program: Program,
        generation: Counter,
        session: String,
        clock: u64,
    ) -> Result<Self, String> {
        if generation.0 == 0
            || uuid::Uuid::parse_str(&session)
                .ok()
                .map(|id| id.to_string())
                .as_deref()
                != Some(&session)
        {
            return Err(
                "A typed mapping needs a canonical session and nonzero run generation.".into(),
            );
        }
        let compiled = program.compile()?;
        let sources = compiled
            .program
            .sources
            .iter()
            .map(|port| (port.endpoint.node_id.clone(), SourceState::default()))
            .collect();
        let values = compiled
            .contracts
            .keys()
            .cloned()
            .map(|endpoint| {
                (
                    endpoint,
                    PortState {
                        sample: SignalSample::Unknown,
                        epoch: 1,
                        observed: 0,
                        captured: 0,
                        captured_sequence: 0,
                        proof: native::NativeProof::default(),
                    },
                )
            })
            .collect();
        let states = compiled
            .program
            .nodes
            .iter()
            .map(|node| (node.id.clone(), NodeState::default()))
            .collect();
        let mut phase = Self {
            compiled,
            run: RunToken {
                session,
                generation,
            },
            clock,
            active: true,
            fault: None,
            sequence: 0,
            report_sequence: 0,
            sources,
            values,
            states,
            timers: BTreeMap::new(),
            effects: VecDeque::new(),
            reports: VecDeque::new(),
            last: BTreeMap::new(),
            last_controllers: BTreeMap::new(),
            desktop_neutrals: None,
            desktop_rule_boundary: None,
            native: native::Ledger::default(),
            causal: native::NativeProof::default(),
        };
        let events = phase.evaluate(BTreeSet::new(), BTreeSet::new(), true)?;
        phase.publish(events, ReportReason::Start, true);
        Ok(phase)
    }
    pub fn run_token(&self) -> RunToken {
        self.run.clone()
    }
    pub fn active(&self) -> bool {
        self.active
    }
    pub fn fault(&self) -> Option<&str> {
        self.fault.as_deref()
    }
    pub fn program(&self) -> &Program {
        &self.compiled.program
    }
    pub fn at_us(&self) -> Counter {
        Counter(self.clock)
    }
}
fn compare(value: f64, threshold: f64, comparison: Comparison) -> bool {
    match comparison {
        Comparison::Greater => value > threshold,
        Comparison::GreaterEqual => value >= threshold,
        Comparison::Less => value < threshold,
        Comparison::LessEqual => value <= threshold,
        Comparison::Equal => value == threshold,
        Comparison::NotEqual => value != threshold,
    }
}
fn legacy_value(sample: &SignalSample, kind: crate::profile::Kind) -> crate::profile::Value {
    match kind {
        crate::profile::Kind::Button => crate::profile::Value::Button(boolean(sample).unwrap()),
        crate::profile::Kind::Axis { .. } => {
            crate::profile::Value::Axis(number(sample).unwrap() as i32)
        }
        crate::profile::Kind::Hat => {
            let value = number(sample).unwrap();
            crate::profile::Value::Hat((value >= 0.0).then_some(value as u8))
        }
    }
}
fn from_legacy(value: crate::profile::Value) -> PortValue {
    match value {
        crate::profile::Value::Button(v) => PortValue::Boolean(v),
        crate::profile::Value::Axis(v) => PortValue::Number(v as f64),
        crate::profile::Value::Hat(v) => PortValue::Number(v.map_or(-1.0, |v| v as f64)),
    }
}

#[cfg(test)]
#[path = "typed_engine_tests.rs"]
mod tests;

#[path = "typed_engine/deadlines.rs"]
mod deadlines;

#[cfg(test)]
#[path = "typed_engine/baseline_tests.rs"]
mod baseline_tests;

#[path = "typed_engine/evaluate.rs"]
mod evaluate;

#[path = "typed_engine/native/mod.rs"]
pub mod native;
