//! Physical action phase composition. Polling/draining stays cached; serial
//! operations and renewal remain on the existing bounded endpoint actors.
use super::*;
use crate::profile_session_actions::{ActionPoll, PanelActions};
use std::time::Duration;
const KIND: &str = "physical-action-lease";
pub(super) fn has_receipts(receipts: &[Receipt]) -> bool {
    receipts.iter().any(|r| r.kind == KIND)
}
trait Actor {
    fn poll(&self) -> ActionPoll;
    fn bind(&mut self, plan: &ExecutionPlan) -> Result<bool, String>;
    fn stop(&mut self) -> Result<ActionPoll, String>;
}
impl Actor for PanelActions {
    fn poll(&self) -> ActionPoll {
        self.poll()
    }
    fn bind(&mut self, p: &ExecutionPlan) -> Result<bool, String> {
        self.bind_delivery(p)
    }
    fn stop(&mut self) -> Result<ActionPoll, String> {
        self.stop()
    }
}
#[derive(Clone, Copy)]
enum Goal {
    Acquired,
    Bound,
    Restored,
}
fn wait(
    actor: &mut impl Actor,
    goal: Goal,
    plan: Option<&ExecutionPlan>,
    timeout: Duration,
    mut guard: impl FnMut() -> Result<(), String>,
    mut keep_alive: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        guard()?;
        let status = match goal {
            Goal::Restored => actor.stop()?,
            Goal::Bound => {
                actor.bind(plan.ok_or("Missing exact native physical delivery plan.")?)?;
                actor.poll()
            }
            Goal::Acquired => actor.poll(),
        };
        let complete = match goal {
            Goal::Acquired => status.acquired,
            Goal::Bound => status.bound,
            Goal::Restored => status.stopped && status.cleanup_complete,
        };
        if !matches!(goal, Goal::Restored) && status.required_failed {
            return Err(if status.diagnostics.is_empty() {
                "A required physical endpoint lost native ownership before activation.".into()
            } else {
                status.diagnostics.join("; ")
            });
        }
        if complete {
            return Ok(());
        }
        if matches!(goal, Goal::Restored) && status.stopped && !status.cleanup_complete {
            return Err(if status.diagnostics.is_empty() {
                "Physical output cleanup is unresolved; retain its sealed recovery receipt.".into()
            } else {
                status.diagnostics.join("; ")
            });
        }
        if Instant::now() >= deadline {
            return Err(if matches!(goal,Goal::Restored){"Physical output cleanup remains pending; its sealed recovery receipt was retained."}else{"Physical output did not finish current native acquisition and binding before its deadline."}.into());
        }
        keep_alive()?;
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn exact_receipts(run: &OwnedRun, actions: &PanelActions) -> Result<(), String> {
    let mut actual = run
        .receipts
        .iter()
        .filter(|r| r.kind == KIND)
        .cloned()
        .collect::<Vec<_>>();
    let mut expected = actions.capture_receipts();
    actual.sort_by(|a, b| a.resource_id.cmp(&b.resource_id));
    expected.sort_by(|a, b| a.resource_id.cmp(&b.resource_id));
    if serde_json::to_value(actual).map_err(|e| e.to_string())?
        != serde_json::to_value(expected).map_err(|e| e.to_string())?
    {
        return Err(
            "Physical cleanup receipts changed from their exact captured native owner.".into(),
        );
    }
    Ok(())
}
/// A hold has no reducer/effect authority. Bound actors renew independently.
pub(super) fn held(state: &NativeRun) -> Result<Vec<String>, String> {
    if state.actions.is_empty() {
        return Ok(vec![]);
    }
    let poll = state.actions.poll();
    if poll.required_failed || !poll.acquired || !poll.bound {
        return Err(
            "The neutral physical output leases are no longer ready for the pending launch.".into(),
        );
    }
    Ok(poll.diagnostics)
}
pub(super) fn start(state: &mut NativeRun) -> Result<(), String> {
    if state.actions.is_empty() {
        Ok(())
    } else {
        state.actions.start()
    }
}
pub(super) fn dispatch(
    state: &mut NativeRun,
    effects: &[osg_mapping::engine::typed::Effect],
) -> Vec<String> {
    if state.actions.is_empty() {
        return vec![];
    }
    let mut diagnostics = vec![];
    if let Err(error) = state.actions.dispatch(effects) {
        diagnostics.push(error);
    }
    let status = state.actions.poll();
    diagnostics.extend(status.diagnostics);
    if status.required_failed && diagnostics.is_empty() {
        diagnostics.push("A physical output branch was revoked and restored; a fresh Apply is required to reclaim it. Independent branches remain active.".into());
    }
    diagnostics
}
impl NativeProfileHost {
    pub(super) fn acquire_actions(&mut self, owner: &RunOwner) -> Result<(), String> {
        let epoch = self.admission_epoch.clone();
        let state = self.run(owner)?;
        if state.actions.is_empty() {
            return Ok(());
        }
        let expected = state.admission_epoch;
        Self::check_epoch(&epoch, expected)?;
        state.actions.acquire()?;
        wait(
            &mut state.actions,
            Goal::Acquired,
            None,
            Duration::from_secs(3),
            || Self::check_epoch(&epoch, expected),
            || state.output.as_mut().map_or(Ok(()), |o| o.keep_alive()),
        )?;
        // Hardware invocation tokens are real cached Registry tokens, even when
        // input sources/game commands still await their separate baselines.
        let delivery = self.action_delivery(owner)?;
        self.bind_actions(owner, &delivery)
    }
    pub(super) fn refresh_action_binding(&mut self, owner: &RunOwner) -> Result<(), String> {
        if self.run(owner)?.actions.is_empty() {
            return Ok(());
        }
        let plan = self.action_delivery(owner)?;
        self.bind_actions(owner, &plan)
    }
    fn action_delivery(&mut self, owner: &RunOwner) -> Result<ExecutionPlan, String> {
        let resources = self.resources.clone();
        let state = self.run(owner)?;
        let mut snapshot = resources.snapshot_for_compile(state.assignments.clone())?;
        snapshot.backend = state.backend.clone();
        Self::game_capture(&mut snapshot, state.game_preparation.as_ref());
        let report = osg_profiles::compiler::compile(&state.document, &snapshot);
        if report.program_hash != state.report.program_hash {
            return Err("Physical action acquisition changed the immutable native program.".into());
        }
        report
            .plan
            .ok_or("The compiler did not produce an exact native physical output plan.".into())
    }
    pub(super) fn bind_actions(
        &mut self,
        owner: &RunOwner,
        plan: &ExecutionPlan,
    ) -> Result<(), String> {
        let resources = self.resources.clone();
        let epoch = self.admission_epoch.clone();
        let state = self.run(owner)?;
        if state.actions.is_empty() {
            return Ok(());
        }
        // Readiness of other sources/commands is deliberately not promoted.
        // Only actual ready physical output tokens can bind neutral leases.
        for sink in &plan.sinks {
            if sink.adapter != SinkAdapter::PhysicalAction
                || !state.actions.owns_endpoint(&sink.endpoint)
                || plan
                    .branches
                    .iter()
                    .any(|b| b.sink == sink.endpoint && b.readiness == PlanReadiness::Dormant)
            {
                continue;
            }
            let proof = (|| {
                if sink.readiness != PlanReadiness::Ready {
                    return Err(
                        "The physical output is no longer ready for native binding.".to_owned()
                    );
                }
                let token = sink
                    .token
                    .as_ref()
                    .filter(|t| t.access == osg_profiles::resources::Access::Invoke)
                    .ok_or("The physical output has no actual native Invoke token.")?;
                if Some(resources.validate_delivery(token)?) != sink.route {
                    return Err(
                        "The physical output token no longer matches its selected route.".into(),
                    );
                }
                Ok(())
            })();
            if let Err(error) = proof {
                let branch = plan
                    .branches
                    .iter()
                    .find(|branch| branch.sink == sink.endpoint)
                    .ok_or("The physical output lost its exact compiled branch.")?;
                if branch.required {
                    return Err(error);
                }
                state.actions.revoke_optional(&sink.endpoint, error)?;
            }
        }
        state.actions.acquire_ready(plan)?;
        let expected = state.admission_epoch;
        wait(
            &mut state.actions,
            Goal::Acquired,
            None,
            Duration::from_secs(3),
            || Self::check_epoch(&epoch, expected),
            || state.output.as_mut().map_or(Ok(()), |o| o.keep_alive()),
        )?;
        wait(
            &mut state.actions,
            Goal::Bound,
            Some(plan),
            Duration::from_secs(3),
            || Self::check_epoch(&epoch, expected),
            || state.output.as_mut().map_or(Ok(()), |o| o.keep_alive()),
        )
    }
    pub(super) fn restore_actions(&mut self, run: &OwnedRun, recovery: bool) -> Result<(), String> {
        let owner_key = key(&run.owner);
        if let Some(state) = self.prepared.get_mut(&owner_key) {
            exact_receipts(run, &state.actions)?;
            if recovery {
                let status = state.actions.poll();
                if status.stopped && !status.cleanup_complete {
                    state.actions =
                        PanelActions::recover(run, self.resources.clone(), self.panels.clone())?;
                }
            }
            return wait(
                &mut state.actions,
                Goal::Restored,
                None,
                Duration::from_secs(3),
                || Ok(()),
                || Ok(()),
            );
        }
        if !has_receipts(&run.receipts) {
            return Ok(());
        }
        if recovery
            && self.recovery_actions.get(&owner_key).is_some_and(|a| {
                let p = a.poll();
                p.stopped && !p.cleanup_complete
            })
        {
            self.recovery_actions.remove(&owner_key);
        }
        if !self.recovery_actions.contains_key(&owner_key) {
            let captured = PanelActions::recover(run, self.resources.clone(), self.panels.clone())?;
            if !run.attempted.contains(&ApplyPhase::Acquire) {
                return Ok(());
            }
            self.recovery_actions.insert(owner_key.clone(), captured);
        }
        wait(
            self.recovery_actions.get_mut(&owner_key).unwrap(),
            Goal::Restored,
            None,
            Duration::from_secs(3),
            || Ok(()),
            || Ok(()),
        )?;
        self.recovery_actions.remove(&owner_key);
        Ok(())
    }
    pub(super) fn discard_actions(&mut self, owner: &RunOwner, mut actions: PanelActions) {
        let _ = actions.stop();
        if !actions.poll().cleanup_complete {
            self.recovery_actions.insert(key(owner), actions);
        }
    }
}
#[cfg(test)]
#[path = "profile_session_action_host_tests.rs"]
mod tests;
