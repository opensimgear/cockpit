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
}
#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Observation {
    pub endpoint:Endpoint,
    pub at_us:Counter,
    pub sequence:Counter,
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
    captured_sequence:u64,
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
}
struct Timer {
    token: Token,
    outputs: BTreeMap<String, SignalValue>,
    allow_one_shot: bool,
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
    /// Start a live phase on the shared native capture clock before emitting output.
    pub fn new_at(
        program: Program,
        generation: Counter,
        session: String,
        now: Counter,
    ) -> Result<Self, String> {
        if now.0 == 0 || now.0 > u64::MAX - 600_000_000 {
            return Err("A live typed mapping needs a nonzero bounded capture clock.".into());
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
                        captured:0,captured_sequence:0,
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
    /// Isolate desktop rule state from controller/game branches. Only native
    /// source snapshots cross this boundary; timers, latches, script state and
    /// previously produced effects never do.
    pub fn fork_desktop(&self) -> Result<Self, String> {
        if !self.active {
            return Err("A stopped mapping cannot seed desktop output.".into());
        }
        let mut program = self.compiled.program.clone();
        program
            .sinks
            .retain(|sink| matches!(sink.kind, SinkKind::Desktop { .. }));
        program.controllers.clear();
        let mut needed = program
            .sinks
            .iter()
            .map(|sink| sink.from.node_id.clone())
            .collect::<BTreeSet<_>>();
        loop {
            let previous = needed.len();
            for node in &program.nodes {
                if needed.contains(&node.id) {
                    needed.extend(node.inputs.iter().map(|input| input.from.node_id.clone()));
                }
            }
            if needed.len() == previous {
                break;
            }
        }
        program.nodes.retain(|node| needed.contains(&node.id));
        if program
            .nodes
            .iter()
            .any(|node| matches!(node.operation, Operation::Script { .. }))
        {
            return Err(
                "Desktop script branches remain behind their native qualification gate.".into(),
            );
        }
        let mut desktop = Self::initialize(
            program,
            self.run.generation,
            self.run.session.clone(),
            self.clock,
        )?;
        desktop.sources = self.sources.clone();
        for port in &desktop.compiled.program.sources {
            desktop
                .values
                .insert(port.endpoint.clone(), self.values[&port.endpoint].clone());
        }
        desktop.desktop_neutrals = Some(BTreeMap::new());
        desktop.refresh_all(ReportReason::Baseline)?;
        desktop.effects.clear();
        desktop.reports.clear();
        Ok(desktop)
    }
    /// Cached snapshots may display neutral, but cannot arm a stateful rule
    /// after scope return. Its effective input must observe neutral again.
    pub fn require_desktop_neutral_after(&mut self, boundary: Counter) -> Result<(), String> {
        if self.desktop_neutrals.is_none() || boundary != self.at_us() {
            return Err("A desktop neutral boundary requires its current isolated phase.".into());
        }
        self.desktop_rule_boundary = Some(boundary);
        self.timers.clear();
        for state in self.states.values_mut() {
            state.rule = crate::rules::State::default();
            state.boolean = false;
            state.previous = None;
        }
        Ok(())
    }
    /// Mirror connection evidence without replacing native samples or their
    /// capture provenance. Independent port freshness stays independent.
    pub fn mirror_source_liveness(&mut self, canonical: &Self) -> Result<(), String> {
        if self.run != canonical.run {
            return Err("Desktop source liveness belongs to another run.".into());
        }
        let mut observations = BTreeMap::<(String, u64, u64), Vec<String>>::new();
        for port in &self.compiled.program.sources {
            let source = &self.sources[&port.endpoint.node_id];
            let other_source = canonical
                .sources
                .get(&port.endpoint.node_id)
                .ok_or("Desktop source liveness does not match the captured program.")?;
            let current = &self.values[&port.endpoint];
            let other = canonical
                .values
                .get(&port.endpoint)
                .ok_or("Desktop source liveness does not match the captured program.")?;
            if source.available
                && other_source.available
                && source.generation == other_source.generation
                && current.epoch == other.epoch
                && current.captured == other.captured
                && current.captured_sequence == other.captured_sequence
                && current.sample == other.sample
                && other.observed >= self.clock
                && other.observed > current.observed
                && matches!(port.contract, PortContract::State { .. })
                && matches!(current.sample, SignalSample::Known(_))
            {
                observations
                    .entry((
                        port.endpoint.node_id.clone(),
                        source.generation,
                        other.observed,
                    ))
                    .or_default()
                    .push(port.endpoint.port_id.clone());
            }
        }
        for ((source, generation, observed), ports) in observations {
            self.observe_port_liveness(&source, Counter(generation), Counter(observed), &ports)?;
        }
        Ok(())
    }
    pub fn sample(&self, endpoint: &Endpoint) -> Option<&SignalSample> {
        self.values.get(endpoint).map(|state| &state.sample)
    }
    /// Cached graph observation only: no clock advance, input polling or drain.
    pub fn inspected_sample(
        &self,
        endpoint: &Endpoint,
    ) -> Option<(&SignalSample, Option<Counter>)> {
        self.values.get(endpoint).map(|state| {
            let observed = (state.epoch > 1
                || state.observed > 0
                || !matches!(state.sample, SignalSample::Unknown))
            .then_some(Counter(state.observed));
            (&state.sample, observed)
        })
    }
    /// Actual accepted native samples only. Liveness extends a latched state's
    /// availability but never turns an old press into fresh rearming evidence.
    pub fn source_observations(&self,token:&Token)->Vec<Observation>{
        if token.run!=self.run{return vec![];}
        let roots=token.dependencies.iter().filter(|dependency|self.sources.contains_key(&dependency.endpoint.node_id)).collect::<Vec<_>>();
        if roots.iter().any(|dependency| {
            self.sources.get(&dependency.endpoint.node_id).is_none_or(|source|!source.available||source.generation!=dependency.generation.0)
                || self.values.get(&dependency.endpoint).is_none_or(|state|state.epoch!=dependency.epoch.0||!matches!(state.sample,SignalSample::Known(_)))
        }){return vec![];}
        roots.into_iter().map(|dependency|{
            let state=&self.values[&dependency.endpoint];
            Observation{endpoint:dependency.endpoint.clone(),at_us:Counter(state.captured),sequence:Counter(state.captured_sequence)}
        }).collect()
    }
    pub fn drain(&mut self) -> Vec<Effect> {
        self.effects.drain(..).collect()
    }
    pub fn drain_reports(&mut self) -> Vec<Report> {
        self.reports.drain(..).collect()
    }
    /// A native provider can confirm that a previously admitted latched source
    /// is still connected in this exact generation, even when a stationary HID
    /// control produces no reports. This is a liveness observation, never an
    /// input sample: it cannot rearm a source, fire events or renew scripts.
    /// Call before advancing the shared clock, using native connection evidence.
    pub fn observe_liveness(
        &mut self,
        source: &str,
        generation: Counter,
        now: Counter,
    ) -> Result<(), String> {
        self.observe_selected_liveness(source,generation,now,None)
    }
    /// Refresh only native latched controls, preserving independent freshness
    /// for sampled panel axes, telemetry and event ports on the same source.
    pub fn observe_port_liveness(&mut self,source:&str,generation:Counter,now:Counter,ports:&[String])->Result<(),String>{
        self.observe_selected_liveness(source,generation,now,Some(ports))
    }
    fn observe_selected_liveness(&mut self,source:&str,generation:Counter,now:Counter,ports:Option<&[String]>)->Result<(),String>{
        if !self.active || now.0 < self.clock || now.0 > u64::MAX - 600_000_000 {
            return Err("Invalid native source liveness clock or inactive run.".into());
        }
        let state = self.sources.get(source).ok_or("Unknown native liveness source.")?;
        if !state.available || state.generation != generation.0 {
            return Err("Native liveness cannot replace a fresh current-generation baseline.".into());
        }
        let mut refreshed = BTreeSet::new();
        for port in &self.compiled.program.sources {
            if port.endpoint.node_id == source
                && ports.is_none_or(|ports|ports.contains(&port.endpoint.port_id))
                && matches!(port.contract, PortContract::State { .. })
                && matches!(self.values[&port.endpoint].sample, SignalSample::Known(_))
            {
                self.values.get_mut(&port.endpoint).unwrap().observed = now.0;
                refreshed.insert(port.endpoint.clone());
            }
        }
        for &index in &self.compiled.order {
            let node = &self.compiled.program.nodes[index];
            if matches!(node.operation, Operation::Script { .. })
                || !node.inputs.iter().any(|input| refreshed.contains(&input.from))
                || !node.inputs.iter().all(|input| {
                    matches!(self.values[&input.from].sample,SignalSample::Known(_))
                        && matches!(self.compiled.contracts[&input.from],PortContract::State{freshness_ms,..}
                            if self.values[&input.from].observed.saturating_add(freshness_ms as u64*1000)>now.0)
                })
            {
                continue;
            }
            for output in &node.outputs {
                let endpoint = Endpoint { node_id: node.id.clone(), port_id: output.id.clone() };
                if matches!(output.contract, PortContract::State { .. })
                    && matches!(self.values[&endpoint].sample, SignalSample::Known(_))
                {
                    self.values.get_mut(&endpoint).unwrap().observed = now.0;
                    refreshed.insert(endpoint);
                }
            }
        }
        Ok(())
    }
    fn put(&mut self, endpoint: &Endpoint, sample: SignalSample) -> bool {
        let state = self.values.get_mut(endpoint).expect("compiled endpoint");
        let changed = state.sample != sample;
        if !same_status(&state.sample, &sample) {
            state.epoch = state.epoch.saturating_add(1);
        }
        state.sample = sample;
        state.observed = self.clock;
        state.captured = self.clock;
        changed
    }
    fn invalidate(&mut self, source: &str, status: SignalSample) {
        if let Some(state) = self.sources.get_mut(source) {
            state.available = false;
        }
        let endpoints = self
            .compiled
            .program
            .sources
            .iter()
            .filter(|port| port.endpoint.node_id == source)
            .map(|port| port.endpoint.clone())
            .collect::<Vec<_>>();
        for endpoint in endpoints {
            self.put(&endpoint, status.clone());
        }
        self.cancel_invalid();
    }
    fn cancel_invalid(&mut self) {
        let tokens = self
            .compiled
            .program
            .nodes
            .iter()
            .filter_map(|node| {
                self.token(&node.id)
                    .ok()
                    .map(|token| (node.id.clone(), token))
            })
            .collect::<BTreeMap<_, _>>();
        self.timers
            .retain(|_, timer| tokens.get(&timer.token.node_id) == Some(&timer.token));
        for (node, state) in &mut self.states {
            if state
                .inflight
                .as_ref()
                .is_some_and(|(ticket, _)| tokens.get(node) != Some(&ticket.producer))
            {
                state.pending.clear();
                state.inflight = None;
                state.script_state.clear();
            }
            state
                .pending
                .retain(|request| tokens.get(node) == Some(&request.ticket.producer));
        }
    }
    fn valid_frame(&self, frame: &Frame, previous: &SourceState) -> bool {
        let expected = self
            .compiled
            .program
            .sources
            .iter()
            .filter(|port| port.endpoint.node_id == frame.source)
            .map(|port| (&port.endpoint.port_id, &port.contract))
            .collect::<BTreeMap<_, _>>();
        let baseline = frame.reason == Reason::Baseline;
        let mut seen = BTreeSet::new();
        frame.generation.0 > 0
            && frame.sequence.0 > 0
            && frame.captured_us.0 <= u64::MAX - 600_000_000
            && frame.captured_us.0 >= previous.observed
            && frame.changes.len() <= expected.len()
            && frame.changes.iter().all(|change| {
                let Some(contract) = expected.get(&change.port_id) else {
                    return false;
                };
                seen.insert(&change.port_id)
                    && match &change.sample {
                        SignalSample::Known(value) => accepts(contract, value),
                        _ => true,
                    }
            })
            && (!baseline || seen.len() == expected.len())
    }
    /// Admit one bounded native capture batch. Every known revocation is applied
    /// before any frame can advance timers. This is required when independent
    /// providers are drained together, rather than delivered as a causal stream.
    /// Handled continuity losses return bounded diagnostics and keep unrelated
    /// sources running; malformed envelopes fail without advancing their clocks.
    pub fn accept_batch(
        &mut self,
        frames: impl IntoIterator<Item = Frame>,
    ) -> Result<Vec<String>, String> {
        if !self.active {
            return Err("Typed mapping is stopped.".into());
        }
        let frames = frames.into_iter().take(MAX_REPORTS + 1).collect::<Vec<_>>();
        if frames.len() > MAX_REPORTS {
            self.effects.clear();
            self.reports.clear();
            self.fault = Some("Typed source batch exceeded its frame budget.".into());
            self.stop();
            return Err("Typed source batch exceeded its frame budget.".into());
        }
        struct Fence {
            first: usize,
            state: SourceState,
            status: SignalSample,
        }
        let mut simulated = self.sources.clone();
        let mut fences = BTreeMap::<String, Fence>::new();
        let mut ports = BTreeMap::<Endpoint, (usize, SignalSample)>::new();
        let mut invalid_sources = BTreeSet::new();
        let mut invalid = false;
        let mut diagnostics = BTreeSet::new();
        let mut now = self.clock;
        for (index, frame) in frames.iter().enumerate() {
            let Some(previous) = simulated.get(&frame.source).cloned() else {
                invalid = true;
                continue;
            };
            let baseline = frame.reason == Reason::Baseline;
            if !self.valid_frame(frame, &previous)
                || baseline
                    && (frame.generation.0 < previous.generation
                        || frame.generation.0 == previous.generation
                            && frame.sequence.0 <= previous.sequence)
            {
                invalid = true;
                invalid_sources.insert(frame.source.clone());
                continue;
            }
            now = now.max(frame.captured_us.0);
            let gap = previous.available
                && !baseline
                && (previous.generation != frame.generation.0
                    || previous.sequence.checked_add(1) != Some(frame.sequence.0));
            let loss = !baseline
                && (!previous.available
                    || gap
                    || matches!(frame.reason, Reason::Gap | Reason::Disconnect));
            let mut next = previous.clone();
            if loss {
                if frame.generation.0 >= next.generation {
                    next.sequence = if frame.generation.0 == next.generation {
                        next.sequence.max(frame.sequence.0)
                    } else {
                        frame.sequence.0
                    };
                    next.generation = frame.generation.0;
                }
                next.observed = frame.captured_us.0;
                next.available = false;
                fences.insert(
                    frame.source.clone(),
                    Fence {
                        first: index + 1,
                        state: next.clone(),
                        status: if frame.reason == Reason::Disconnect {
                            SignalSample::Unavailable
                        } else {
                            SignalSample::Unknown
                        },
                    },
                );
                diagnostics.insert(format!(
                    "{} lost source continuity; a newer complete baseline is required.",
                    frame.source
                ));
            } else {
                if baseline {
                    let mut state = previous;
                    state.available = false;
                    fences.insert(
                        frame.source.clone(),
                        Fence {
                            first: index,
                            state,
                            status: SignalSample::Unknown,
                        },
                    );
                }
                next.generation = frame.generation.0;
                next.sequence = frame.sequence.0;
                next.observed = frame.captured_us.0;
                next.available = true;
                for change in &frame.changes {
                    if !matches!(change.sample, SignalSample::Known(_)) {
                        ports.insert(
                            Endpoint {
                                node_id: frame.source.clone(),
                                port_id: change.port_id.clone(),
                            },
                            (index, change.sample.clone()),
                        );
                        if !baseline {
                            diagnostics.insert(format!(
                                "{} has unavailable input samples; dependent work was revoked.",
                                frame.source
                            ));
                        }
                    }
                }
            }
            simulated.insert(frame.source.clone(), next);
        }
        let mut revoked = BTreeMap::new();
        for source in &self.compiled.program.sources {
            if fences.contains_key(&source.endpoint.node_id)
                || invalid_sources.contains(&source.endpoint.node_id)
            {
                revoked.insert(
                    source.endpoint.clone(),
                    fences
                        .get(&source.endpoint.node_id)
                        .map_or(SignalSample::Unknown, |fence| fence.status.clone()),
                );
            }
        }
        for (endpoint, (index, sample)) in &ports {
            if fences
                .get(&endpoint.node_id)
                .is_none_or(|fence| *index >= fence.first)
            {
                revoked.insert(endpoint.clone(), sample.clone());
            }
        }
        let affected = self
            .compiled
            .program
            .sinks
            .iter()
            .filter(|sink| {
                revoked.contains_key(&sink.from)
                    || self
                        .compiled
                        .dependencies
                        .get(&sink.from.node_id)
                        .is_some_and(|dependencies| {
                            dependencies
                                .iter()
                                .any(|endpoint| revoked.contains_key(endpoint))
                        })
            })
            .map(|sink| sink.id.clone())
            .collect::<BTreeSet<_>>();
        let virtual_controls = self
            .compiled
            .program
            .sinks
            .iter()
            .filter(|sink| affected.contains(&sink.id))
            .filter_map(|sink| match &sink.kind {
                SinkKind::Virtual {
                    controller_id,
                    control,
                } => Some((controller_id.clone(), control.clone())),
                _ => None,
            })
            .collect::<Vec<_>>();
        // An already-produced but unconsumed action is not grandfathered across
        // loss. Preserve queued work from independent branches and targets.
        self.effects
            .retain(|effect| !affected.contains(&effect.sink_id));
        self.last.retain(|sink, _| !affected.contains(sink));
        for (source, fence) in &fences {
            self.sources.insert(source.clone(), fence.state.clone());
        }
        for source in &invalid_sources {
            self.sources.get_mut(source).unwrap().available = false;
        }
        for (endpoint, sample) in &revoked {
            // Even Unknown -> Unknown revokes tickets made while an explicit
            // fallback masked the previous loss; validity must never resurrect.
            self.values.get_mut(endpoint).unwrap().epoch =
                self.values[endpoint].epoch.saturating_add(1);
            self.put(endpoint, sample.clone());
        }
        self.cancel_invalid();
        if !revoked.is_empty() {
            let events = self.evaluate(revoked.keys().cloned().collect(), BTreeSet::new(), true)?;
            self.publish(events, ReportReason::SourceLost, false);
            // Reports contain whole virtual controllers. Clamp only revoked
            // slots so an independent queued press/release is not discarded.
            for report in &mut self.reports {
                let Some(current) = self.last_controllers.get(&report.target) else {
                    continue;
                };
                for (target, control) in &virtual_controls {
                    if target != &report.target {
                        continue;
                    }
                    match control {
                        crate::profile::OutputControl::Button { index } => {
                            report.state.buttons[*index as usize] = current.buttons[*index as usize]
                        }
                        crate::profile::OutputControl::Axis { index } => {
                            report.state.axes[*index as usize] = current.axes[*index as usize]
                        }
                        crate::profile::OutputControl::Hat { index } => {
                            report.state.hats[*index as usize] = current.hats[*index as usize]
                        }
                    }
                }
            }
        }
        if invalid {
            return Err(
                "Invalid bounded typed source batch; acquire complete native baselines.".into(),
            );
        }
        if !self.active {
            return Err("Typed output queue overflow.".into());
        }
        let mut retained = frames
            .into_iter()
            .enumerate()
            .filter(|(index, frame)| {
                fences
                    .get(&frame.source)
                    .is_none_or(|fence| *index >= fence.first)
            })
            .collect::<Vec<_>>();
        retained.sort_by_key(|(_, frame)| frame.captured_us);
        for (index, mut frame) in retained {
            if frame.reason == Reason::Baseline {
                for change in &mut frame.changes {
                    if let Some((_, sample)) = ports
                        .get(&Endpoint {
                            node_id: frame.source.clone(),
                            port_id: change.port_id.clone(),
                        })
                        .filter(|(after, _)| index < *after)
                    {
                        change.sample = sample.clone();
                    }
                }
            } else {
                frame.changes.retain(|change| {
                    ports
                        .get(&Endpoint {
                            node_id: frame.source.clone(),
                            port_id: change.port_id.clone(),
                        })
                        .is_none_or(|(after, _)| index >= *after)
                });
            }
            self.accept(frame)?;
        }
        self.advance(Counter(now.max(self.clock)))?;
        Ok(diagnostics.into_iter().collect())
    }
    pub fn accept(&mut self, frame: Frame) -> Result<(), String> {
        if !self.active {
            return Err("Typed mapping is stopped.".into());
        }
        let Some(previous) = self.sources.get(&frame.source) else {
            return Err("Unknown typed source resource.".into());
        };
        let baseline = frame.reason == Reason::Baseline;
        let valid = self.valid_frame(&frame, previous);
        if !valid {
            self.invalidate(&frame.source, SignalSample::Unknown);
            self.refresh_all(ReportReason::SourceLost)?;
            return Err("Invalid bounded typed source frame; acquire a complete baseline.".into());
        }
        let gap = previous.available
            && !baseline
            && (previous.generation != frame.generation.0
                || previous.sequence.checked_add(1) != Some(frame.sequence.0));
        if !baseline
            && (!previous.available
                || gap
                || matches!(frame.reason, Reason::Gap | Reason::Disconnect))
        {
            // Valid loss metadata still fences later baselines. Availability does
            // not erase the last observed generation, sequence or capture time.
            let source = self.sources.get_mut(&frame.source).unwrap();
            if frame.generation.0 >= source.generation {
                source.sequence = if frame.generation.0 == source.generation {
                    source.sequence.max(frame.sequence.0)
                } else {
                    frame.sequence.0
                };
                source.generation = frame.generation.0;
            }
            source.observed = frame.captured_us.0;
            self.invalidate(
                &frame.source,
                if frame.reason == Reason::Disconnect {
                    SignalSample::Unavailable
                } else {
                    SignalSample::Unknown
                },
            );
            self.refresh_all(ReportReason::SourceLost)?;
            self.advance(Counter(self.clock.max(frame.captured_us.0)))?;
            return if matches!(frame.reason, Reason::Gap | Reason::Disconnect) {
                Ok(())
            } else {
                Err(
                    "Typed source gap or generation change; acquire a complete fresh baseline."
                        .into(),
                )
            };
        }
        if baseline
            && (frame.generation.0 < previous.generation
                || previous.generation == frame.generation.0
                    && frame.sequence.0 <= previous.sequence)
        {
            return Err("An old typed baseline cannot replay retained input.".into());
        }
        if baseline {
            self.invalidate(&frame.source, SignalSample::Unknown);
            self.refresh_all(ReportReason::SourceLost)?;
        }
        self.advance(Counter(self.clock.max(frame.captured_us.0)))?;
        let state = self.sources.get_mut(&frame.source).unwrap();
        state.generation = frame.generation.0;
        state.sequence = frame.sequence.0;
        state.observed = frame.captured_us.0;
        state.available = true;
        let mut dirty = BTreeSet::new();
        let mut events = BTreeSet::new();
        for change in frame.changes {
            let endpoint = Endpoint {
                node_id: frame.source.clone(),
                port_id: change.port_id,
            };
            let event = matches!(
                self.compiled.contracts[&endpoint],
                PortContract::Event { .. }
            ) && matches!(change.sample, SignalSample::Known(_));
            let sample = if matches!(self.compiled.contracts[&endpoint],PortContract::State{freshness_ms,..} if frame.captured_us.0.saturating_add(freshness_ms as u64*1000)<=self.clock)
                && matches!(change.sample, SignalSample::Known(_))
            {
                SignalSample::Stale
            } else {
                change.sample
            };
            self.put(&endpoint, sample);
            self.values.get_mut(&endpoint).unwrap().observed = frame.captured_us.0;
            self.values.get_mut(&endpoint).unwrap().captured = frame.captured_us.0;
            self.values.get_mut(&endpoint).unwrap().captured_sequence = frame.sequence.0;
            dirty.insert(endpoint.clone());
            if event && !baseline {
                events.insert(endpoint);
            }
        }
        self.cancel_invalid();
        let events = self.evaluate(dirty, events, baseline)?;
        self.publish(
            events,
            if baseline {
                ReportReason::Baseline
            } else {
                ReportReason::Change
            },
            false,
        );
        Ok(())
    }
    fn refresh_all(&mut self, reason: ReportReason) -> Result<(), String> {
        let dirty = self
            .compiled
            .program
            .sources
            .iter()
            .map(|p| p.endpoint.clone())
            .collect();
        let events = self.evaluate(dirty, BTreeSet::new(), true)?;
        self.publish(events, reason, false);
        Ok(())
    }
    fn inputs(&self, node: &Node) -> BTreeMap<String, SignalSample> {
        node.inputs
            .iter()
            .map(|input| {
                (
                    input.port_id.clone(),
                    self.values[&input.from].sample.clone(),
                )
            })
            .collect()
    }
    fn evaluate(
        &mut self,
        mut dirty: BTreeSet<Endpoint>,
        mut events: BTreeSet<Endpoint>,
        baseline: bool,
    ) -> Result<BTreeSet<Endpoint>, String> {
        for index in self.compiled.order.clone() {
            let node = self.compiled.program.nodes[index].clone();
            let due = self.states[&node.id]
                .rule
                .next()
                .is_some_and(|at| at <= self.clock);
            if !due
                && !node.inputs.iter().any(|input| dirty.contains(&input.from))
                && !matches!(node.operation, Operation::Constant { .. })
            {
                continue;
            }
            let inputs = self.inputs(&node);
            let event_inputs = node
                .inputs
                .iter()
                .filter(|input| events.contains(&input.from))
                .map(|input| input.port_id.clone())
                .collect::<BTreeSet<_>>();
            let token = self.token(&node.id)?;
            let state = self.states.get_mut(&node.id).unwrap();
            if state
                .validity
                .as_ref()
                .is_some_and(|previous| previous != &token)
            {
                state.rule = crate::rules::State::default();
                state.previous = None;
                state.boolean = false;
                state.pending.clear();
                state.inflight = None;
                state.script_state.clear();
                state.script_started = false;
            }
            state.validity = Some(token);
            if matches!(node.operation, Operation::Script { .. }) {
                if let Some(status) = missing(&inputs) {
                    for output in &node.outputs {
                        let endpoint = Endpoint {
                            node_id: node.id.clone(),
                            port_id: output.id.clone(),
                        };
                        self.put(&endpoint, status.clone());
                        dirty.insert(endpoint);
                    }
                }
                self.enqueue_script(&node, inputs, !baseline && !event_inputs.is_empty())?;
                continue;
            }
            let outputs = match self.evaluate_node(&node, &inputs, &event_inputs, baseline) {
                Ok(outputs) => outputs,
                Err(error) => {
                    self.fault = Some(error.clone());
                    self.stop();
                    return Err(error);
                }
            };
            for (id, sample, fire) in outputs {
                let endpoint = Endpoint {
                    node_id: node.id.clone(),
                    port_id: id,
                };
                self.put(&endpoint, sample);
                dirty.insert(endpoint.clone());
                if fire {
                    events.insert(endpoint);
                }
            }
        }
        Ok(events)
    }
    fn evaluate_node(
        &mut self,
        node: &Node,
        inputs: &BTreeMap<String, SignalSample>,
        events: &BTreeSet<String>,
        baseline: bool,
    ) -> Result<Vec<(String, SignalSample, bool)>, String> {
        let output = &node.outputs[0];
        let first = inputs
            .values()
            .next()
            .cloned()
            .unwrap_or(SignalSample::Unknown);
        if let Some(boundary) = self.desktop_rule_boundary.filter(|_| matches!(node.operation,
            Operation::Builtin { .. } | Operation::Hysteresis { .. }
                | Operation::EventEdge { .. } | Operation::SetOnChange { .. }))
        {
            let observations = self.source_observations(&self.token(&node.id)?);
            if observations.is_empty()
                || observations.iter().any(|observation| observation.at_us <= boundary)
            {
                let state = self.states.get_mut(&node.id).unwrap();
                state.rule = crate::rules::State::default();
                state.boolean = false;
                state.previous = None;
                return Ok(vec![(output.id.clone(), SignalSample::Unknown, false)]);
            }
        }
        let missing = missing(inputs);
        let mut fire = false;
        let sample = match &node.operation {
            Operation::Constant { value } => SignalSample::scalar(value.clone()),
            Operation::Fallback {
                unknown,
                stale,
                unavailable,
            } => match first {
                SignalSample::Unknown => SignalSample::scalar(unknown.clone()),
                SignalSample::Stale => SignalSample::scalar(stale.clone()),
                SignalSample::Unavailable => SignalSample::scalar(unavailable.clone()),
                known => known,
            },
            Operation::Priority { order } => order
                .iter()
                .find_map(|id| {
                    matches!(inputs[id], SignalSample::Known(_)).then(|| inputs[id].clone())
                })
                .unwrap_or_else(|| missing.clone().unwrap_or(SignalSample::Unknown)),
            Operation::Any {} => {
                if inputs.values().any(|v| boolean(v) == Some(true)) {
                    SignalSample::scalar(PortValue::Boolean(true))
                } else {
                    missing
                        .clone()
                        .unwrap_or_else(|| SignalSample::scalar(PortValue::Boolean(false)))
                }
            }
            Operation::All {} => {
                if inputs.values().any(|v| boolean(v) == Some(false)) {
                    SignalSample::scalar(PortValue::Boolean(false))
                } else {
                    missing
                        .clone()
                        .unwrap_or_else(|| SignalSample::scalar(PortValue::Boolean(true)))
                }
            }
            Operation::ActionPack { arguments } | Operation::SetOnChange { arguments } => {
                if let Some(missing) = missing {
                    self.states.get_mut(&node.id).unwrap().previous = None;
                    missing
                } else {
                    let value = SignalValue::ActionPack(ActionPack {
                        arguments: arguments
                            .iter()
                            .map(|id| NamedArgument {
                                id: id.clone(),
                                value: scalar(&inputs[id]).unwrap().clone(),
                            })
                            .collect(),
                    });
                    let state = self.states.get_mut(&node.id).unwrap();
                    fire = match node.operation {
                        Operation::ActionPack { .. } => {
                            !baseline && events.iter().any(|event| !arguments.contains(event))
                        }
                        _ => state.previous.as_ref() != Some(&value),
                    };
                    state.previous = Some(value.clone());
                    SignalSample::Known(value)
                }
            }
            _ if missing.is_some() => {
                let state = self.states.get_mut(&node.id).unwrap();
                state.previous = None;
                state.boolean = false;
                state.rule = crate::rules::State::default();
                missing.unwrap()
            }
            Operation::Identity {}
            | Operation::DeclareQuantity { .. }
            | Operation::EventToState {} => {
                fire = !baseline
                    && !events.is_empty()
                    && matches!(
                        output.contract,
                        PortContract::Event { .. } | PortContract::Invocation { .. }
                    );
                first
            }
            Operation::Not {} => {
                SignalSample::scalar(PortValue::Boolean(!boolean(&first).unwrap()))
            }
            Operation::HatDirection { direction, diagonals } => {
                if let Some(value)=number(&first) {
                    SignalSample::scalar(PortValue::Boolean(crate::desktop::hat_direction(
                        (value>=0.0).then_some(value as u8),*direction,
                        if *diagonals { crate::desktop::HatDiagonal::IncludeAdjacent }else{crate::desktop::HatDiagonal::CardinalOnly})))
                }else{missing.clone().unwrap_or(SignalSample::Unknown)}
            }
            Operation::Gate {} => {
                if boolean(&inputs["gate"]) == Some(true) {
                    fire = !baseline && events.contains("value");
                    inputs["value"].clone()
                } else {
                    SignalSample::Unknown
                }
            }
            Operation::Select {
                when_true,
                when_false,
            } => SignalSample::scalar(if boolean(&first).unwrap() {
                when_true.clone()
            } else {
                when_false.clone()
            }),
            Operation::EventEdge { edge } => {
                let value = if let SignalSample::Known(value) = &first {
                    value.clone()
                } else {
                    unreachable!()
                };
                let state = self.states.get_mut(&node.id).unwrap();
                let previous = state.previous.replace(value.clone());
                fire = !baseline
                    && previous.as_ref().is_some_and(|previous| match edge {
                        Edge::Change => *previous != value,
                        Edge::Rising => {
                            matches!(previous, SignalValue::Scalar(PortValue::Boolean(false)))
                                && boolean(&first) == Some(true)
                        }
                        Edge::Falling => {
                            matches!(previous, SignalValue::Scalar(PortValue::Boolean(true)))
                                && boolean(&first) == Some(false)
                        }
                    });
                SignalSample::Known(value)
            }
            Operation::Compare {
                comparison,
                threshold,
            } => SignalSample::scalar(PortValue::Boolean(compare(
                number(&first).unwrap(),
                *threshold,
                *comparison,
            ))),
            Operation::Hysteresis {
                press,
                release,
                comparison,
            } => {
                let state = self.states.get_mut(&node.id).unwrap();
                let threshold = if state.boolean { *release } else { *press };
                let comparison = if state.boolean {
                    if *comparison == Comparison::GreaterEqual {
                        Comparison::Greater
                    } else {
                        Comparison::Less
                    }
                } else {
                    *comparison
                };
                state.boolean = compare(number(&first).unwrap(), threshold, comparison);
                SignalSample::scalar(PortValue::Boolean(state.boolean))
            }
            Operation::Builtin {
                rule,
                inputs: order,
            } => {
                let values = order
                    .iter()
                    .map(|id| {
                        legacy_value(
                            &inputs[id],
                            controller_kind(
                                &node
                                    .inputs
                                    .iter()
                                    .find(|input| &input.port_id == id)
                                    .unwrap()
                                    .contract,
                            )
                            .unwrap(),
                        )
                    })
                    .collect::<Vec<_>>();
                let kind = controller_kind(&output.contract).unwrap();
                let state = self.states.get_mut(&node.id).unwrap();
                state
                    .rule
                    .apply(rule, &values, kind, self.clock)
                    .map(from_legacy)
                    .map(SignalSample::scalar)
                    .unwrap_or(SignalSample::Unknown)
            }
            Operation::Format {
                decimals,
                prefix,
                suffix,
                rounding,
                overflow,
            } => {
                let factor = 10f64.powi(*decimals as i32);
                let value = rounded(number(&first).unwrap() * factor, *rounding) / factor;
                let mut text = format!(
                    "{prefix}{value:.precision$}{suffix}",
                    precision = *decimals as usize
                );
                let ScalarSchema::Text {
                    max_bytes,
                    encoding,
                } = schema(&output.contract).unwrap()
                else {
                    unreachable!()
                };
                if *encoding == crate::signal::TextEncoding::Ascii && !text.is_ascii() {
                    return Err("Formatted value is not ASCII.".into());
                }
                if text.len() > *max_bytes as usize {
                    if *overflow == TextOverflow::Reject {
                        return Err("Formatted value exceeds its explicit text budget.".into());
                    }
                    let mut end = (*max_bytes as usize).min(text.len());
                    while !text.is_char_boundary(end) {
                        end -= 1
                    }
                    text.truncate(end)
                }
                SignalSample::scalar(PortValue::Text(text))
            }
            operation => {
                let value =
                    number(&first).ok_or("Numeric operation received a nonnumeric scalar.")?;
                let input = &node.inputs[0].contract;
                let (min, max, _) = schema(input).and_then(numeric).unwrap();
                let result = match operation {
                    Operation::Scale { factor, offset } => value * factor + offset,
                    Operation::RangeMap {
                        input_min,
                        input_max,
                        output_min,
                        output_max,
                        rounding,
                    } => rounded(
                        (value - input_min) * (output_max - output_min) / (input_max - input_min)
                            + output_min,
                        *rounding,
                    ),
                    Operation::UnitConvert { from, to, rounding } => {
                        let (factor, offset) = conversion(from, to).unwrap();
                        rounded(value * factor + offset, *rounding)
                    }
                    Operation::Invert {} => min + max - value,
                    Operation::Deadzone {
                        center,
                        radius,
                        rounding,
                    } => rounded(
                        if (value - center).abs() <= *radius {
                            *center
                        } else if value > *center {
                            center
                                + (value - center - radius) * (max - center)
                                    / (max - center - radius)
                        } else {
                            center
                                - (center - value - radius) * (center - min)
                                    / (center - min - radius)
                        },
                        *rounding,
                    ),
                    Operation::Curve { points, rounding } => {
                        let pair = points
                            .windows(2)
                            .find(|pair| value <= pair[1].input)
                            .ok_or("Curve input exceeds its declared domain.")?;
                        rounded(
                            pair[0].output
                                + (value - pair[0].input) * (pair[1].output - pair[0].output)
                                    / (pair[1].input - pair[0].input),
                            *rounding,
                        )
                    }
                    _ => return Err("Unsupported typed operation.".into()),
                };
                SignalSample::scalar(PortValue::Number(result))
            }
        };
        if let SignalSample::Known(value) = &sample {
            if !accepts(&output.contract, value) {
                return Err(format!(
                    "Operation {} exceeded its declared output contract.",
                    node.id
                ));
            }
        }
        if matches!(output.contract, PortContract::Event { .. })
            && !matches!(node.operation, Operation::EventEdge { .. })
        {
            fire = !baseline && (!events.is_empty() || fire);
        }
        Ok(vec![(output.id.clone(), sample, fire)])
    }
    pub fn token(&self, node_id: &str) -> Result<Token, String> {
        if !self.active {
            return Err("Typed mapping is stopped.".into());
        }
        let endpoints = self
            .compiled
            .dependencies
            .get(node_id)
            .ok_or("Unknown typed producer.")?;
        if endpoints
            .iter()
            .any(|endpoint| self.values[endpoint].epoch == u64::MAX)
        {
            return Err("Typed port validity epoch exhausted.".into());
        }
        let dependencies = endpoints
            .iter()
            .map(|endpoint| Dependency {
                endpoint: endpoint.clone(),
                generation: Counter(
                    self.sources
                        .get(&endpoint.node_id)
                        .map_or(0, |source| source.generation),
                ),
                epoch: Counter(self.values[endpoint].epoch),
            })
            .collect();
        Ok(Token {
            run: self.run.clone(),
            node_id: node_id.into(),
            dependencies,
        })
    }
    /// Capture validity for an exact executable endpoint, including a direct
    /// native source port which has no processing-node entry in the graph.
    pub fn endpoint_token(&self, endpoint: &Endpoint) -> Result<Token, String> {
        if !self.active || !self.values.contains_key(endpoint) {
            return Err("Unknown or stopped typed endpoint producer.".into());
        }
        if self.compiled.dependencies.contains_key(&endpoint.node_id) {
            return self.token(&endpoint.node_id);
        }
        Ok(Token {
            run: self.run.clone(), node_id: endpoint.node_id.clone(),
            dependencies: vec![Dependency {
                endpoint: endpoint.clone(),
                generation: Counter(self.sources.get(&endpoint.node_id).map_or(0,|source|source.generation)),
                epoch: Counter(self.values[endpoint].epoch),
            }],
        })
    }
    fn validate_token(&self, token: &Token) -> Result<(), String> {
        if self.token(&token.node_id)? != *token {
            return Err(
                "Stale typed producer run, source generation or port validity epoch.".into(),
            );
        }
        Ok(())
    }
    fn validate_outputs(
        &self,
        node: &str,
        outputs: &BTreeMap<String, SignalValue>,
        allow_one_shot: bool,
    ) -> Result<(), String> {
        let definition = self
            .compiled
            .program
            .nodes
            .iter()
            .find(|n| n.id == node)
            .ok_or("Unknown output producer.")?;
        if outputs.len() > definition.outputs.len() {
            return Err("Too many producer outputs.".into());
        }
        for (id, value) in outputs {
            let port = definition
                .outputs
                .iter()
                .find(|p| p.id == *id)
                .ok_or("Undeclared producer output port.")?;
            if !accepts(&port.contract, value)
                || !allow_one_shot
                    && matches!(
                        port.contract,
                        PortContract::Invocation {
                            semantics: ActionSemantics::OneShot,
                            ..
                        }
                    )
            {
                return Err("Invalid producer output or one-shot replay from a baseline.".into());
            }
        }
        Ok(())
    }
    pub fn schedule(
        &mut self,
        token: Token,
        id: String,
        at: Counter,
        outputs: BTreeMap<String, SignalValue>,
    ) -> Result<(), String> {
        self.schedule_checked(token, id, at, outputs, false)
    }
    fn schedule_checked(
        &mut self,
        token: Token,
        id: String,
        at: Counter,
        outputs: BTreeMap<String, SignalValue>,
        allow_one_shot: bool,
    ) -> Result<(), String> {
        self.validate_token(&token)?;
        crate::signal::text(&id, 80)?;
        if at.0 < self.clock || at.0 - self.clock > 60_000_000 {
            return Err("Typed timer exceeds its explicit time bound.".into());
        }
        self.validate_outputs(&token.node_id, &outputs, allow_one_shot)?;
        let replacing = self
            .timers
            .keys()
            .any(|(_, node, known_id)| node == &token.node_id && known_id == &id);
        if !replacing && self.timers.len() + self.compiled.timer_reserve >= MAX_TIMERS {
            return Err("Typed shared timer budget exceeded.".into());
        }
        self.timers
            .retain(|(_, node, known_id), _| node != &token.node_id || known_id != &id);
        self.timers.insert(
            (at.0, token.node_id.clone(), id),
            Timer {
                token,
                outputs,
                allow_one_shot,
            },
        );
        Ok(())
    }
    fn apply_outputs(
        &mut self,
        node: &str,
        outputs: BTreeMap<String, SignalValue>,
        allow_events: bool,
        replace_outputs: bool,
    ) -> Result<(), String> {
        let mut dirty = BTreeSet::new();
        let mut events = BTreeSet::new();
        if replace_outputs {
            let omitted = self
                .compiled
                .program
                .nodes
                .iter()
                .find(|definition| definition.id == node)
                .unwrap()
                .outputs
                .iter()
                .filter(|port| !outputs.contains_key(&port.id))
                .map(|port| Endpoint {
                    node_id: node.into(),
                    port_id: port.id.clone(),
                })
                .collect::<Vec<_>>();
            for endpoint in omitted {
                self.put(&endpoint, SignalSample::Unknown);
                dirty.insert(endpoint);
            }
        }
        for (id, value) in outputs {
            let endpoint = Endpoint {
                node_id: node.into(),
                port_id: id,
            };
            self.put(&endpoint, SignalSample::Known(value));
            dirty.insert(endpoint.clone());
            if matches!(
                self.compiled.contracts[&endpoint],
                PortContract::Invocation {
                    semantics: ActionSemantics::Setter,
                    ..
                }
            ) || allow_events
                && matches!(
                    self.compiled.contracts[&endpoint],
                    PortContract::Event { .. } | PortContract::Invocation { .. }
                )
            {
                events.insert(endpoint);
            }
        }
        self.cancel_invalid();
        let events = self.evaluate(dirty, events, !allow_events)?;
        self.publish(events, ReportReason::Timer, false);
        Ok(())
    }
    pub fn advance(&mut self, now: Counter) -> Result<(), String> {
        if !self.active {
            return Err("Typed mapping is stopped.".into());
        }
        if now.0 < self.clock || now.0 > u64::MAX - 600_000_000 {
            return Err(
                "Typed mapping time cannot move backwards or exceed its safe deadline bound."
                    .into(),
            );
        }
        let mut work = 0;
        loop {
            let stale = self
                .compiled
                .contracts
                .iter()
                .filter_map(|(endpoint, contract)| match contract {
                    PortContract::State { freshness_ms, .. }
                        if matches!(self.values[endpoint].sample, SignalSample::Known(_))
                            && !self.compiled.program.nodes.iter().any(|node| {
                                node.id == endpoint.node_id
                                    && matches!(node.operation, Operation::Constant { .. })
                            }) =>
                    {
                        self.values[endpoint]
                            .observed
                            .checked_add(*freshness_ms as u64 * 1000)
                    }
                    _ => None,
                })
                .min();
            let internal = self
                .states
                .values()
                .filter_map(|state| state.rule.next())
                .min();
            let external = self.timers.first_key_value().map(|((at, ..), _)| *at);
            let Some(at) = [stale, internal, external]
                .into_iter()
                .flatten()
                .min()
                .filter(|at| *at <= now.0)
            else {
                break;
            };
            work += 1;
            if work > MAX_REPORTS {
                self.fault = Some("Typed timer work exceeded its bound.".into());
                self.stop();
                return Err("Typed timer work exceeded its bound.".into());
            }
            self.clock = at;
            if stale == Some(at) {
                let endpoints=self.compiled.contracts.iter().filter(|(endpoint,contract)|matches!(contract,PortContract::State{freshness_ms,..} if self.values[*endpoint].observed.saturating_add(*freshness_ms as u64*1000)<=at)&&matches!(self.values[*endpoint].sample,SignalSample::Known(_))&&!self.compiled.program.nodes.iter().any(|node|node.id==endpoint.node_id&&matches!(node.operation,Operation::Constant{..}))).map(|(endpoint,_)|endpoint.clone()).collect::<Vec<_>>();
                let dirty = endpoints.iter().cloned().collect();
                for endpoint in endpoints {
                    self.put(&endpoint, SignalSample::Stale);
                }
                self.cancel_invalid();
                let events = self.evaluate(dirty, BTreeSet::new(), true)?;
                self.publish(events, ReportReason::SourceLost, false);
            }
            if internal == Some(at) {
                let events = self.evaluate(BTreeSet::new(), BTreeSet::new(), false)?;
                self.publish(events, ReportReason::Timer, false);
            }
            // Expiry or a built-in transition above may have revoked this timer.
            // Never pop a later deadline using the head captured before revocation.
            if external == Some(at)
                && self
                    .timers
                    .first_key_value()
                    .is_some_and(|((deadline, ..), _)| *deadline == at)
            {
                let (_, timer) = self.timers.pop_first().unwrap();
                if self.validate_token(&timer.token).is_ok()
                    && self
                        .validate_outputs(
                            &timer.token.node_id,
                            &timer.outputs,
                            timer.allow_one_shot,
                        )
                        .is_ok()
                {
                    self.apply_outputs(
                        &timer.token.node_id,
                        timer.outputs,
                        timer.allow_one_shot,
                        false,
                    )?;
                }
            }
            if !self.active {
                return Err("Typed output queue overflow.".into());
            }
        }
        self.clock = now.0;
        Ok(())
    }
    fn enqueue_script(
        &mut self,
        node: &Node,
        inputs: BTreeMap<String, SignalSample>,
        allow_one_shot: bool,
    ) -> Result<(), String> {
        if self.states[&node.id].fault {
            return Ok(());
        }
        let queued = self
            .states
            .values()
            .map(|s| s.pending.len() + usize::from(s.inflight.is_some()))
            .sum::<usize>();
        if queued >= crate::script::QUEUE_LIMIT {
            self.script_fault(&node.id, "Typed script queue overflow.");
            return Ok(());
        }
        let token = self.token(&node.id)?;
        let state = self.states.get_mut(&node.id).unwrap();
        state.script_sequence = state
            .script_sequence
            .checked_add(1)
            .ok_or("Script sequence exhausted.")?;
        let Operation::Script { parameters, .. } = &node.operation else {
            unreachable!()
        };
        let invocation = ScriptInvocation {
            node_id: node.id.clone(),
            ticket: ScriptTicket {
                producer: token,
                sequence: Counter(state.script_sequence),
            },
            at_us: Counter(self.clock),
            callback: if state.script_started {
                "on_event"
            } else {
                "on_start"
            }
            .into(),
            event_trigger: allow_one_shot,
            inputs,
            parameters: parameters.clone(),
            state: state.script_state.clone(),
        };
        if serde_json::to_vec(&invocation)
            .map_err(|e| e.to_string())?
            .len()
            > crate::script::IPC_LIMIT
        {
            self.script_fault(&node.id, "Typed script invocation exceeds its IPC budget.");
            return Ok(());
        }
        state.script_started = true;
        state.pending.push_back(invocation);
        Ok(())
    }
    pub fn script_requests(&mut self) -> Vec<ScriptInvocation> {
        let mut requests = Vec::new();
        let mut oversized = Vec::new();
        for (node, state) in &mut self.states {
            if state.inflight.is_none() {
                if let Some(mut request) = state.pending.pop_front() {
                    request.state = state.script_state.clone();
                    if serde_json::to_vec(&request)
                        .map_or(true, |bytes| bytes.len() > crate::script::IPC_LIMIT)
                    {
                        oversized.push(node.clone());
                        continue;
                    }
                    state.inflight = Some((request.ticket.clone(), request.event_trigger));
                    requests.push(request);
                }
            }
        }
        for node in oversized {
            self.script_fault(
                &node,
                "Typed script invocation exceeds its IPC budget after state update.",
            );
        }
        requests
    }
    /// Native workers can discard stale replies and faults without affecting a
    /// newer callback from the same node.
    pub fn script_ticket_current(&self, ticket: &ScriptTicket) -> bool {
        self.active
            && self
                .states
                .get(&ticket.producer.node_id)
                .and_then(|state| state.inflight.as_ref())
                .is_some_and(|(expected, _)| expected == ticket)
            && self.validate_token(&ticket.producer).is_ok()
    }
    pub fn script_reply(
        &mut self,
        node: &str,
        ticket: &ScriptTicket,
        transaction: ScriptTransaction,
    ) -> Result<(), String> {
        if ticket.producer.node_id != node || !self.script_ticket_current(ticket) {
            return Err("Stale typed script ticket.".into());
        }
        let result = self.script_reply_inner(node, ticket, transaction);
        if let Err(error) = &result {
            self.script_fault(node, error);
        }
        result
    }
    fn script_reply_inner(
        &mut self,
        node: &str,
        ticket: &ScriptTicket,
        transaction: ScriptTransaction,
    ) -> Result<(), String> {
        let allow = self
            .states
            .get(node)
            .and_then(|state| state.inflight.as_ref())
            .filter(|(expected, _)| expected == ticket)
            .map(|(_, allow)| *allow)
            .ok_or("Stale or unexpected typed script ticket.")?;
        self.validate_token(&ticket.producer)?;
        self.validate_outputs(node, &transaction.outputs, allow)?;
        if transaction.timers.len() > MAX_TIMERS
            || self
                .timers
                .values()
                .filter(|timer| timer.token.node_id != node)
                .count()
                + self.compiled.timer_reserve
                + transaction.timers.len()
                > MAX_TIMERS
            || serde_json::to_vec(&transaction)
                .map_err(|e| e.to_string())?
                .len()
                > crate::script::IPC_LIMIT
            || serde_json::to_vec(&transaction.state)
                .map_err(|e| e.to_string())?
                .len()
                > crate::script::STATE_LIMIT
        {
            return Err("Typed script transaction exceeds a shared budget.".into());
        }
        for (key, value) in &transaction.state {
            crate::signal::text(key, 80)?;
            if matches!(value,PortValue::Number(value) if !value.is_finite()||value.abs()>crate::signal::MAX_SAFE_INTEGER as f64)
            {
                return Err("Invalid typed script state scalar.".into());
            }
        }
        let mut ids = BTreeSet::new();
        for timer in &transaction.timers {
            crate::signal::text(&timer.id, 80)?;
            if !ids.insert(&timer.id)
                || timer.after_us.0 == 0
                || timer.after_us.0 > 60_000_000
                || self.clock.checked_add(timer.after_us.0).is_none()
            {
                return Err("Invalid typed script timer.".into());
            }
            self.validate_outputs(node, &timer.outputs, allow)?;
        }
        // Every callback replaces its complete output contribution and timer set.
        // All fields and the final shared budget have passed validation above.
        self.timers.retain(|_, timer| timer.token.node_id != node);
        for timer in transaction.timers {
            self.schedule_checked(
                ticket.producer.clone(),
                timer.id,
                Counter(self.clock + timer.after_us.0),
                timer.outputs,
                allow,
            )?;
        }
        let state = self.states.get_mut(node).unwrap();
        state.script_state = transaction.state;
        state.inflight = None;
        self.apply_outputs(node, transaction.outputs, allow, true)
    }
    pub fn script_fault(&mut self, node: &str, _reason: &str) {
        if let Some(state) = self.states.get_mut(node) {
            state.fault = true;
            state.pending.clear();
            state.inflight = None;
            state.script_state.clear();
        }
        self.timers.retain(|_, timer| timer.token.node_id != node);
        let outputs = self
            .compiled
            .program
            .nodes
            .iter()
            .find(|n| n.id == node)
            .map(|n| {
                n.outputs
                    .iter()
                    .map(|port| Endpoint {
                        node_id: node.into(),
                        port_id: port.id.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let dirty = outputs.iter().cloned().collect();
        for endpoint in outputs {
            self.put(&endpoint, SignalSample::Unknown);
        }
        if let Ok(events) = self.evaluate(dirty, BTreeSet::new(), true) {
            self.publish(events, ReportReason::SourceLost, false);
        }
    }
    pub fn stop(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        self.timers.clear();
        for state in self.states.values_mut() {
            state.rule = crate::rules::State::default();
            state.pending.clear();
            state.inflight = None;
            state.script_state.clear();
        }
        for state in self.values.values_mut() {
            state.sample = SignalSample::Unavailable;
            state.epoch = state.epoch.saturating_add(1);
        }
        self.publish(BTreeSet::new(), ReportReason::Stop, true);
    }
    fn publish(&mut self, events: BTreeSet<Endpoint>, reason: ReportReason, force: bool) {
        let mut sinks = self.compiled.program.sinks.clone();
        sinks.sort_by(|a, b| a.id.cmp(&b.id));
        let updates = sinks
            .into_iter()
            .filter_map(|sink| {
                let sample = self.values[&sink.from].sample.clone();
                // A real unchanged release can precede a press in this same
                // native batch. Preserve that neutral witness only in the
                // desktop projection, and never manufacture one from liveness.
                let observed_neutral = self.desktop_neutrals.as_ref().is_some_and(|previous| {
                    matches!(sink.kind, SinkKind::Desktop { .. })
                        && (boolean(&sample) == Some(false) || number(&sample) == Some(0.0))
                        && self.endpoint_token(&sink.from).ok().is_some_and(|token| {
                            let observations = self.source_observations(&token);
                            !observations.is_empty() && previous.get(&sink.id) != Some(&observations)
                        })
                });
                let occurrence = matches!(
                    sink.contract,
                    PortContract::Event { .. } | PortContract::Action { .. }
                );
                let fire = events.contains(&sink.from);
                if occurrence && matches!(sample, SignalSample::Known(_)) && !fire && !observed_neutral {
                    return None;
                }
                if !force && !fire && !observed_neutral && self.last.get(&sink.id) == Some(&sample) {
                    return None;
                }
                Some((sink, sample))
            })
            .collect::<Vec<_>>();
        if updates.iter().any(|(sink,sample)|matches!(sample,SignalSample::Known(value)if !accepts(&sink.contract,value))){self.fault=Some("A typed sink value exceeded its exact advertised contract.".into());self.stop();return}
        if self.effects.len()
            + self.reports.len()
            + updates.len()
            + self.compiled.program.controllers.len()
            > MAX_REPORTS
        {
            self.effects.clear();
            self.reports.clear();
            self.fault =
                Some("Typed output consumer fell behind; safe fallbacks were emitted.".into());
            self.active = false;
            self.timers.clear();
            for state in self.states.values_mut() {
                state.pending.clear();
                state.inflight = None;
                state.rule = crate::rules::State::default();
            }
            for state in self.values.values_mut() {
                state.sample = SignalSample::Unavailable;
                state.epoch = state.epoch.saturating_add(1);
            }
            self.publish(BTreeSet::new(), ReportReason::Overflow, true);
            return;
        }
        for (sink, sample) in updates {
            let producer=if matches!(sink.kind,SinkKind::Desktop { .. }) {
                let endpoints=self.compiled.dependencies.get(&sink.from.node_id).cloned()
                    .unwrap_or_else(||BTreeSet::from([sink.from.clone()]));
                Some(Token { run:self.run.clone(),node_id:sink.from.node_id.clone(),
                    dependencies:endpoints.into_iter().map(|endpoint| Dependency {
                        generation:Counter(self.sources.get(&endpoint.node_id).map_or(0,|source|source.generation)),
                        epoch:Counter(self.values[&endpoint].epoch),endpoint,
                    }).collect() })
            }else{None};
            let observations=producer.as_ref().map_or_else(Vec::new,|producer|self.source_observations(producer));
            if let Some(previous) = &mut self.desktop_neutrals {
                previous.insert(sink.id.clone(), observations.clone());
            }
            self.sequence = self.sequence.saturating_add(1);
            self.last.insert(sink.id.clone(), sample.clone());
            self.effects.push_back(Effect {
                run: self.run.clone(),
                sequence: Counter(self.sequence),
                at_us: Counter(self.clock),
                sink_id: sink.id,
                endpoint: sink.endpoint,
                sample,
                reason,
                producer,
                observations,
            });
        }
        let mut controllers = self.compiled.program.controllers.clone();
        controllers.sort_by(|a, b| a.id.cmp(&b.id));
        for controller in controllers {
            let mut state = TargetState {
                buttons: vec![false; controller.layout.buttons.len()],
                axes: controller
                    .layout
                    .axes
                    .iter()
                    .map(|axis| axis.fallback)
                    .collect(),
                hats: vec![None; controller.layout.hats.len()],
            };
            for sink in &self.compiled.program.sinks {
                let SinkKind::Virtual {
                    controller_id,
                    control,
                } = &sink.kind
                else {
                    continue;
                };
                if *controller_id != controller.id {
                    continue;
                }
                let sample = &self.values[&sink.from].sample;
                match control {
                    crate::profile::OutputControl::Button { index } => {
                        state.buttons[*index as usize] = boolean(sample).unwrap_or(false)
                    }
                    crate::profile::OutputControl::Axis { index } => {
                        if let Some(value) = number(sample) {
                            state.axes[*index as usize] = value as i32
                        }
                    }
                    crate::profile::OutputControl::Hat { index } => {
                        if let Some(value) = number(sample) {
                            state.hats[*index as usize] = (value >= 0.0).then_some(value as u8)
                        }
                    }
                }
            }
            if force || self.last_controllers.get(&controller.id) != Some(&state) {
                self.report_sequence = self.report_sequence.saturating_add(1);
                self.last_controllers
                    .insert(controller.id.clone(), state.clone());
                self.reports.push_back(Report {
                    session: self.run.session.clone(),
                    run_generation: self.run.generation,
                    sequence: Counter(self.report_sequence),
                    at_us: Counter(self.clock),
                    target: controller.id,
                    reason,
                    state,
                });
            }
        }
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
