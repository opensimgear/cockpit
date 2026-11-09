//! Retained scope evidence and immutable admission boundaries.
use super::*;

impl<A: Adapter> Worker<A> {
    #[cfg(test)]
    pub(crate) fn test_admission(&self, command: &Command) -> Option<ScopeAdmission> {
        let token = match command {
            Command::Arm { producer, .. } => &producer.run,
            Command::Deliver { delivery } | Command::DeliverGuarded { delivery, .. } => &delivery.producer.run,
            Command::DeliverKeyPair { deliveries } => &deliveries[0].producer.run,
            Command::DeliverPointerBatch { deliveries } => &deliveries.first()?.producer.run,
            _ => return None,
        };
        self.runs.iter().find(|run| run.token == *token).map(|run| ScopeAdmission {
            registration: Counter(run.registration), epoch: Counter(run.epoch),
        })
    }

    fn pause_scope(&mut self, index: usize, advance: bool) -> Result<(), String> {
        let run = &mut self.runs[index];
        if advance {
            run.epoch = run.epoch.checked_add(1).ok_or("Desktop scope epoch exhausted.")?;
        }
        run.paused = true;
        let events = self.reducer.pause(&run.token);
        self.submit(&events)?;
        self.guards.prune(&self.reducer);
        Ok(())
    }

    pub fn tick(&mut self, now: u64) -> Result<(), String> {
        if self.fault.is_some() { return Ok(()); }
        let result: Result<(), String> = (|| {
            // Report due native releases before applying scope-loss projections.
            self.release_due(now)?;
            let mut expired = Vec::new();
            for index in 0..self.runs.len() {
                let run = &self.runs[index];
                if !run.live { continue; }
                if now >= run.lease {
                    self.runs[index].live = false;
                    self.runs[index].source_eligible = false;
                    self.pause_scope(index, true)?;
                    expired.push(index);
                    continue;
                }
                let evidence = self.adapter.scope_evidence(run.registration, &run.scope)?;
                let lost = evidence.loss_epoch != run.loss_epoch
                    || (run.source_eligible && !evidence.eligible);
                let run = &mut self.runs[index];
                run.loss_epoch = evidence.loss_epoch;
                run.source_eligible = evidence.eligible;
                if lost { self.pause_scope(index, true)?; }
            }
            // Release every expired contribution before any watch retirement
            // can block on the native observer owner.
            for index in expired {
                self.runs[index].scope_bound = false;
                self.adapter.retire_scope(self.runs[index].registration);
            }
            Ok(())
        })();
        if let Err(error) = &result {
            self.fault = Some(error.clone());
            self.cleanup();
        }
        result
    }

    /// None means no timed work remains; notification adapters never poll.
    pub(super) fn wait_duration(&self, now: u64) -> Option<Duration> {
        let lease = self.runs.iter().filter(|run| run.live)
            .map(|run| Duration::from_micros(run.lease.saturating_sub(now))).min();
        let due = lease.into_iter().chain(self.guards.wait(now)).min();
        if self.adapter.notification_driven() { due }
        else { Some(due.unwrap_or(Duration::from_millis(5)).min(Duration::from_millis(5))) }
    }

    pub(super) fn scope_receipts(&self) -> Vec<ScopeReceipt> {
        self.runs.iter().map(|run| ScopeReceipt {
            admission: ScopeAdmission { registration: Counter(run.registration), epoch: Counter(run.epoch) },
            live: run.live, eligible: run.live && run.source_eligible,
        }).collect()
    }

    pub(super) fn admit(&mut self, request: &Request, now: u64, ack: &mut Ack) -> Result<bool, String> {
        let (token, arm) = match &request.command {
            Command::Arm { producer, .. } => (&producer.run, true),
            Command::Deliver { delivery } | Command::DeliverGuarded { delivery, .. }
                if !matches!(delivery.intent, Intent::Cleanup)
                    && !matches!(&delivery.intent, Intent::Held { controls } if controls.is_empty()) => (&delivery.producer.run, false),
            Command::DeliverPointerBatch { deliveries } => (&deliveries[0].producer.run, false),
            Command::DeliverKeyPair { deliveries } => (&deliveries[0].producer.run, false),
            _ => return Ok(true),
        };
        if request.admission.is_none() { return Err("Desktop input requires a scope admission proof.".into()); }
        let index = self.runs.iter().position(|run| run.token == *token && run.live);
        let matches = index.is_some_and(|index| {
            let run = &self.runs[index];
            request.admission == Some(ScopeAdmission { registration: Counter(run.registration), epoch: Counter(run.epoch) })
        });
        // A stale packet has no authority over any current reducer state.
        if !matches {
            ack.skipped = true;
            ack.scope_rejected = true;
            return Ok(false);
        }
        let index = index.expect("matched live scope");
        if arm && request.deadline_us.is_some_and(|deadline| now >= deadline.0) {
            return Ok(true);
        }
        if !arm && self.runs[index].paused {
            ack.skipped = true;
            ack.scope_rejected = true;
            return Ok(false);
        }
        if !self.adapter.eligible(&self.runs[index].scope).unwrap_or(false) {
            self.runs[index].source_eligible = false;
            if let Err(error) = self.pause_scope(index, true) {
                self.fault = Some(error.clone());
                self.cleanup();
                return Err(error);
            }
            ack.skipped = true;
            ack.scope_rejected = true;
            return Ok(false);
        }
        Ok(true)
    }
}

#[cfg(test)]
#[path = "scope_tests.rs"]
mod tests;
