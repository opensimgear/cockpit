//! Desktop scheduler/coordinator bridge shared by native profile sessions.
//! This layer owns no profile storage, frontend events or application selector.
use osg_desktop_output::{Coordinator, Scope};
use osg_mapping::{
    desktop::Scheduler,
    engine::{
        typed::{Effect, Frame, Observation, Phase, Token},
        Counter, RunToken,
    },
    program::{Program, SinkKind},
};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
pub const OUTCOME_LIMIT: usize = 256;
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub delivery: osg_mapping::desktop::Delivery,
    pub acknowledgement: osg_desktop_output::protocol::Ack,
}
fn eligible(scope: &Scope) -> Result<bool, String> {
    #[cfg(windows)]
    {
        osg_desktop_output::windows::eligible_scope(scope)
    }
    #[cfg(not(windows))]
    {
        let _ = scope;
        Ok(false)
    }
}
pub struct Desktop {
    output: Coordinator,
    scheduler: Scheduler,
    run: RunToken,
    scope: Option<Scope>,
    armed: BTreeMap<String, Token>,
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
    shadow: Option<Phase>,
    audit_shadow: u64,
}
impl Desktop {
    pub fn acquire(
        run: RunToken,
        program: &Program,
        enabled: bool,
        qualified: bool,
        journal: std::path::PathBuf,
        scope: Option<Scope>,
    ) -> Result<Self, String> {
        let scheduler = Scheduler::from_program(run.clone(), program)?;
        let output = Coordinator::spawn_with_owned_journal(
            enabled,
            qualified,
            journal,
            run.clone(),
            scope.clone(),
        )?;
        Ok(Self {
            output,
            scheduler,
            run,
            scope,
            armed: BTreeMap::new(),
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
            shadow: None,
            audit_shadow: 0,
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
        use osg_desktop_output::audit::{self, AliasKind, Kind};
        if !audit::enabled() { return; }
        if let Some(run) = audit::alias(AliasKind::Run, delivery.producer.run.session.as_bytes()) {
            audit::record(Kind::DeliveryDropped, &[self.audit_shadow, run, delivery.producer.run.generation.0, delivery.operation_id.0, reason]);
        }
    }
    pub fn bind_scope_before_start(&mut self, scope: Scope) -> Result<(), String> {
        if self.started || self.registered {
            return Err(
                "A running desktop scope is immutable; apply a fresh profile revision.".into(),
            );
        }
        self.output.bind_scope_before_activate(&self.run,&scope)?;
        self.scope = Some(scope);
        Ok(())
    }
    fn flush(&mut self, now: u64) -> Result<(), String> {
        let mut cancelled = BTreeSet::new();
        for delivery in self.scheduler.drain() {
            let cleanup = matches!(&delivery.intent, osg_mapping::desktop::Intent::Cleanup)
                || matches!(&delivery.intent, osg_mapping::desktop::Intent::Held { controls } if controls.is_empty());
            if cleanup && self.armed.get(&delivery.action_id) != Some(&delivery.producer) {
                // A neutral baseline can precede this generation's first Arm.
                // Only a token which acquired an owner may release that owner.
                self.audit_drop(&delivery, 1);
                continue;
            }
            if !cleanup && cancelled.contains(&delivery.action_id) {
                self.audit_drop(&delivery, 2);
                continue;
            }
            if !cleanup && self.armed.get(&delivery.action_id) != Some(&delivery.producer) {
                self.output
                    .arm(delivery.producer.clone(), delivery.action_id.clone())?;
                self.armed
                    .insert(delivery.action_id.clone(), delivery.producer.clone());
            }
            let acknowledgement = self.output.submit(delivery.clone(), now)?;
            if acknowledgement.collision || acknowledgement.skipped {
                if self.diagnostics.len() == 32 {
                    self.diagnostics.pop_front();
                }
                self.diagnostics.push_back(if acknowledgement.collision {
                    format!("Desktop action {} shares a held native control; its pulse cannot manufacture a release edge.",delivery.action_id)
                }else{format!("Desktop action {} expired before submission and was skipped.",delivery.action_id)});
            }
            if acknowledgement.skipped
                && matches!(&delivery.intent,osg_mapping::desktop::Intent::Held{controls}if !controls.is_empty())
            {
                self.scheduler.cancel_action(
                    &delivery.action_id,
                    "Submission deadline expired; queued taps cancelled.",
                )?;
                cancelled.insert(delivery.action_id.clone());
            }
            if self.outcomes.len() == OUTCOME_LIMIT {
                self.outcomes.pop_front();
                self.dropped_outcomes = self.dropped_outcomes.saturating_add(1);
            }
            self.outcomes.push_back(Outcome {
                delivery,
                acknowledgement,
            });
        }
        if !cancelled.is_empty() {
            self.flush(now)?;
        }
        Ok(())
    }
    fn baseline(&mut self, reducer: &Phase) -> Result<(), String> {
        let definitions = reducer
            .program()
            .sinks
            .iter()
            .filter(|sink| matches!(sink.kind, SinkKind::Desktop { .. }))
            .cloned()
            .collect::<Vec<_>>();
        for sink in definitions {
            let Some(sample) = reducer.sample(&sink.from).cloned() else {
                continue;
            };
            let producer = reducer.endpoint_token(&sink.from)?;
            self.scheduler
                .accept(&sink.id, producer, sample, reducer.at_us(), true)?;
        }
        Ok(())
    }
    pub fn start(&mut self, reducer: &Phase, scope: Option<Scope>) -> Result<(), String> {
        if self.scope.is_none() {
            self.scope = Some(
                scope.ok_or("Choose an exact application scope before starting desktop output.")?,
            );
        }
        let focused = eligible(self.scope.as_ref().unwrap())?;
        self.start_focused(reducer, focused)
    }
    fn start_focused(&mut self, reducer: &Phase, focused: bool) -> Result<(), String> {
        let shadow = reducer.fork_desktop()?;
        self.output
            .activate(self.run.clone(), self.scope.clone().unwrap())?;
        self.registered = true;
        self.started = true;
        self.focused = focused;
        self.baseline(&shadow)?;
        self.audit_shadow = osg_desktop_output::audit::next_id().unwrap_or(0);
        self.shadow = Some(shadow);
        self.flush(reducer.at_us().0)?;
        Ok(())
    }
    pub fn poll(&mut self, frames: &[Frame], reducer: &Phase) -> Result<Vec<String>, String> {
        let eligibility = self.scope.as_ref().map_or(Ok(false), eligible);
        self.poll_checked(frames, reducer, eligibility)
    }
    fn poll_checked(
        &mut self,
        frames: &[Frame],
        reducer: &Phase,
        eligibility: Result<bool, String>,
    ) -> Result<Vec<String>, String> {
        match eligibility {
            Ok(focused) => {
                self.scope_error = None;
                self.poll_focused(frames, reducer, focused)
            }
            Err(error) => {
                let mut diagnostics = self.poll_focused(frames, reducer, false)?;
                self.scope_error = Some(error.clone());
                diagnostics.push(format!("Keyboard and mouse output paused: {error}"));
                Ok(diagnostics)
            }
        }
    }
    fn poll_focused(
        &mut self,
        frames: &[Frame],
        reducer: &Phase,
        focused: bool,
    ) -> Result<Vec<String>, String> {
        if !self.started {
            return Ok(vec![]);
        }
        self.output.renew(self.run.clone())?;
        if !focused {
            if self.focused {
                self.scheduler
                    .pause("Application focus lost; pending actions and motion cancelled.");
                self.flush(reducer.at_us().0)?;
                self.output.pause(self.run.clone())?;
                self.armed.clear();
            }
            self.shadow = None;
            self.focused = false;
            return Ok(vec!["Keyboard and mouse output is waiting for its application scope; returning requires neutral controls.".into()]);
        }
        if !self.focused {
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
            self.flush(reducer.at_us().0)?;
            // Effects captured before the new scope baseline are never replayed.
            return Ok(vec![]);
        }
        let mut shadow = self.shadow.take().ok_or("The isolated desktop reducer is stopped.")?;
        let result = (|| {
            shadow.mirror_source_liveness(reducer)?;
            let mut diagnostics = shadow.accept_batch(frames.iter().cloned())?;
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
                if fresh_observations(&observations, boundary) {
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
        self.scheduler.stop();
        for delivery in self.scheduler.drain() { self.audit_drop(&delivery, 3); }
        self.armed.clear();
        self.shadow = None;
        self.started = false;
        let result = self.output.stop_all();
        self.last_error = result.as_ref().err().cloned();
        result
    }
    pub fn status(&self) -> Value {
        let backend = self.output.snapshot();
        let faulted = backend.uncertain
            || !backend.cleanup_complete
            || backend.error.is_some()
            || self.last_error.is_some();
        let state = if faulted {
            "faulted"
        } else if !self.started {
            "stopped"
        } else if self.focused && !self.waiting_neutral.is_empty() {
            "waiting"
        } else if self.focused {
            "active"
        } else {
            "paused"
        };
        let error = self.last_error.clone().or_else(|| backend.error.clone());
        json!({"state":state,
            "scope":self.scope,"backend":backend,"scheduler":self.scheduler.decisions(),"error":error,
            "outcomes":self.outcomes,"droppedOutcomes":self.dropped_outcomes,
            "scopeError":self.scope_error,"waitingNeutral":self.waiting_neutral,"focusBoundaryUs":self.focus_boundary,
            "nativeSubmission":"SendInput count is recorded separately from application observations."})
    }
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
impl Drop for Desktop {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
#[path = "desktop_tests.rs"]
mod tests;
