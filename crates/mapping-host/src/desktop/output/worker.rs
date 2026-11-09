use super::{Completion, Job, Reply, Setup, Shared, State, child, execute, focus};
use osg_desktop_output::{Coordinator, protocol::ScopeAdmission};
use osg_mapping::engine::{Counter, RunToken, typed::Token};
use std::{collections::BTreeMap, sync::Arc, time::{Duration, Instant}};

const RENEW_INTERVAL: Duration = Duration::from_millis(100);
const AUTHORIZATION_FRESHNESS: Duration = Duration::from_millis(5);
pub(super) struct Owner {
    output: Coordinator,
    shared: Arc<Shared>,
    armed: BTreeMap<String, Token>,
    armed_admission: Option<ScopeAdmission>,
    tap_durations: BTreeMap<String, Counter>,
    pub(super) last_renew: Option<Instant>,
    last_authorization: u64,
    failure_stopped: bool,
    renewal_requested: bool,
}
enum Work {
    Stop(Reply), Pause(RunToken), Setup(Setup), Data(Job), Renew(RunToken, u64), Authorize, Fault,
}
fn take_job(state: &mut State) -> Option<Work> {
    let job = state.job.take()?;
    state.executing_epoch = job.epoch;
    state.executing_admission = job.admission;
    Some(Work::Data(job))
}
impl Owner {
    pub(super) fn new(output: Coordinator, shared: Arc<Shared>, tap_durations: BTreeMap<String, Counter>) -> Self {
        Self { output, shared, armed: BTreeMap::new(), armed_admission: None, tap_durations, last_renew: None, last_authorization: 0, failure_stopped: false, renewal_requested: false }
    }
    pub(super) fn fail(&mut self, error: String) {
        let mut state = self.shared.lock();
        if state.fault.is_none() { state.fault = Some(error.clone()); }
        state.epoch = state.epoch.wrapping_add(1);
        state.authorization = None;
        // A panic may have interrupted execute before it returned a completion.
        if state.busy && state.completion.is_none() {
            state.job = None;
            let mut completion = Completion::new(state.busy_shadow);
            completion.error = Some(error);
            state.late_press.retire(&mut completion);
            state.completion = Some(completion);
        }
        state.snapshot.uncertain = true;
        state.snapshot.cleanup_complete = false;
        state.control_active = false;
        drop(state);
        self.shared.notify();
    }
    fn next(&mut self) -> Option<Work> {
        let mut state = self.shared.lock();
        if let Some(reply) = state.stop.take() {
            state.control_active = true;
            return Some(Work::Stop(reply));
        }
        if let Some(run) = state.pause.take() {
            state.control_active = true;
            return Some(Work::Pause(run));
        }
        if let Some(setup) = state.setup.take() {
            state.control_active = true;
            return Some(Work::Setup(setup));
        }
        if state.fault.is_some() && !self.failure_stopped {
            self.failure_stopped = true;
            return Some(Work::Fault);
        }
        if state.job.as_ref().is_some_and(|job| job.prearm.is_none()) {
            return take_job(&mut state);
        }
        if !self.renewal_enabled(&state) { return take_job(&mut state); }
        if let Some((run, admitted, id)) = state.authorization.clone() {
            if admitted.elapsed() > AUTHORIZATION_FRESHNESS || id == self.last_authorization {
                state.authorization = None;
                // Each expired response permits one replacement request. Its
                // removal prevents repeated wakes for a stalled parent.
                if id != self.last_authorization { self.renewal_requested = false; }
            } else if self.last_renew.is_none_or(|last| last.elapsed() >= RENEW_INTERVAL) {
                state.authorization = None;
                state.control_active = true;
                return Some(Work::Renew(run, id));
            }
        }
        if !self.renewal_requested && self.last_renew.is_some_and(|last| last.elapsed() >= RENEW_INTERVAL) {
            self.renewal_requested = true;
            return Some(Work::Authorize);
        }
        // Wait for an outstanding authorization before starting an idle Arm.
        if self.renewal_requested { None } else { take_job(&mut state) }
    }
    fn renewal_enabled(&self, state: &State) -> bool {
        self.last_renew.is_some() && !state.closed && !state.shutdown && state.fault.is_none()
            && focus::renewable(state) && child::renewable(state)
    }
    fn renewal_wait(&self, state: &State) -> Option<Duration> {
        (self.renewal_enabled(state) && !self.renewal_requested)
            .then(|| RENEW_INTERVAL.saturating_sub(self.last_renew.unwrap().elapsed()))
    }
    pub(super) fn step(&mut self) -> bool {
        let Some(work) = self.next() else { return false };
        match work {
            Work::Data(job) => {
                if self.armed_admission != job.admission {
                    self.armed.clear();
                    self.armed_admission = job.admission;
                }
                let mut completion = execute::execute(&mut self.output, &self.shared, &mut self.armed, &self.tap_durations, job);
                let error = {
                    let mut state = self.shared.lock();
                    state.executing_admission = None;
                    state.late_press.retire(&mut completion);
                    // A concurrent admission failure also terminates this ticket.
                    if completion.error.is_none() { completion.error = state.fault.clone(); }
                    let error = completion.error.clone();
                    state.completion = Some(completion);
                    error
                };
                if let Some(error) = error { self.fail(error); }
            }
            Work::Stop(reply) => {
                self.last_renew = None;
                self.renewal_requested = false;
                // Complete an accepted but unstarted data ticket without replay.
                let mut state = self.shared.lock();
                if let Some(job) = state.job.take() {
                    let mut completion = Completion::new(job.audit_shadow);
                    completion.dropped = job.deliveries.into_iter().map(|delivery| (delivery, 4)).collect();
                    completion.error = state.fault.clone();
                    state.completion = Some(completion);
                }
                drop(state);
                let result = self.output.stop_all().and_then(|_| self.output.retire_stopped_journal());
                if result.is_ok() { self.armed.clear(); }
                if let Err(error) = &result { self.fail(error.clone()); }
                self.shared.publish(self.output.snapshot());
                self.shared.lock().control_active = false;
                let _ = reply.send(result);
            }
            Work::Pause(run) => {
                self.armed.clear();
                self.armed_admission = None;
                let result = self.output.pause(run);
                if result.is_ok() { self.armed.clear(); }
                if let Err(error) = result { self.fail(error); }
            }
            Work::Setup(setup) => {
                let (result, reply) = match setup {
                    Setup::Bind(run, scope, reply) => (self.output.bind_scope_before_activate(&run, &scope), reply),
                    Setup::Activate(run, scope, reply) => {
                        let result = self.output.activate(run.clone(), scope);
                        if result.is_ok() {
                            self.last_renew = Some(Instant::now());
                            self.renewal_requested = false;
                            self.shared.lock().activation_scope = self.output.scope_receipt(&run);
                        }
                        (result, reply)
                    }
                };
                if let Err(error) = &result { self.fail(error.clone()); }
                self.shared.publish(self.output.snapshot());
                self.shared.lock().control_active = false;
                let _ = reply.send(result);
            }
            Work::Renew(run, authorization) => {
                self.last_authorization = authorization;
                self.renewal_requested = false;
                let result = self.output.renew(run);
                self.last_renew = Some(Instant::now());
                if let Err(error) = result { self.fail(error); }
            }
            Work::Fault => {
                self.last_renew = None;
                self.renewal_requested = false;
                // No replay. Try only the explicit release barrier; retain the
                // original fault and the coordinator's durable uncertainty.
                let _ = self.output.stop_all();
                self.armed.clear();
            }
            Work::Authorize => {
                // A due lease needs a newly evaluated parent decision. Retain
                // one wake, then wait for its response without renewing alone.
                self.shared.notify();
                return true;
            }
        }
        self.shared.publish(self.output.snapshot());
        self.shared.lock().control_active = false;
        true
    }
    pub(super) fn run(&mut self) {
        loop {
            if self.shared.lock().shutdown { break; }
            if self.step() { continue; }
            let state = self.shared.lock();
            if state.shutdown { break; }
            // Recheck predicates under the same lock used by producers so no
            // control/job notification can be lost between step and sleep.
            let job_ready = state.job.as_ref().is_some_and(|job| job.prearm.is_none()
                || !self.renewal_requested || !self.renewal_enabled(&state));
            if state.stop.is_some() || state.pause.is_some() || state.setup.is_some()
                || job_ready || (state.fault.is_some() && !self.failure_stopped) { continue; }
            // A fresh response can arrive after next() but before this lock.
            // Its notification must not be lost while a request is outstanding.
            if self.renewal_enabled(&state) && state.authorization.is_some()
                && self.last_renew.is_some_and(|last| last.elapsed() >= RENEW_INTERVAL) { continue; }
            let deadline = self.renewal_wait(&state);
            if let Some(duration) = deadline {
                drop(self.shared.changed.wait_timeout(state, duration).unwrap_or_else(|error| error.into_inner()));
            } else {
                drop(self.shared.changed.wait(state).unwrap_or_else(|error| error.into_inner()));
            }
        }
        // Shutdown is also owned here. Drop signals it without waiting for IPC.
        let _ = self.output.stop_all();
    }
}

#[cfg(test)]
#[path = "renewal_tests.rs"]
mod renewal_tests;
