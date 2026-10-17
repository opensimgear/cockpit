//! Deterministic state reducer. Time is supplied explicitly; this module performs no I/O.
use crate::profile::{
    self, Activation, Compiled, Input, Kind, Operation, Origin, OutputControl, Profile, Value,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
pub const MAX_TIMERS: usize = 256;
pub const MAX_REPORTS: usize = 4096;
#[path = "typed_engine.rs"]
pub mod typed;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Counter(pub u64);
impl Serialize for Counter {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}
impl<'de> Deserialize<'de> for Counter {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value.is_empty()
            || !value.bytes().all(|b| b.is_ascii_digit())
            || (value.len() > 1 && value.starts_with('0'))
        {
            return Err(serde::de::Error::custom(
                "Counters require canonical decimal strings.",
            ));
        }
        value
            .parse::<u64>()
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Source {
    pub origin: Origin,
    pub id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ControlAssignment {
    pub control: String,
    pub source_control: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Assignment {
    pub role: String,
    pub source: Source,
    pub controls: Vec<ControlAssignment>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    Baseline,
    Change,
    Gap,
    Disconnect,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Change {
    pub control: String,
    pub value: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Frame {
    pub source: Source,
    pub generation: Counter,
    pub sequence: Counter,
    pub captured_us: Counter,
    pub reason: Reason,
    pub changes: Vec<Change>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TargetState {
    pub buttons: Vec<bool>,
    pub axes: Vec<i32>,
    pub hats: Vec<Option<u8>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReportReason {
    Start,
    Baseline,
    Change,
    SourceLost,
    Timer,
    Stop,
    Overflow,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Report {
    pub session: String,
    pub run_generation: Counter,
    pub sequence: Counter,
    pub at_us: Counter,
    pub target: String,
    pub reason: ReportReason,
    pub state: TargetState,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceGeneration {
    pub source: Source,
    pub generation: Counter,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProducerToken {
    pub session: String,
    pub run_generation: Counter,
    pub node: String,
    pub sources: Vec<SourceGeneration>,
}
#[derive(Default)]
struct SourceState {
    generation: u64,
    sequence: u64,
    captured_us: u64,
    available: bool,
    values: BTreeMap<String, Value>,
}
#[derive(Clone)]
struct Timer {
    token: ProducerToken,
    value: Value,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunToken {
    pub session: String,
    pub generation: Counter,
}
pub struct Engine {
    session: String,
    compiled: Compiled,
    assignments: BTreeMap<String, Assignment>,
    expected: BTreeMap<Source, BTreeMap<String, Kind>>,
    sources: BTreeMap<Source, SourceState>,
    generation: u64,
    clock: u64,
    sequence: u64,
    active: bool,
    fault: Option<String>,
    armed: BTreeSet<String>,
    node_values: BTreeMap<String, Value>,
    rule_states: BTreeMap<String, crate::rules::State>,
    contributions: BTreeMap<String, Value>,
    timers: BTreeMap<(u64, String), Timer>,
    reports: VecDeque<Report>,
    last: BTreeMap<String, TargetState>,
    scripts: BTreeMap<String, crate::script::Instance>,
}
fn native_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && !id.chars().any(char::is_control)
        && ![
            "opensimgear-virtual:",
            "cockpit-virtual:",
            "osg-virtual:",
            "vjoy:",
            "uinput:",
            "macos-hid:",
        ]
        .iter()
        .any(|prefix| id.to_ascii_lowercase().starts_with(prefix))
}
type Bindings = (
    BTreeMap<String, Assignment>,
    BTreeMap<Source, BTreeMap<String, Kind>>,
);
pub fn validate_assignments(profile: &Profile, assignments: &[Assignment]) -> Result<(), String> {
    bind(&profile.compile()?, assignments.to_vec()).map(|_| ())
}
fn bind(compiled: &Compiled, assignments: Vec<Assignment>) -> Result<Bindings, String> {
    if assignments.len() != compiled.profile.roles.len() {
        return Err("An explicit assignment for every role is required.".into());
    }
    let mut assigned = BTreeMap::new();
    let mut expected: BTreeMap<Source, BTreeMap<String, Kind>> = BTreeMap::new();
    for assignment in assignments {
        let role = compiled
            .profile
            .roles
            .iter()
            .find(|r| r.id == assignment.role)
            .ok_or("Unknown assigned role.")?;
        if role.origin != assignment.source.origin
            || !native_id(&assignment.source.id)
            || assignment.controls.len() != role.controls.len()
            || assigned.contains_key(&assignment.role)
        {
            return Err("Assignments must be unique, complete and use the declared source kind; virtual sources are excluded.".into());
        }
        let mut logical = BTreeSet::new();
        let mut physical = BTreeSet::new();
        for mapping in &assignment.controls {
            let control = role
                .controls
                .iter()
                .find(|c| c.id == mapping.control)
                .ok_or("Unknown assigned logical control.")?;
            if !logical.insert(mapping.control.clone())
                || !physical.insert(mapping.source_control.clone())
                || !native_id(&mapping.source_control)
            {
                return Err("Control assignments must be explicit and unique.".into());
            }
            if expected
                .entry(assignment.source.clone())
                .or_default()
                .insert(mapping.source_control.clone(), control.kind)
                .is_some_and(|previous| previous != control.kind)
            {
                return Err("The same native control has incompatible declared kinds.".into());
            }
        }
        assigned.insert(assignment.role.clone(), assignment);
    }
    if expected.values().any(|controls| controls.len() > 256) {
        return Err(
            "A native source may declare at most 256 controls across its assigned roles.".into(),
        );
    }
    Ok((assigned, expected))
}
impl Engine {
    pub fn new(
        profile: Profile,
        assignments: Vec<Assignment>,
        generation: Counter,
        session: String,
    ) -> Result<Self, String> {
        if uuid::Uuid::parse_str(&session)
            .map(|id| id.to_string())
            .ok()
            .as_deref()
            != Some(session.as_str())
        {
            return Err("A canonical worker session UUID is required.".into());
        }
        let compiled = profile.compile()?;
        if generation.0 == 0 {
            return Err("Run generation must be nonzero.".into());
        }
        let (assigned, expected) = bind(&compiled, assignments)?;
        let sources = expected
            .keys()
            .cloned()
            .map(|source| (source, SourceState::default()))
            .collect();
        let mut engine = Self {
            session,
            compiled,
            assignments: assigned,
            expected,
            sources,
            generation: generation.0,
            clock: 1,
            sequence: 0,
            active: true,
            fault: None,
            armed: BTreeSet::new(),
            node_values: BTreeMap::new(),
            rule_states: BTreeMap::new(),
            contributions: BTreeMap::new(),
            timers: BTreeMap::new(),
            reports: VecDeque::new(),
            last: BTreeMap::new(),
            scripts: BTreeMap::new(),
        };
        for script in &engine.compiled.profile.scripts {
            engine.scripts.insert(script.id.clone(), Default::default());
        }
        engine.publish(ReportReason::Start, 1, true);
        Ok(engine)
    }
    pub fn profile(&self) -> &Profile {
        &self.compiled.profile
    }
    pub fn active(&self) -> bool {
        self.active
    }
    pub fn run_token(&self) -> RunToken {
        RunToken {
            session: self.session.clone(),
            generation: Counter(self.generation),
        }
    }
    pub fn generation(&self) -> Counter {
        Counter(self.generation)
    }
    pub fn ready(&self) -> bool {
        self.active && self.sources.values().all(|s| s.available)
    }
    pub fn fault(&self) -> Option<&str> {
        self.fault.as_deref()
    }
    pub fn contributions(&self) -> &BTreeMap<String, Value> {
        &self.contributions
    }
    pub fn drain(&mut self) -> Vec<Report> {
        self.reports.drain(..).collect()
    }
    fn affected(&self, node: &str, source: &Source) -> bool {
        self.compiled.dependencies[node]
            .iter()
            .any(|role| self.assignments[role].source == *source)
    }
    fn invalidate(&mut self, source: &Source) {
        self.invalidate_scripts(source);
        if let Some(state) = self.sources.get_mut(source) {
            state.available = false;
            state.values.clear();
        }
        let affected: BTreeSet<_> = self
            .compiled
            .profile
            .nodes
            .iter()
            .filter(|n| self.affected(&n.id, source))
            .map(|n| n.id.clone())
            .collect();
        for node in &affected {
            self.armed.remove(node);
            self.rule_states.remove(node);
            self.node_values.remove(node);
            self.contributions.remove(node);
        }
        self.timers
            .retain(|_, timer| !affected.contains(&timer.token.node));
    }
    fn input(&self, input: &Input) -> Option<Value> {
        match input {
            Input::Node { node } => self.node_values.get(node).copied(),
            Input::Source { role, control } => {
                let assignment = &self.assignments[role];
                let state = &self.sources[&assignment.source];
                if !state.available {
                    return None;
                }
                let native = &assignment
                    .controls
                    .iter()
                    .find(|c| c.control == *control)?
                    .source_control;
                state.values.get(native).copied()
            }
        }
    }
    fn evaluate(&mut self, source: Option<&Source>) {
        let mut changed = BTreeSet::new();
        for &index in &self.compiled.order {
            let node = &self.compiled.profile.nodes[index];
            if !source.is_some_and(|source| self.affected(&node.id, source))
                && !self
                    .rule_states
                    .get(&node.id)
                    .and_then(|s| s.next())
                    .is_some_and(|at| at <= self.clock)
                && !node
                    .inputs
                    .iter()
                    .any(|i| matches!(i,Input::Node{node} if changed.contains(node)))
            {
                continue;
            }
            changed.insert(node.id.clone());
            let inputs = node
                .inputs
                .iter()
                .map(|input| self.input(input))
                .collect::<Option<Vec<_>>>();
            let value = inputs.and_then(|values| match &node.operation {
                Operation::Transform { rule } => self
                    .rule_states
                    .entry(node.id.clone())
                    .or_default()
                    .apply(rule, &values, self.compiled.kinds[&node.id], self.clock),
                Operation::Direct { activation } => {
                    let mut value = values[0];
                    if let (Activation::Momentary, Value::Button(pressed)) = (activation, value) {
                        if !pressed {
                            self.armed.insert(node.id.clone());
                        }
                        value = Value::Button(pressed && self.armed.contains(&node.id));
                    }
                    Some(value)
                }
            });
            if let Some(value) = value {
                self.node_values.insert(node.id.clone(), value);
                if node.output.is_some() {
                    self.contributions.insert(node.id.clone(), value);
                }
            } else {
                self.node_values.remove(&node.id);
                self.contributions.remove(&node.id);
                self.armed.remove(&node.id);
                self.rule_states.remove(&node.id);
            }
        }
    }
    pub fn inspect(&self, offset: usize) -> serde_json::Value {
        let nodes=self.compiled.profile.nodes.iter().skip(offset).take(32).map(|node|serde_json::json!({
            "id":node.id,"inputs":node.inputs.iter().map(|i|serde_json::json!({"input":i,"value":self.input(i),"native":match i {Input::Source{role,control}=>{let a=&self.assignments[role];serde_json::json!({"source":a.source,"control":a.controls.iter().find(|c|c.control==*control).map(|c|&c.source_control)})},_=>serde_json::Value::Null}})).collect::<Vec<_>>(),
            "operation":node.operation,"output":node.output,"value":self.node_values.get(&node.id),"contribution":self.contributions.get(&node.id),
            "nextUs":self.rule_states.get(&node.id).and_then(|s|s.next()).map(Counter),
            "writers":node.output.as_ref().and_then(|o|self.compiled.writers.get(o))
        })).collect::<Vec<_>>();
        serde_json::json!({"nodes":nodes,"offset":offset,"total":self.compiled.profile.nodes.len(),"token":self.run_token(),"atUs":Counter(self.clock),"outputs":self.last})
    }
    pub fn accept(&mut self, frame: Frame) -> Result<(), String> {
        if !self.active {
            return Err("Mapping is stopped.".into());
        }
        let expected = self
            .expected
            .get(&frame.source)
            .ok_or("Unassigned input source.")?;
        let previous = &self.sources[&frame.source];
        if frame.captured_us.0 == 0
            || frame.generation.0 == 0
            || frame.sequence.0 == 0
            || frame.generation.0 < previous.generation
            || frame.captured_us.0 < previous.captured_us
        {
            return Err("Invalid or stale source generation/time.".into());
        }
        let mut changes = BTreeMap::new();
        for change in &frame.changes {
            if !expected
                .get(&change.control)
                .is_some_and(|kind| kind.accepts(change.value))
                || changes
                    .insert(change.control.clone(), change.value)
                    .is_some()
            {
                return Err("Invalid, duplicate or out-of-range source change; no partial update was applied.".into());
            }
        }
        match frame.reason {
            Reason::Baseline
                if frame.generation.0 <= previous.generation || changes.len() != expected.len() =>
            {
                return Err("A new generation and complete baseline are required.".into())
            }
            Reason::Change if changes.is_empty() => {
                return Err("A change frame must contain controls.".into())
            }
            Reason::Gap | Reason::Disconnect if !changes.is_empty() => {
                return Err("Source loss cannot contain control values.".into())
            }
            _ => {}
        }
        if frame.reason != Reason::Baseline
            && frame.generation.0 == previous.generation
            && frame.sequence.0 <= previous.sequence
        {
            return Err("Stale source sequence.".into());
        }
        let gap = frame.reason == Reason::Change
            && (frame.generation.0 != previous.generation
                || !previous.available
                || previous.sequence.checked_add(1) != Some(frame.sequence.0));
        if gap
            || matches!(
                frame.reason,
                Reason::Baseline | Reason::Gap | Reason::Disconnect
            )
        {
            self.invalidate_scripts(&frame.source);
            let affected: BTreeSet<_> = self
                .compiled
                .profile
                .nodes
                .iter()
                .filter(|node| self.affected(&node.id, &frame.source))
                .map(|node| node.id.clone())
                .collect();
            self.timers
                .retain(|_, timer| !affected.contains(&timer.token.node));
            self.rule_states.retain(|node, _| !affected.contains(node));
        }
        self.advance(Counter(self.clock.max(frame.captured_us.0)))?;
        if matches!(frame.reason, Reason::Gap | Reason::Disconnect) || gap {
            self.invalidate(&frame.source);
            self.publish(ReportReason::SourceLost, self.clock, false);
            return if gap {
                Err("Source sequence gap; contributions and timers were released. Acquire a new baseline.".into())
            } else {
                Ok(())
            };
        }
        if frame.reason == Reason::Baseline {
            self.invalidate(&frame.source);
        }
        let state = self
            .sources
            .get_mut(&frame.source)
            .expect("assigned source");
        state.generation = frame.generation.0;
        state.sequence = frame.sequence.0;
        state.captured_us = frame.captured_us.0;
        state.available = true;
        state.values.extend(changes);
        self.evaluate(Some(&frame.source));
        self.queue_scripts(Some(&frame.source), serde_json::json!({"reason":frame.reason,"source":frame.source,"sequence":frame.sequence}));
        self.publish(
            if frame.reason == Reason::Baseline {
                ReportReason::Baseline
            } else {
                ReportReason::Change
            },
            self.clock,
            false,
        );
        if !self.active {
            return Err("Mapping stopped because its report consumer fell behind.".into());
        }
        Ok(())
    }
    pub fn token(&self, node: &str) -> Result<ProducerToken, String> {
        if !self.active
            || !self
                .compiled
                .writers
                .values()
                .any(|owners| owners.iter().any(|owner| owner == node))
        {
            return Err("An active declared output producer is required.".into());
        }
        let mut sources = BTreeMap::new();
        for role in &self.compiled.dependencies[node] {
            let source = &self.assignments[role].source;
            let state = &self.sources[source];
            if !state.available {
                return Err("Producer sources require a baseline.".into());
            }
            sources.insert(source.clone(), Counter(state.generation));
        }
        Ok(ProducerToken {
            session: self.session.clone(),
            run_generation: Counter(self.generation),
            node: node.into(),
            sources: sources
                .into_iter()
                .map(|(source, generation)| SourceGeneration { source, generation })
                .collect(),
        })
    }
    fn validate_intent(&self, token: &ProducerToken, value: Value) -> Result<(), String> {
        if *token != self.token(&token.node)? || !self.compiled.kinds[&token.node].accepts(value) {
            return Err("Stale producer token or invalid output intent.".into());
        }
        Ok(())
    }
    pub fn schedule(
        &mut self,
        token: ProducerToken,
        id: String,
        at: Counter,
        value: Value,
    ) -> Result<(), String> {
        self.validate_intent(&token, value)?;
        if token.node.contains(':') {
            return Err("Script timers belong to validated script transactions.".into());
        }
        if !profile::identifier(&id)
            || at.0 < self.clock
            || at.0.saturating_sub(self.clock) > 86_400_000_000
        {
            return Err("Invalid timer ID or deadline.".into());
        }
        let owned = format!("{}:{id}", token.node);
        let replacing = self.timers.keys().any(|(_, key)| *key == owned);
        if !replacing
            && (self.timers.len()
                + self.compiled.timer_reserve
                + self.scripts.values().map(|s| s.timers.len()).sum::<usize>()
                >= MAX_TIMERS
                || self
                    .timers
                    .values()
                    .filter(|t| t.token.node == token.node)
                    .count()
                    >= 16)
        {
            return Err("Mapping timer limit reached.".into());
        }
        self.timers.retain(|(_, key), _| *key != owned);
        self.timers.insert((at.0, owned), Timer { token, value });
        Ok(())
    }
    pub fn cancel_timer(&mut self, token: &ProducerToken, id: &str) -> Result<(), String> {
        if *token != self.token(&token.node)? {
            return Err("Stale timer owner.".into());
        }
        let owned = format!("{}:{id}", token.node);
        self.timers.retain(|(_, key), _| *key != owned);
        Ok(())
    }
    pub fn advance(&mut self, now: Counter) -> Result<(), String> {
        if !self.active {
            return Err("Mapping is stopped.".into());
        }
        if now.0 < self.clock {
            return Err("Mapping time cannot move backwards.".into());
        }
        let mut transitions = 0;
        loop {
            let external = self.timers.first_key_value().map(|((at, _), _)| *at);
            let internal = self.rule_states.values().filter_map(|s| s.next()).min();
            let Some(at) = external
                .into_iter()
                .chain(internal)
                .min()
                .filter(|at| *at <= now.0)
            else {
                break;
            };
            transitions += 1;
            if transitions > MAX_REPORTS {
                self.fault = Some("Transformation timer work exceeded its bound.".into());
                self.stop();
                return Err("Transformation timer work exceeded its bound.".into());
            }
            self.clock = at;
            if internal == Some(at) {
                self.evaluate(None);
                self.publish(ReportReason::Timer, at, false);
            }
            if external == Some(at) {
                let (_, timer) = self.timers.pop_first().expect("due timer");
                if self.validate_intent(&timer.token, timer.value).is_ok() {
                    self.contributions.insert(timer.token.node, timer.value);
                    self.publish(ReportReason::Timer, at, false);
                }
            }
            if !self.active {
                return Err("Mapping report queue overflow.".into());
            }
        }
        self.clock = now.0;
        self.queue_script_timers();
        Ok(())
    }
    pub fn stop(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        self.clear_scripts();
        self.timers.clear();
        self.contributions.clear();
        self.node_values.clear();
        self.armed.clear();
        self.rule_states.clear();
        for state in self.sources.values_mut() {
            state.available = false;
            state.values.clear();
        }
        self.publish(ReportReason::Stop, self.clock, true);
    }
    fn state(&self, target: &profile::Target) -> TargetState {
        let mut state = TargetState {
            buttons: vec![false; target.layout.buttons as usize],
            axes: target.layout.axes.iter().map(|a| a.fallback).collect(),
            hats: vec![None; target.layout.hats as usize],
        };
        for (output, writers) in &self.compiled.writers {
            if output.target != target.id {
                continue;
            }
            match output.control {
                OutputControl::Button { index } => {
                    state.buttons[index as usize] = writers
                        .iter()
                        .any(|node| self.contributions.get(node) == Some(&Value::Button(true)))
                }
                OutputControl::Axis { index } => {
                    if let Some(Value::Axis(value)) =
                        writers.iter().find_map(|node| self.contributions.get(node))
                    {
                        state.axes[index as usize] = *value;
                    }
                }
                OutputControl::Hat { index } => {
                    if let Some(Value::Hat(value)) =
                        writers.iter().find_map(|node| self.contributions.get(node))
                    {
                        state.hats[index as usize] = *value;
                    }
                }
            }
        }
        state
    }
    fn publish(&mut self, reason: ReportReason, at: u64, force: bool) {
        let updates: Vec<_> = self
            .compiled
            .profile
            .targets
            .iter()
            .map(|target| (target.id.clone(), self.state(target)))
            .filter(|(id, state)| force || self.last.get(id) != Some(state))
            .collect();
        if self.reports.len() + updates.len() > MAX_REPORTS {
            self.reports.clear();
            self.active = false;
            self.clear_scripts();
            self.contributions.clear();
            self.timers.clear();
            self.rule_states.clear();
            self.node_values.clear();
            self.armed.clear();
            self.rule_states.clear();
            for state in self.sources.values_mut() {
                state.available = false;
                state.values.clear();
            }
            self.fault =
                Some("Report consumer fell behind; mapping stopped with fallback reports.".into());
            self.publish(ReportReason::Overflow, at, true);
            return;
        }
        for (target, state) in updates {
            self.sequence = self.sequence.saturating_add(1);
            self.last.insert(target.clone(), state.clone());
            self.reports.push_back(Report {
                session: self.session.clone(),
                run_generation: Counter(self.generation),
                sequence: Counter(self.sequence),
                at_us: Counter(at),
                target,
                reason,
                state,
            });
        }
    }
}
#[path = "script_engine.rs"]
mod script_engine;
#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
