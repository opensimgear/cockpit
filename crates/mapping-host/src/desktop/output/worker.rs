use super::{Completion, Job, Reply, Setup, Shared, execute};
use osg_desktop_output::Coordinator;
use osg_mapping::engine::{Counter, RunToken, typed::Token};
use std::{collections::BTreeMap, sync::Arc, time::{Duration, Instant}};

const RENEW_INTERVAL: Duration = Duration::from_millis(100);
const AUTHORIZATION_FRESHNESS: Duration = Duration::from_millis(5);
pub(super) struct Owner {
    output: Coordinator,
    shared: Arc<Shared>,
    armed: BTreeMap<String, Token>,
    tap_durations: BTreeMap<String, Counter>,
    pub(super) last_renew: Option<Instant>,
    last_authorization: u64,
    failure_stopped: bool,
}
enum Work {
    Stop(Reply), Pause(RunToken), Setup(Setup), Data(Job), Renew(RunToken, u64), Fault,
}
impl Owner {
    pub(super) fn new(output: Coordinator, shared: Arc<Shared>, tap_durations: BTreeMap<String, Counter>) -> Self {
        Self { output, shared, armed: BTreeMap::new(), tap_durations, last_renew: None, last_authorization: 0, failure_stopped: false }
    }
    pub(super) fn fail(&mut self, error: String) {
        let mut state = self.shared.lock();
        if state.fault.is_none() { state.fault = Some(error.clone()); }
        state.epoch = state.epoch.wrapping_add(1);
        state.authorization = None;
        // A panic may have interrupted execute before it returned a completion.
        if state.busy && state.completion.is_none() {
            state.job = None;
            state.completion = Some(Completion { audit_shadow: state.busy_shadow, outcomes: vec![], dropped: vec![], error: Some(error) });
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
        if let Some(job) = state.job.take() {
            state.executing_epoch = job.epoch;
            return Some(Work::Data(job));
        }
        if let Some((run, admitted, id)) = state.authorization.clone() {
            if admitted.elapsed() > AUTHORIZATION_FRESHNESS || id == self.last_authorization {
                state.authorization = None;
            } else if self.last_renew.is_none_or(|last| last.elapsed() >= RENEW_INTERVAL) {
                state.authorization = None;
                state.control_active = true;
                return Some(Work::Renew(run, id));
            }
        }
        None
    }
    pub(super) fn step(&mut self) -> bool {
        let Some(work) = self.next() else { return false };
        match work {
            Work::Data(job) => {
                let mut completion = execute::execute(&mut self.output, &self.shared, &mut self.armed, &self.tap_durations, job);
                // A concurrent admission failure also terminates this ticket.
                if completion.error.is_none() { completion.error = self.shared.lock().fault.clone(); }
                let error = completion.error.clone();
                self.shared.lock().completion = Some(completion);
                if let Some(error) = error { self.fail(error); }
            }
            Work::Stop(reply) => {
                // Complete an accepted but unstarted data ticket without replay.
                let mut state = self.shared.lock();
                if let Some(job) = state.job.take() {
                    state.completion = Some(Completion { audit_shadow: job.audit_shadow, outcomes: vec![], dropped: job.deliveries.into_iter().map(|d| (d, 4)).collect(), error: state.fault.clone() });
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
                let result = self.output.pause(run);
                if result.is_ok() { self.armed.clear(); }
                if let Err(error) = result { self.fail(error); }
            }
            Work::Setup(setup) => {
                let (result, reply) = match setup {
                    Setup::Bind(run, scope, reply) => (self.output.bind_scope_before_activate(&run, &scope), reply),
                    Setup::Activate(run, scope, reply) => {
                        let result = self.output.activate(run, scope);
                        if result.is_ok() { self.last_renew = Some(Instant::now()); }
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
                let result = self.output.renew(run);
                self.last_renew = Some(Instant::now());
                if let Err(error) = result { self.fail(error); }
            }
            Work::Fault => {
                // No replay. Try only the explicit release barrier; retain the
                // original fault and the coordinator's durable uncertainty.
                let _ = self.output.stop_all();
                self.armed.clear();
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
            if state.stop.is_some() || state.pause.is_some() || state.setup.is_some()
                || state.job.is_some() || (state.fault.is_some() && !self.failure_stopped) { continue; }
            let deadline = state.authorization.as_ref().and_then(|(_, admitted, id)| {
                if *id == self.last_authorization || admitted.elapsed() > AUTHORIZATION_FRESHNESS { return None; }
                Some(self.last_renew.map_or(Duration::ZERO, |last| RENEW_INTERVAL.saturating_sub(last.elapsed())))
            });
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
