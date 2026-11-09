//! Desktop scheduler/coordinator bridge shared by native profile sessions.
//! This layer owns no profile storage, frontend events or application selector.
use osg_desktop_output::{Coordinator, Scope};
use output::Output;
use osg_mapping::{
    desktop::Scheduler,
    engine::{typed::{native::Admission, Effect, Frame, Observation, Phase, Token}, Counter, RunToken},
    program::{Program, SinkKind},
};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;
pub const OUTCOME_LIMIT: usize = 256;
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub delivery: osg_mapping::desktop::Delivery,
    pub acknowledgement: osg_desktop_output::protocol::Ack,
}
fn tap_durations(program: &Program) -> BTreeMap<String, Counter> {
    program.sinks.iter().filter_map(|sink| {
        let SinkKind::Desktop { action } = &sink.kind else { return None };
        let down_ms = match action {
            osg_mapping::desktop::Action::KeyTap { down_ms, .. }
            | osg_mapping::desktop::Action::MouseClick { down_ms, .. } => *down_ms,
            _ => return None,
        };
        Some((sink.id.clone(), Counter(u64::from(down_ms) * 1_000)))
    }).collect()
}
pub struct Desktop {
    output: Output,
    scheduler: Scheduler,
    tap_clock: Arc<dyn Fn() -> u64 + Send + Sync>,
    run: RunToken,
    scope: Option<Scope>,
    focused: bool,
    started: bool,
    registered: bool,
    last_error: Option<String>,
    outcomes: VecDeque<Outcome>,
    dropped_outcomes: u64,
    diagnostics: VecDeque<String>,
    scope_error: Option<String>,
    focus_boundary: Option<Counter>,
    waiting_neutral: BTreeSet<String>,
    rearm_evidence: BTreeMap<String, (Token, Vec<Observation>)>,
    neutral_arms: BTreeMap<String, (Token, bool)>,
    shadow: Option<Phase>,
    shadow_admission: Option<osg_desktop_output::protocol::ScopeAdmission>,
    audit_shadow: u64,
    focus_watch: focus::FocusWatch,
    authorization_deadline: Option<Counter>,
}
impl Desktop {
    pub fn acquire(
        run: RunToken,
        program: &Program,
        enabled: bool,
        qualified: bool,
        journal: std::path::PathBuf,
        scope: Option<Scope>,
        wake: std::sync::Arc<dyn Fn() + Send + Sync>,
        tap_clock: Arc<dyn Fn() -> u64 + Send + Sync>,
    ) -> Result<Self, String> {
        let mut scheduler = Scheduler::from_program(run.clone(), program)?;
        let tap_durations = tap_durations(program);
        if !tap_durations.is_empty() {
            scheduler.use_native_tap_timing()?;
        }
        let output = Coordinator::spawn_with_owned_journal(
            enabled,
            qualified,
            journal,
            run.clone(),
            scope.clone(),
        )?;
        Ok(Self {
            output: Output::threaded(output, wake, tap_durations)?,
            scheduler,
            tap_clock,
            run,
            scope,
            focused: false,
            started: false,
            registered: false,
            last_error: None,
            outcomes: VecDeque::new(),
            dropped_outcomes: 0,
            diagnostics: VecDeque::new(),
            scope_error: None,
            focus_boundary: None,
            waiting_neutral: BTreeSet::new(),
            rearm_evidence: BTreeMap::new(),
            neutral_arms: BTreeMap::new(),
            shadow: None,
            shadow_admission: None,
            audit_shadow: 0,
            focus_watch: Default::default(),
            authorization_deadline: None,
        })
    }
    pub fn scope(&self) -> Option<&Scope> {
        self.scope.as_ref()
    }
    fn audit_effect(&self, effect: &Effect) -> Option<u64> {
        use osg_desktop_output::audit::{self, AliasKind, Fault, Kind};
        if !audit::enabled() { return None; }
        let producer = effect.producer.as_ref()?;
        let id = audit::next_id()?;
        let run = audit::alias(AliasKind::Run, effect.run.session.as_bytes())?;
        let mut roots = Vec::with_capacity(effect.observations.len());
        for observation in &effect.observations {
            let mut dependencies = producer.dependencies.iter().filter(|dependency| dependency.endpoint == observation.endpoint);
            let Some(dependency) = dependencies.next() else { audit::fault(Fault::Provenance); return None; };
            if dependencies.next().is_some() { audit::fault(Fault::Provenance); return None; }
            roots.push([
                id,
                audit::alias(AliasKind::LogicalSource, observation.endpoint.node_id.as_bytes())?,
                audit::alias(AliasKind::Port, observation.endpoint.port_id.as_bytes())?,
                dependency.generation.0, dependency.epoch.0,
                observation.sequence.0, observation.at_us.0,
            ]);
        }
        audit::record(Kind::Effect, &[id, self.audit_shadow, run, effect.run.generation.0, effect.sequence.0, roots.len() as u64]);
        for root in roots { audit::record(Kind::EffectRoot, &root); }
        Some(id)
    }
    fn audit_drop(&self, delivery: &osg_mapping::desktop::Delivery, reason: u64) {
        self.audit_drop_for(delivery, reason, self.audit_shadow);
    }
    fn audit_drop_for(&self, delivery: &osg_mapping::desktop::Delivery, reason: u64, shadow: u64) {
        use osg_desktop_output::audit::{self, AliasKind, Kind};
        if !audit::enabled() { return; }
        if let Some(run) = audit::alias(AliasKind::Run, delivery.producer.run.session.as_bytes()) {
            audit::record(Kind::DeliveryDropped, &[shadow, run, delivery.producer.run.generation.0, delivery.operation_id.0, reason]);
        }
    }
    pub fn poll(&mut self, admissions: &[Admission], reducer: &Phase) -> Result<Vec<String>, String> {
        self.poll_scope(admissions, reducer)
    }
    #[cfg(test)]
    fn poll_checked(
        &mut self,
        frames: &[Frame],
        reducer: &Phase,
        eligibility: Result<bool, String>,
    ) -> Result<Vec<String>, String> {
        let admissions = frames.iter().cloned().map(Into::into).collect::<Vec<Admission>>();
        self.poll_admissions_checked(&admissions, reducer, eligibility)
    }
    fn poll_admissions_checked(
        &mut self,
        admissions: &[Admission],
        reducer: &Phase,
        eligibility: Result<bool, String>,
    ) -> Result<Vec<String>, String> {
        match eligibility {
            Ok(focused) => {
                self.scope_error = None;
                self.poll_admissions_focused(admissions, reducer, focused)
            }
            Err(error) => {
                let mut diagnostics = self.poll_admissions_focused(admissions, reducer, false)?;
                self.scope_error = Some(error.clone());
                diagnostics.push(format!("Keyboard and mouse output paused: {error}"));
                Ok(diagnostics)
            }
        }
    }
    #[cfg(test)]
    fn poll_focused(
        &mut self,
        frames: &[Frame],
        reducer: &Phase,
        focused: bool,
    ) -> Result<Vec<String>, String> {
        let admissions = frames.iter().cloned().map(Into::into).collect::<Vec<Admission>>();
        self.poll_admissions_focused(&admissions, reducer, focused)
    }
    fn poll_admissions_focused_inner(
        &mut self,
        admissions: &[Admission],
        reducer: &Phase,
        focused: bool,
    ) -> Result<Vec<String>, String> {
        if !self.started {
            return Ok(vec![]);
        }
        // A healthy parent also keeps a paused registration alive. Renewal
        // never arms an action; the fresh-neutral path still controls input.
        self.output.renew(self.run.clone())?;
        self.authorization_deadline = Some(Counter(reducer.at_us().0.saturating_add(100_000)));
        if !focused {
            self.neutral_arms.clear();
            if self.focused {
                self.scheduler
                    .pause("Application focus lost; pending actions and motion cancelled.");
                self.flush(reducer.at_us().0)?;
                self.output.pause(self.run.clone())?;
            }
            self.shadow = None;
            self.focused = false;
            // Losing focus is expected application scoping. The paused status
            // remains inspectable, while only actual delivery failures affect
            // profile health.
            return Ok(vec![]);
        }
        if !self.focused {
            let admission = self.output.child_scope().map(|receipt| receipt.admission);
            self.focused = true;
            self.scheduler
                .pause("Application scope returned; require a fresh neutral baseline.");
            self.focus_boundary = Some(reducer.at_us());
            self.waiting_neutral = reducer
                .program()
                .sinks
                .iter()
                .filter(|sink| matches!(sink.kind, SinkKind::Desktop { .. }))
                .map(|sink| sink.id.clone())
                .collect();
            self.rearm_evidence.clear();
            let mut shadow = reducer.fork_desktop()?;
            shadow.require_desktop_neutral_after(reducer.at_us())?;
            self.audit_shadow = osg_desktop_output::audit::next_id().unwrap_or(0);
            self.shadow = Some(shadow);
            self.shadow_admission = admission;
            if let Some(proof) = self.shadow_admission { self.output.acknowledge_child(proof); }
            self.flush(reducer.at_us().0)?;
            // Effects captured before the new scope baseline are never replayed.
            return Ok(vec![]);
        }
        let mut shadow = self.shadow.take().ok_or("The isolated desktop reducer is stopped.")?;
        let result = (|| {
            shadow.mirror_source_liveness(reducer)?;
            let mut diagnostics = shadow.accept_admissions(admissions.iter().cloned(), reducer.at_us())?;
            shadow.mirror_native_state(reducer)?;
            shadow.mirror_source_liveness(reducer)?;
            shadow.advance(reducer.at_us())?;
            let effects = shadow.drain();
            diagnostics.extend(self.deliver(&effects, &shadow)?);
            Ok(diagnostics)
        })();
        self.shadow = Some(shadow);
        result
    }
    fn deliver(&mut self, effects: &[Effect], reducer: &Phase) -> Result<Vec<String>, String> {
        // Revocation must precede clock advances even when a fallback or a
        // neutral output suppressed an otherwise unchanged desktop effect.
        let tokens = reducer
            .program()
            .sinks
            .iter()
            .filter(|sink| matches!(sink.kind, SinkKind::Desktop { .. }))
            .map(|sink| Ok((sink.id.clone(), reducer.endpoint_token(&sink.from)?)))
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        self.output.reconcile(&tokens);
        for (id, producer) in &tokens {
            self.scheduler.reconcile_producer(id, producer.clone())?;
            if self.focus_boundary.is_some()
                && self
                    .rearm_evidence
                    .get(id)
                    .is_some_and(|(token, _)| token != producer)
            {
                self.waiting_neutral.insert(id.clone());
                self.rearm_evidence.remove(id);
            }
        }
        for effect in effects {
            if effect
                .producer
                .as_ref()
                .is_some_and(|producer| tokens.get(&effect.sink_id) != Some(producer))
            {
                continue;
            }
            if tokens.contains_key(&effect.sink_id)
                && matches!(effect.sample, osg_mapping::program::SignalSample::Known(_))
            {
                if let Some(boundary) = self.focus_boundary {
                    if self.waiting_neutral.contains(&effect.sink_id) {
                        if !neutral(&effect.sample)
                            || !fresh_observations(&effect.observations, boundary)
                            || !reducer.effect_has_current_native_level(effect)
                        {
                            continue;
                        }
                        let producer = effect.producer.as_ref().unwrap().clone();
                        self.scheduler.accept(
                            &effect.sink_id,
                            producer.clone(),
                            effect.sample.clone(),
                            effect.at_us,
                            true,
                        )?;
                        self.waiting_neutral.remove(&effect.sink_id);
                        self.rearm_evidence.insert(
                            effect.sink_id.clone(),
                            (producer, effect.observations.clone()),
                        );
                        continue;
                    }
                    if self
                        .rearm_evidence
                        .get(&effect.sink_id)
                        .is_none_or(|(_, baseline)| !after_neutral(&effect.observations, baseline))
                        || !reducer.effect_has_current_native_level(effect)
                    {
                        continue;
                    }
                }
            }
            // Only direct KeyHold changes have an immediate one-effect delivery
            // boundary here. Timer, pointer, tap and cleanup work stays outside
            // this latency population rather than borrowing a later root.
            let audit_effect = if matches!(effect.reason, osg_mapping::engine::ReportReason::Change)
                && matches!(self.scheduler.action(&effect.sink_id), Some(osg_mapping::desktop::Action::KeyHold { .. }))
            { self.audit_effect(effect) } else { None };
            let previous = audit_effect.map(|_| self.scheduler.pending_deliveries().map(|delivery| delivery.operation_id.0).max().unwrap_or(0));
            self.scheduler.accept_effect(effect)?;
            if let (Some(id), Some(previous)) = (audit_effect, previous) {
                use osg_desktop_output::audit::{self, AliasKind, Kind};
                let run = audit::alias(AliasKind::Run, effect.run.session.as_bytes());
                let action = audit::alias(AliasKind::Action, effect.sink_id.as_bytes());
                if let (Some(run), Some(action)) = (run, action) {
                    for delivery in self.scheduler.pending_deliveries().filter(|delivery|
                        delivery.operation_id.0 > previous && delivery.action_id == effect.sink_id
                        && effect.producer.as_ref() == Some(&delivery.producer)
                        && delivery.at_us == effect.at_us
                        && matches!(delivery.intent, osg_mapping::desktop::Intent::Held { .. }))
                    {
                        audit::record(Kind::DeliveryBound, &[id, self.audit_shadow, run, effect.run.generation.0, delivery.operation_id.0, action]);
                    }
                }
            }
        }
        if let Some(boundary) = self.focus_boundary {
            for sink in &reducer.program().sinks {
                if !self.waiting_neutral.contains(&sink.id) {
                    continue;
                }
                let Some(sample) = reducer.sample(&sink.from).filter(|sample| neutral(sample))
                else {
                    continue;
                };
                let producer = tokens[&sink.id].clone();
                let observations = reducer.source_observations(&producer);
                if fresh_observations(&observations, boundary)
                    && reducer.endpoint_has_current_native_level(&sink.from)
                {
                    self.scheduler.accept(
                        &sink.id,
                        producer.clone(),
                        sample.clone(),
                        reducer.at_us(),
                        true,
                    )?;
                    self.waiting_neutral.remove(&sink.id);
                    self.rearm_evidence
                        .insert(sink.id.clone(), (producer, observations));
                }
            }
        }
        self.prepare_neutral_arms(reducer, &tokens);
        self.scheduler.advance(reducer.at_us())?;
        self.flush(reducer.at_us().0)?;
        if let Some(error) = self.scheduler.fault() {
            return Err(error.into());
        }
        Ok(self.diagnostics.drain(..).collect())
    }
    pub fn keep_alive(&mut self) -> Result<(), String> {
        if self.registered {
            self.output.renew(self.run.clone())
        } else {
            Ok(())
        }
    }
    pub fn stop(&mut self) -> Result<(), String> {
        self.retire_scope_watch();
        self.authorization_deadline = None;
        self.scheduler.stop();
        self.neutral_arms.clear();
        for delivery in self.scheduler.drain() { self.audit_drop(&delivery, 3); }
        self.shadow = None;
        self.shadow_admission = None;
        self.started = false;
        let result = self.output.stop_all();
        let completion = self.collect_completion();
        let result = result.and(completion);
        self.last_error = result.as_ref().err().cloned();
        result
    }
    fn readiness_with_fault(&self, faulted: bool) -> Readiness {
        if faulted || self.last_error.is_some() {
            Readiness::Faulted
        } else if !self.started {
            Readiness::Stopped
        } else if self.focused && !self.waiting_neutral.is_empty() {
            Readiness::Waiting
        } else if self.focused {
            Readiness::Active
        } else {
            Readiness::Paused
        }
    }
    pub fn readiness(&self) -> Readiness {
        self.readiness_with_fault(self.output.faulted())
    }
    pub fn status(&self) -> Value {
        let backend = self.output.snapshot();
        let state = self.readiness_with_fault(backend.uncertain || !backend.cleanup_complete || backend.error.is_some());
        let error = self.last_error.clone().or_else(|| backend.error.clone());
        json!({"state":state,
            "scope":self.scope,"backend":backend,"scheduler":self.scheduler.decisions(),"error":error,
            "dispatchPending":self.output.pending(),
            "outcomes":self.outcomes,"droppedOutcomes":self.dropped_outcomes,
            "scopeError":self.scope_error,"waitingNeutral":self.waiting_neutral,"focusBoundaryUs":self.focus_boundary,
            "scopeNotifications":self.scope_notifications(),
            "helperScope":if self.started { self.output.child_scope() } else { None },
            "nativeSubmission":"SendInput count is recorded separately from application observations."})
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Readiness {
    #[default]
    Stopped,
    Waiting,
    Active,
    Paused,
    Faulted,
    RecoveryNeeded,
}
fn neutral(sample: &osg_mapping::program::SignalSample) -> bool {
    matches!(
        sample,
        osg_mapping::program::SignalSample::Known(osg_mapping::program::SignalValue::Scalar(
            osg_mapping::signal::PortValue::Boolean(false)
        ))
    ) || matches!(sample,osg_mapping::program::SignalSample::Known(osg_mapping::program::SignalValue::Scalar(osg_mapping::signal::PortValue::Number(value)))if *value==0.0)
}
fn fresh_observations(observations: &[Observation], boundary: Counter) -> bool {
    !observations.is_empty()
        && observations
            .iter()
            .all(|observation| observation.at_us > boundary && observation.sequence.0 > 0)
}
fn after_neutral(observations: &[Observation], baseline: &[Observation]) -> bool {
    !observations.is_empty()
        && observations.len() == baseline.len()
        && baseline.iter().all(|prior| {
            observations.iter().any(|current| {
                current.endpoint == prior.endpoint
                    && current.at_us >= prior.at_us
                    && current.sequence >= prior.sequence
            })
        })
}
#[path = "desktop/dispatch.rs"]
mod dispatch;
#[path = "desktop/deadlines.rs"]
mod deadlines;
#[cfg(test)]
#[path = "desktop/dispatch_integration_tests.rs"]
mod dispatch_integration_tests;
#[cfg(test)]
#[path = "desktop_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "desktop/focus_health_tests.rs"]
mod focus_health_tests;

#[path = "desktop/output.rs"]
mod output;
#[path = "desktop/lifecycle.rs"]
mod lifecycle;
#[path = "desktop/focus.rs"]
mod focus;
#[path = "desktop/helper_scope.rs"]
mod helper_scope;
