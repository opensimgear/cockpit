use super::{neutral, Desktop, Outcome, Phase, Scope, Token, OUTCOME_LIMIT};
use osg_mapping::desktop::{Delivery, Intent};
use std::collections::BTreeMap;

fn compact_owner_set(tokens: &BTreeMap<String, Token>, scope: Option<&Scope>) -> bool {
    // Empty native owners still occupy journal space. Leave room for the
    // exact in-flight request, scopes and actual holds of the entire profile.
    if tokens.len() > 32 || matches!(scope, Some(Scope::Application { identity }) if identity.executable.len() > 1024) {
        return false;
    }
    let mut identifiers = 0;
    let mut dependencies = 0;
    for (action, token) in tokens {
        identifiers += action.len() + token.node_id.len() + token.run.session.len();
        dependencies += token.dependencies.len();
        for dependency in &token.dependencies {
            identifiers += dependency.endpoint.node_id.len() + dependency.endpoint.port_id.len();
        }
    }
    identifiers <= 4096 && dependencies <= 128
}

impl Desktop {
    pub(super) fn prepare_neutral_arms(&mut self, reducer: &Phase, tokens: &BTreeMap<String, Token>) {
        self.neutral_arms.retain(|id, (producer, _)| tokens.get(id) == Some(producer));
        if self.neutral_arms.len() == tokens.len() || !compact_owner_set(tokens, self.scope.as_ref()) {
            return;
        }
        for sink in &reducer.program().sinks {
            let Some(producer) = tokens.get(&sink.id) else { continue };
            if self.neutral_arms.contains_key(&sink.id) || self.waiting_neutral.contains(&sink.id) {
                continue;
            }
            if reducer.sample(&sink.from).is_some_and(neutral)
                && reducer.endpoint_has_current_native_level(&sink.from)
            {
                self.neutral_arms.insert(sink.id.clone(), (producer.clone(), false));
            }
        }
    }

    pub(super) fn flush_dispatch(&mut self, now: u64) -> Result<(), String> {
        loop {
            // Apply ACK-dependent cancellation before dispatching another
            // batch. Only the worker's terminal KeyHold window can take one
            // additional head, and that window retains its own ACK barrier.
            self.collect_completion()?;
            if self.output.busy() {
                return self.output.enqueue_late_press(
                    &mut self.scheduler, now, self.audit_shadow, self.shadow_admission,
                );
            }
            let pending = self.scheduler.drain();
            let prearm = if pending.is_empty() {
                // Yield to the profile worker between neutral preparations so
                // an authored edge can run before the next idle preparation.
                self.neutral_arms.iter_mut().find(|(_, (_, queued))| !queued)
                    .map(|(id, (producer, queued))| {
                        *queued = true;
                        (id.clone(), producer.clone())
                    })
            } else { None };
            if pending.is_empty() && prearm.is_none() { return Ok(()); }
            let key_holds = pending.iter().filter(|delivery| {
                matches!(self.scheduler.action(&delivery.action_id),
                    Some(osg_mapping::desktop::Action::KeyHold { .. }))
            }).map(|delivery| delivery.operation_id).collect();
            self.output.dispatch_prepared(pending, prearm, now, self.audit_shadow, self.shadow_admission, key_holds)?;
            // Recording completes inline; the native owner completes by wake.
            if self.output.asynchronous() { return Ok(()); }
        }
    }

    pub(super) fn collect_completion(&mut self) -> Result<(), String> {
        let Some(mut completion) = self.output.take_completion() else { return Ok(()); };
        for (delivery, reason) in std::mem::take(&mut completion.dropped) {
            self.audit_drop_for(&delivery, reason, completion.audit_shadow);
        }
        for (delivery, acknowledgement) in std::mem::take(&mut completion.outcomes) {
            let cancellation_current = completion.cancellation_current(delivery.operation_id);
            self.record_outcome(delivery, acknowledgement, cancellation_current)?;
        }
        if let Some(error) = completion.error {
            self.last_error = Some(error.clone());
            return Err(error);
        }
        Ok(())
    }

    fn record_outcome(
        &mut self,
        delivery: Delivery,
        acknowledgement: osg_desktop_output::protocol::Ack,
        cancellation_current: bool,
    ) -> Result<(), String> {
        if acknowledgement.collision || acknowledgement.skipped && !acknowledgement.scope_rejected {
            if self.diagnostics.len() == 32 {
                self.diagnostics.pop_front();
            }
            self.diagnostics.push_back(if acknowledgement.collision {
                format!("Desktop action {} shares a held native control; its pulse cannot manufacture a release edge.",delivery.action_id)
            } else {
                format!("Desktop action {} expired before submission and was skipped.",delivery.action_id)
            });
        }
        if acknowledgement.skipped && !acknowledgement.scope_rejected
            && cancellation_current
            && self.scheduler.active()
            && self.output.current(&delivery)
            && matches!(&delivery.intent, Intent::Held { controls } if !controls.is_empty())
        {
            self.scheduler.cancel_action(
                &delivery.action_id,
                "Submission deadline expired; queued taps cancelled.",
            )?;
        }
        if self.outcomes.len() == OUTCOME_LIMIT {
            self.outcomes.pop_front();
            self.dropped_outcomes = self.dropped_outcomes.saturating_add(1);
        }
        self.outcomes.push_back(Outcome {
            delivery,
            acknowledgement,
        });
        Ok(())
    }
}

#[cfg(test)]
use super::output::{same_pointer_batch, effective_source_now};
#[cfg(test)]
use std::time::Instant;
#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
