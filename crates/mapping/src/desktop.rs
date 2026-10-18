//! Portable desktop intent contracts and deterministic action scheduling.
//!
//! This module cannot inject native input. A session coordinator reduces its
//! desired owner contributions before a separate qualified adapter submits them.
use crate::{
    engine::{
        typed::{Effect, Token},
        Counter, ReportReason, RunToken,
    },
    program::{Program, SignalSample, SignalValue, SinkKind},
    signal::{PortContract, PortValue, ScalarSchema},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const MAX_ACTIONS: usize = 256;
pub const MAX_TAPS: usize = 32;
pub const MAX_DELIVERIES: usize = 4096;
pub const MAX_TRACE: usize = 4096;
pub const POINTER_TICK_US: u64 = 8_000;
pub const MAX_ELAPSED_US: u64 = 50_000;
pub const MAX_LATENESS_US: u64 = 100_000;

/// Physical USB HID keyboard usage. Characters, media and Unicode are separate
/// future domains; unsupported usages are never silently mapped to a character.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Key(pub u16);
impl Key {
    pub fn validate(self) -> Result<(), String> {
        if matches!(self.0, 0x04..=0x47 | 0x49..=0x65 | 0xe0..=0xe7) {
            Ok(())
        } else {
            Err("Unsupported physical USB HID keyboard usage.".into())
        }
    }
    pub fn is_modifier(self) -> bool {
        (0xe0..=0xe7).contains(&self.0)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    X1,
    X2,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Control {
    Key { key: Key },
    MouseButton { button: MouseButton },
}
impl Control {
    pub fn validate(self) -> Result<(), String> {
        match self {
            Self::Key { key } => key.validate(),
            _ => Ok(()),
        }
    }
    pub fn is_modifier(self) -> bool {
        matches!(self, Self::Key { key } if key.is_modifier())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PointerAxis {
    X,
    Y,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Action {
    KeyHold {
        key: Key,
    },
    KeyTap {
        key: Key,
        down_ms: u32,
        up_ms: u32,
    },
    Chord {
        keys: Vec<Key>,
    },
    MouseHold {
        button: MouseButton,
    },
    MouseClick {
        button: MouseButton,
        down_ms: u32,
        up_ms: u32,
    },
    Wheel {
        horizontal: i32,
        vertical: i32,
    },
    Pointer {
        axis: PointerAxis,
        speed: u32,
        curve: f64,
    },
    Repeat {
        key: Key,
        delay_ms: u32,
        interval_ms: u32,
    },
}
impl Action {
    pub fn validate(&self) -> Result<(), String> {
        let dwell = |down: u32, up: u32| (1..=60_000).contains(&down) && (1..=60_000).contains(&up);
        match self {
            Self::KeyHold { key } => key.validate(),
            Self::KeyTap {
                key,
                down_ms,
                up_ms,
            } if dwell(*down_ms, *up_ms) => key.validate(),
            Self::Chord { keys }
                if !keys.is_empty()
                    && keys.len() <= 8
                    && keys.iter().copied().collect::<BTreeSet<_>>().len() == keys.len() =>
            {
                for key in keys {
                    key.validate()?;
                }
                Ok(())
            }
            Self::MouseHold { .. } => Ok(()),
            Self::MouseClick { down_ms, up_ms, .. } if dwell(*down_ms, *up_ms) => Ok(()),
            Self::Wheel {
                horizontal,
                vertical,
            } if horizontal.abs_diff(0) <= 120
                && vertical.abs_diff(0) <= 120
                && (*horizontal != 0 || *vertical != 0) =>
            {
                Ok(())
            }
            Self::Pointer { speed, curve, .. }
                if (1..=20_000).contains(speed)
                    && curve.is_finite()
                    && (0.1..=5.0).contains(curve) =>
            {
                Ok(())
            }
            Self::Repeat {
                key,
                delay_ms,
                interval_ms,
            } if (1..=60_000).contains(delay_ms) && (8..=60_000).contains(interval_ms) => {
                key.validate()
            }
            _ => Err("Invalid desktop action parameters, physical key or timing.".into()),
        }
    }
    /// Exact port contract exposed by this logical desktop action.
    pub fn contract(&self) -> PortContract {
        match self {
            Self::KeyTap { .. } | Self::MouseClick { .. } | Self::Wheel { .. } => {
                PortContract::Event {
                    schema: ScalarSchema::Boolean {},
                }
            }
            Self::Pointer { .. } => PortContract::State {
                schema: ScalarSchema::Number {
                    min: -1.0,
                    max: 1.0,
                    unit: None,
                },
                freshness_ms: 1000,
                fallback: Some(PortValue::Number(0.0)),
            },
            _ => PortContract::State {
                schema: ScalarSchema::Boolean {},
                freshness_ms: 1000,
                fallback: Some(PortValue::Boolean(false)),
            },
        }
    }
    fn controls(&self) -> Vec<Control> {
        match self {
            Self::KeyHold { key } | Self::KeyTap { key, .. } | Self::Repeat { key, .. } => {
                vec![Control::Key { key: *key }]
            }
            Self::Chord { keys } => {
                let mut keys = keys.clone();
                keys.sort_by_key(|key| (!key.is_modifier(), *key));
                keys.into_iter().map(|key| Control::Key { key }).collect()
            }
            Self::MouseHold { button } | Self::MouseClick { button, .. } => {
                vec![Control::MouseButton { button: *button }]
            }
            _ => Vec::new(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActionDefinition {
    pub id: String,
    pub action: Action,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Intent {
    /// Complete contribution for this producer and action. Empty releases it.
    Held {
        controls: Vec<Control>,
    },
    Repeat {
        key: Key,
    },
    Pointer {
        dx: i32,
        dy: i32,
    },
    Wheel {
        horizontal: i32,
        vertical: i32,
    },
    /// Cleanup never expires and removes only this owner's contribution.
    Cleanup,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Delivery {
    pub producer: Token,
    pub action_id: String,
    pub operation_id: Counter,
    pub sequence: Counter,
    pub at_us: Counter,
    pub deadline_us: Option<Counter>,
    pub intent: Intent,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Decision {
    pub at_us: Counter,
    pub action_id: Option<String>,
    pub kind: String,
    pub detail: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Command {
    Accept {
        action_id: String,
        producer: Token,
        sample: SignalSample,
        at_us: Counter,
        baseline: bool,
    },
    Advance {
        at_us: Counter,
    },
    Pause {
        reason: String,
    },
    SourceLost {
        source: String,
    },
    Reconcile {
        action_id: String,
        producer: Token,
    },
    Cancel {
        action_id: String,
        reason: String,
    },
    Drain,
    Stop,
    Fault {
        message: String,
    },
}
/// Bounded portable replay input. No recorder or replay path can load an OS
/// adapter. Clock advances are part of the input rather than read during replay.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Trace {
    pub run: RunToken,
    pub definitions: Vec<ActionDefinition>,
    pub commands: Vec<Command>,
    pub dropped: Counter,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Replay {
    pub deliveries: Vec<Delivery>,
    pub decisions: Vec<Decision>,
    pub fault: Option<String>,
}
impl Trace {
    pub fn replay(&self) -> Result<Replay, String> {
        if self.commands.len() > MAX_TRACE || self.dropped.0 > 0 {
            return Err(
                "Desktop trace is incomplete or exceeds its bounded command budget.".into(),
            );
        }
        let mut scheduler = Scheduler::new(self.run.clone(), self.definitions.clone())?;
        let mut deliveries = Vec::new();
        for command in &self.commands {
            let result = match command {
                Command::Accept {
                    action_id,
                    producer,
                    sample,
                    at_us,
                    baseline,
                } => scheduler.accept(
                    action_id,
                    producer.clone(),
                    sample.clone(),
                    *at_us,
                    *baseline,
                ),
                Command::Advance { at_us } => scheduler.advance(*at_us),
                Command::Pause { reason } => {
                    scheduler.pause(reason);
                    Ok(())
                }
                Command::SourceLost { source } => {
                    scheduler.source_lost(source);
                    Ok(())
                }
                Command::Reconcile {
                    action_id,
                    producer,
                } => scheduler.reconcile_producer(action_id, producer.clone()),
                Command::Cancel { action_id, reason } => scheduler.cancel_action(action_id, reason),
                Command::Drain => {
                    deliveries.extend(scheduler.deliveries.drain(..));
                    Ok(())
                }
                Command::Stop => {
                    scheduler.stop();
                    Ok(())
                }
                Command::Fault { message } => {
                    scheduler.fail(message);
                    Ok(())
                }
            };
            if let Err(error) = result {
                if scheduler.fault().is_none() {
                    return Err(error);
                }
            }
        }
        deliveries.extend(scheduler.deliveries.drain(..));
        Ok(Replay {
            deliveries,
            decisions: scheduler.decisions(),
            fault: scheduler.fault,
        })
    }
}
#[derive(Clone)]
struct Tap {
    producer: Token,
}
#[derive(Default)]
struct State {
    producer: Option<Token>,
    armed: bool,
    held: bool,
    previous: bool,
    taps: VecDeque<Tap>,
    tap_down: bool,
    due: Option<u64>,
    repeat: Option<u64>,
    velocity: f64,
    remainder: f64,
    pointer_at: u64,
}
/// One run's deterministic scheduler. Cross-run ownership reduction is performed
/// by the desktop coordinator, never by fabricating native release edges here.
pub struct Scheduler {
    run: RunToken,
    definitions: BTreeMap<String, Action>,
    states: BTreeMap<String, State>,
    latest: BTreeMap<String, Token>,
    now: u64,
    active: bool,
    sequence: u64,
    deliveries: VecDeque<Delivery>,
    decisions: VecDeque<Decision>,
    dropped_decisions: u64,
    fault: Option<String>,
    capture: Trace,
}
impl Scheduler {
    pub fn new(run: RunToken, definitions: Vec<ActionDefinition>) -> Result<Self, String> {
        if run.generation.0 == 0
            || uuid::Uuid::parse_str(&run.session)
                .ok()
                .map(|id| id.to_string())
                .as_deref()
                != Some(&run.session)
            || definitions.len() > MAX_ACTIONS
        {
            return Err("Invalid desktop run or action budget.".into());
        }
        let capture = Trace {
            run: run.clone(),
            definitions: definitions.clone(),
            commands: Vec::new(),
            dropped: Counter(0),
        };
        let mut actions = BTreeMap::new();
        for definition in definitions {
            crate::signal::text(&definition.id, 256)?;
            definition.action.validate()?;
            if actions.insert(definition.id, definition.action).is_some() {
                return Err("Duplicate desktop action.".into());
            }
        }
        let states = actions
            .keys()
            .map(|id| (id.clone(), State::default()))
            .collect();
        Ok(Self {
            run,
            definitions: actions,
            states,
            latest: BTreeMap::new(),
            now: 0,
            active: true,
            sequence: 0,
            deliveries: VecDeque::new(),
            decisions: VecDeque::new(),
            dropped_decisions: 0,
            fault: None,
            capture,
        })
    }
    pub fn from_program(run: RunToken, program: &Program) -> Result<Self, String> {
        program.compile()?;
        Self::new(
            run,
            program
                .sinks
                .iter()
                .filter_map(|sink| {
                    if let SinkKind::Desktop { action } = &sink.kind {
                        Some(ActionDefinition {
                            id: sink.id.clone(),
                            action: action.clone(),
                        })
                    } else {
                        None
                    }
                })
                .collect(),
        )
    }
    pub fn active(&self) -> bool {
        self.active
    }
    pub fn fault(&self) -> Option<&str> {
        self.fault.as_deref()
    }
    pub fn at_us(&self) -> Counter {
        Counter(self.now)
    }
    pub fn drain(&mut self) -> Vec<Delivery> {
        let _ = self.capture(Command::Drain);
        self.deliveries.drain(..).collect()
    }
    pub fn decisions(&self) -> Vec<Decision> {
        self.decisions.iter().cloned().collect()
    }
    pub fn dropped_decisions(&self) -> Counter {
        Counter(self.dropped_decisions)
    }
    pub fn trace(&self) -> Trace {
        self.capture.clone()
    }
    fn capture(&mut self, command: Command) -> Result<(), String> {
        if self.capture.commands.len() >= MAX_TRACE {
            self.capture.dropped.0 = self.capture.dropped.0.saturating_add(1);
            if self.capture.dropped.0 == 1 {
                self.record(
                    None,
                    "trace-overflow",
                    "Recording reached its bounded command budget; replay is marked incomplete.",
                );
            }
            return Ok(());
        }
        self.capture.commands.push(command);
        Ok(())
    }
    fn record(&mut self, id: Option<&str>, kind: &str, detail: impl Into<String>) {
        if self.decisions.len() == MAX_TRACE {
            self.decisions.pop_front();
            self.dropped_decisions = self.dropped_decisions.saturating_add(1);
        }
        self.decisions.push_back(Decision {
            at_us: Counter(self.now),
            action_id: id.map(str::to_owned),
            kind: kind.into(),
            detail: detail.into(),
        });
    }
    fn emit(&mut self, id: &str, producer: Token, intent: Intent) -> Result<(), String> {
        if self.deliveries.len() >= MAX_DELIVERIES - MAX_ACTIONS
            && !matches!(intent, Intent::Cleanup)
        {
            self.fail("Desktop ordered output queue overflow; explicit restart required.");
            return Err(self.fault.clone().unwrap());
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Desktop sequence exhausted.")?;
        let cleanup = matches!(&intent, Intent::Cleanup)
            || matches!(&intent,Intent::Held{controls} if controls.is_empty());
        let deadline_us = (!cleanup).then(|| Counter(self.now.saturating_add(MAX_LATENESS_US)));
        self.deliveries.push_back(Delivery {
            producer,
            action_id: id.into(),
            operation_id: Counter(self.sequence),
            sequence: Counter(self.sequence),
            at_us: Counter(self.now),
            deadline_us,
            intent,
        });
        Ok(())
    }
    fn cancel(&mut self, id: &str, reason: &str) {
        let old = std::mem::take(self.states.get_mut(id).unwrap());
        // Revocation fences work that was produced but has not left preview yet.
        // Preserve contiguous channel identities while making it cleanup only.
        for delivery in &mut self.deliveries {
            if delivery.action_id == id {
                delivery.intent = Intent::Cleanup;
                delivery.deadline_us = None;
            }
        }
        if let Some(producer) = old.producer {
            let _ = self.emit(id, producer, Intent::Cleanup);
        }
        self.record(Some(id), "cancel", reason);
    }
    fn fail(&mut self, message: &str) {
        if !self.active {
            return;
        }
        self.active = false;
        self.fault = Some(message.into());
        // Never replay uncertain queued work. Preserve the contiguous channel
        // sequence: drained consumers can observe the fault and run cleanup.
        let ids = self.states.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            self.cancel(&id, message);
        }
        self.record(None, "fault", message);
    }
    pub fn stop(&mut self) {
        if !self.active {
            return;
        }
        let _ = self.capture(Command::Stop);
        if !self.active {
            return;
        }
        self.active = false;
        let ids = self.states.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            self.cancel(&id, "stop");
        }
        self.record(
            None,
            "stop",
            "Run stopped; timers, queued taps and velocity cancelled.",
        );
    }
    /// Pausing cancels all contributions, fractional movement and pending work.
    /// Re-entry requires fresh neutral observations before any held action arms.
    pub fn pause(&mut self, reason: &str) {
        if !self.active {
            return;
        }
        if self
            .capture(Command::Pause {
                reason: reason.into(),
            })
            .is_err()
        {
            return;
        }
        let ids = self.states.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            self.cancel(&id, reason);
        }
        self.record(None, "scope", reason);
    }
    /// Revoke a source on gap/loss before accepting fresh baseline effects.
    pub fn source_lost(&mut self, source: &str) {
        if !self.active {
            return;
        }
        if self
            .capture(Command::SourceLost {
                source: source.into(),
            })
            .is_err()
        {
            return;
        }
        let ids = self
            .states
            .iter()
            .filter(|(_, state)| {
                state.producer.as_ref().is_some_and(|token| {
                    token
                        .dependencies
                        .iter()
                        .any(|dependency| dependency.endpoint.node_id == source)
                })
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in ids {
            self.cancel(&id, "source-lost");
        }
    }
    /// Inspect validity independently of changed output values. A graph fallback
    /// can keep the same boolean while a source epoch changes underneath it.
    pub fn reconcile_producer(&mut self, id: &str, producer: Token) -> Result<(), String> {
        if !self.active || producer.run != self.run || !self.definitions.contains_key(id) {
            return Err("Unknown or stopped desktop producer reconciliation.".into());
        }
        if self
            .latest
            .get(id)
            .is_some_and(|previous| previous != &producer)
        {
            self.capture(Command::Reconcile {
                action_id: id.into(),
                producer: producer.clone(),
            })?;
            self.cancel(id, "source-dependency-revoked");
            self.latest.insert(id.into(), producer);
        }
        Ok(())
    }
    /// Cancel an expired submitted click/held action, including its queued taps,
    /// without stopping independent desktop producers in this run.
    pub fn cancel_action(&mut self, id: &str, reason: &str) -> Result<(), String> {
        if !self.active || !self.definitions.contains_key(id) {
            return Err("Unknown or stopped desktop action cancellation.".into());
        }
        self.capture(Command::Cancel {
            action_id: id.into(),
            reason: reason.into(),
        })?;
        self.cancel(id, reason);
        Ok(())
    }
    pub fn accept_effect(&mut self, effect: &Effect) -> Result<(), String> {
        if !self.definitions.contains_key(&effect.sink_id) {
            return Ok(());
        }
        if effect.run != self.run {
            return Err("Stale desktop run generation.".into());
        }
        if matches!(effect.reason, ReportReason::Stop | ReportReason::Overflow) {
            self.stop();
            return Ok(());
        }
        let producer = effect
            .producer
            .clone()
            .ok_or("Desktop effects require captured source dependencies.")?;
        self.accept(
            &effect.sink_id,
            producer,
            effect.sample.clone(),
            effect.at_us,
            matches!(
                effect.reason,
                ReportReason::Start | ReportReason::Baseline | ReportReason::SourceLost
            ),
        )
    }
    pub fn accept(
        &mut self,
        id: &str,
        producer: Token,
        sample: SignalSample,
        at: Counter,
        baseline: bool,
    ) -> Result<(), String> {
        if !self.active {
            return Err("Desktop scheduler is stopped.".into());
        }
        if producer.run != self.run {
            return Err("Stale desktop producer run generation.".into());
        }
        let action = self
            .definitions
            .get(id)
            .ok_or("Unknown desktop action.")?
            .clone();
        if at.0 < self.now {
            return Err("Desktop monotonic clock cannot move backwards.".into());
        }
        if let SignalSample::Known(value) = &sample {
            if !matches!(value,SignalValue::Scalar(value) if action.contract().scalar_schema().is_some_and(|schema|schema.accepts(value)))
            {
                return Err("Desktop action value exceeds its exact scalar contract.".into());
            }
        }
        // Revoke replaced generations before advancing timers: old work cannot
        // become eligible merely because a new source frame advanced the clock.
        if let Some(previous) = self.latest.get(id) {
            if previous != &producer {
                let stale = producer.dependencies.iter().any(|dependency| {
                    previous.dependencies.iter().any(|old| {
                        old.endpoint == dependency.endpoint
                            && (dependency.generation < old.generation
                                || (dependency.generation == old.generation
                                    && dependency.epoch < old.epoch))
                    })
                });
                if stale {
                    return Err("Stale desktop source generation or validity epoch.".into());
                }
                self.cancel(id, "producer-generation-changed");
            }
        }
        self.capture(Command::Accept {
            action_id: id.into(),
            producer: producer.clone(),
            sample: sample.clone(),
            at_us: at,
            baseline,
        })?;
        self.latest.insert(id.into(), producer.clone());
        if baseline {
            self.cancel(id, "fresh-baseline");
        }
        if !matches!(sample, SignalSample::Known(_)) {
            self.cancel(id, "source-unavailable");
            self.advance_clock(at)?;
            return Ok(());
        }
        self.advance_clock(at)?;
        let state = self.states.get_mut(id).unwrap();
        state.producer = Some(producer.clone());
        if baseline {
            state.armed = false;
            state.previous = false;
        }
        let value = if let SignalSample::Known(SignalValue::Scalar(value)) = sample {
            value
        } else {
            return Err("Desktop action requires its exact scalar contract.".into());
        };
        if !action
            .contract()
            .scalar_schema()
            .is_some_and(|schema| schema.accepts(&value))
        {
            return Err("Desktop action value exceeds its exact contract.".into());
        }
        if let Action::Pointer { speed, curve, .. } = action {
            let PortValue::Number(value) = value else {
                unreachable!()
            };
            if value == 0.0 {
                state.armed = true;
                state.remainder = 0.0;
            }
            if state.velocity == 0.0 || baseline {
                state.pointer_at = self.now;
            }
            state.velocity = if state.armed && !baseline {
                value.signum() * value.abs().powf(curve) * speed as f64
            } else {
                0.0
            };
            return Ok(());
        }
        let PortValue::Boolean(down) = value else {
            unreachable!()
        };
        let occurrence = matches!(
            action,
            Action::KeyTap { .. } | Action::MouseClick { .. } | Action::Wheel { .. }
        );
        // Event producers already require a fresh baseline at their graph input.
        // Baseline events never trigger; each subsequent true occurrence counts.
        if occurrence {
            if baseline || !down {
                state.armed = true;
                return Ok(());
            }
            match action {
                Action::Wheel {
                    horizontal,
                    vertical,
                } => {
                    self.emit(
                        id,
                        producer,
                        Intent::Wheel {
                            horizontal,
                            vertical,
                        },
                    )?;
                }
                _ => {
                    if state.taps.len() >= MAX_TAPS {
                        self.fail(
                            "Desktop per-action tap queue overflow; detents were not merged.",
                        );
                        return Err(self.fault.clone().unwrap());
                    }
                    state.taps.push_back(Tap {
                        producer: producer.clone(),
                    });
                    if state.due.is_none() {
                        let down_ms = match action {
                            Action::KeyTap { down_ms, .. } | Action::MouseClick { down_ms, .. } => {
                                down_ms
                            }
                            _ => unreachable!(),
                        };
                        state.tap_down = true;
                        state.held = true;
                        state.due = Some(self.now + down_ms as u64 * 1000);
                        self.emit(
                            id,
                            producer,
                            Intent::Held {
                                controls: action.controls(),
                            },
                        )?;
                    }
                }
            }
            return Ok(());
        }
        if !down {
            state.armed = true;
        }
        let desired = down && state.armed && !baseline;
        state.previous = down;
        if state.held != desired {
            state.held = desired;
            state.repeat = if desired {
                if let Action::Repeat { delay_ms, .. } = action {
                    Some(self.now + delay_ms as u64 * 1000)
                } else {
                    None
                }
            } else {
                None
            };
            self.emit(
                id,
                producer,
                Intent::Held {
                    controls: if desired {
                        action.controls()
                    } else {
                        Vec::new()
                    },
                },
            )?;
        }
        Ok(())
    }
    pub fn advance(&mut self, at: Counter) -> Result<(), String> {
        if at.0 < self.now {
            return Err("Desktop monotonic clock cannot move backwards.".into());
        }
        if !self.active {
            return Err("Desktop scheduler is stopped.".into());
        }
        self.capture(Command::Advance { at_us: at })?;
        self.advance_clock(at)
    }
    fn advance_clock(&mut self, at: Counter) -> Result<(), String> {
        self.now = at.0;
        self.record(None, "advance", format!("{}", at.0));
        let ids = self.definitions.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            let action = self.definitions[&id].clone();
            let state = self.states.get_mut(&id).unwrap();
            if let Some(due) = state.due.filter(|due| *due <= at.0) {
                if at.0 - due > MAX_LATENESS_US {
                    self.cancel(&id, "expired-tap");
                    continue;
                }
                let (down_ms, up_ms) = match action {
                    Action::KeyTap { down_ms, up_ms, .. }
                    | Action::MouseClick { down_ms, up_ms, .. } => (down_ms, up_ms),
                    _ => unreachable!(),
                };
                if state.tap_down {
                    state.tap_down = false;
                    state.held = false;
                    state.due = Some(at.0 + up_ms as u64 * 1000);
                    let producer = state.taps.front().unwrap().producer.clone();
                    self.emit(
                        &id,
                        producer,
                        Intent::Held {
                            controls: Vec::new(),
                        },
                    )?;
                    self.record(Some(&id), "timer", "tap-up");
                } else {
                    state.taps.pop_front();
                    if let Some(tap) = state.taps.front() {
                        let producer = tap.producer.clone();
                        state.tap_down = true;
                        state.held = true;
                        state.due = Some(at.0 + down_ms as u64 * 1000);
                        self.emit(
                            &id,
                            producer,
                            Intent::Held {
                                controls: action.controls(),
                            },
                        )?;
                        self.record(Some(&id), "timer", "tap-down");
                    } else {
                        state.due = None;
                    }
                }
            }
            let state = self.states.get_mut(&id).unwrap();
            if let Action::Repeat {
                key, interval_ms, ..
            } = action
            {
                if let Some(due) = state.repeat.filter(|due| *due <= at.0) {
                    let missed = (at.0 - due) / (interval_ms as u64 * 1000);
                    state.repeat = Some(at.0.saturating_add(interval_ms as u64 * 1000));
                    let producer = state.producer.clone().unwrap();
                    if at.0 - due <= MAX_LATENESS_US {
                        self.emit(&id, producer, Intent::Repeat { key })?;
                    }
                    self.record(
                        Some(&id),
                        "repeat",
                        format!("Skipped {missed} missed intervals; no catch-up burst."),
                    );
                }
            }
            let state = self.states.get_mut(&id).unwrap();
            if let Action::Pointer { axis, .. } = action {
                let elapsed = at.0.saturating_sub(state.pointer_at);
                if elapsed >= POINTER_TICK_US && state.velocity != 0.0 {
                    let elapsed = elapsed.min(MAX_ELAPSED_US);
                    let total = state.remainder + state.velocity * elapsed as f64 / 1_000_000.0;
                    let delta = total.trunc() as i32;
                    state.remainder = total - delta as f64;
                    state.pointer_at = at.0;
                    if delta != 0 {
                        let producer = state.producer.clone().unwrap();
                        let (dx, dy) = match axis {
                            PointerAxis::X => (delta, 0),
                            PointerAxis::Y => (0, delta),
                        };
                        self.emit(&id, producer, Intent::Pointer { dx, dy })?;
                    }
                    self.record(
                        Some(&id),
                        "pointer-integrate",
                        format!("Elapsed {elapsed} us; fractional remainder retained."),
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HatDirection {
    Up,
    Right,
    Down,
    Left,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HatDiagonal {
    CardinalOnly,
    IncludeAdjacent,
}
pub fn hat_direction(value: Option<u8>, direction: HatDirection, diagonal: HatDiagonal) -> bool {
    let cardinal = match direction {
        HatDirection::Up => 0,
        HatDirection::Right => 2,
        HatDirection::Down => 4,
        HatDirection::Left => 6,
    };
    value.is_some_and(|value| {
        value < 8
            && (value == cardinal
                || diagonal == HatDiagonal::IncludeAdjacent
                    && (value == (cardinal + 7) % 8 || value == (cardinal + 1) % 8))
    })
}

#[cfg(test)]
#[path = "desktop_tests.rs"]
mod tests;
