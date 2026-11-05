use super::{Desktop, Outcome, OUTCOME_LIMIT};
use osg_mapping::desktop::{Delivery, Intent};
use std::collections::BTreeSet;

impl Desktop {
    pub(super) fn flush_dispatch(&mut self, now: u64) -> Result<(), String> {
        loop {
            // Applying a completion can cancel queued taps. Never admit the next
            // ordered batch until those ACK-dependent decisions have run.
            self.collect_completion()?;
            if self.output.busy() { return Ok(()); }
            let pending = self.scheduler.drain();
            if pending.is_empty() { return Ok(()); }
            self.output.dispatch(pending, now, self.audit_shadow)?;
            // Recording completes inline; the native owner completes by wake.
            if self.output.asynchronous() { return Ok(()); }
        }
    }

    pub(super) fn collect_completion(&mut self) -> Result<(), String> {
        let Some(completion) = self.output.take_completion() else { return Ok(()); };
        for (delivery, reason) in completion.dropped { self.audit_drop_for(&delivery, reason, completion.audit_shadow); }
        let mut cancelled = BTreeSet::new();
        for (delivery, acknowledgement) in completion.outcomes {
            self.record_outcome(delivery, acknowledgement, &mut cancelled)?;
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
        cancelled: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        if acknowledgement.collision || acknowledgement.skipped {
            if self.diagnostics.len() == 32 {
                self.diagnostics.pop_front();
            }
            self.diagnostics.push_back(if acknowledgement.collision {
                format!("Desktop action {} shares a held native control; its pulse cannot manufacture a release edge.",delivery.action_id)
            } else {
                format!("Desktop action {} expired before submission and was skipped.",delivery.action_id)
            });
        }
        if acknowledgement.skipped
            && self.scheduler.active()
            && self.output.current(&delivery)
            && matches!(&delivery.intent, Intent::Held { controls } if !controls.is_empty())
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
