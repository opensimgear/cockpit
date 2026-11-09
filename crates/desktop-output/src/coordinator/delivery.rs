//! Projections begin after reconciling any worker-owned release receipts.
use super::*;
use super::transaction::{DeferredJournal, JournalMode};

impl Coordinator {
    pub fn is_key_acquisition(delivery: &Delivery) -> bool {
        matches!(&delivery.intent, Intent::Held { controls }
            if matches!(controls.as_slice(), [Control::Key { .. }]))
    }
    pub fn arm(&mut self, producer: Token, action_id: String) -> Result<(), String> {
        self.arm_checked(producer, action_id).map(|_| ())
    }
    pub fn arm_checked(&mut self, producer: Token, action_id: String) -> Result<bool, String> {
        let revision = self.projection_revision;
        let mut next = self.reducer.clone();
        let mut expected = native_count(&next.arm(producer.clone(), action_id.clone())?);
        let ack = self.send_with_commit(Command::Arm { producer: producer.clone(), action_id: action_id.clone() },
            None, next.held(), |coordinator, ack| {
                if ack.scope_rejected {
                    next = coordinator.reducer.clone();
                    expected = 0;
                } else if ack.skipped {
                    next = coordinator.reducer.clone();
                    expected = native_count(&next.pause(&producer.run));
                } else if revision != coordinator.projection_revision {
                    next = coordinator.reducer.clone();
                    expected = native_count(&next.arm(producer, action_id)?);
                }
                if ack.attempted != expected { return Err("Desktop Arm acknowledgement has incorrect native counts.".into()); }
                coordinator.reducer = next;
                if !ack.scope_rejected {
                    coordinator.snapshot.phase = if ack.skipped { "paused" } else { "active" }.into();
                }
                coordinator.update_owners();
                Ok(true)
            })?;
        Ok(!ack.skipped)
    }

    pub fn submit(&mut self, delivery: Delivery, source_now_us: u64) -> Result<Ack, String> {
        self.submit_inner(delivery, source_now_us, None, JournalMode::Immediate)
    }

    /// An already queued key acquisition may share the previous receipt's
    /// durable write. The caller checks that it still has authority after the
    /// first ACK. Each request keeps its own native scope gate and ACK.
    pub fn submit_key_pair(
        &mut self,
        first: Delivery,
        source_now_us: u64,
        next: impl FnOnce(&Ack) -> Option<(Delivery, u64)>,
    ) -> Result<(Ack, Result<Option<Ack>, String>), String> {
        let mut deferred = DeferredJournal::default();
        let mode = if Self::is_key_acquisition(&first) {
            JournalMode::Defer(&mut deferred)
        } else {
            JournalMode::Immediate
        };
        let first = self.submit_inner(first, source_now_us, None, mode)?;
        let second = match next(&first) {
            Some((delivery, now)) if Self::is_key_acquisition(&delivery) => self
                .submit_inner(delivery, now, None, JournalMode::Carry(&mut deferred))
                .map(Some),
            Some((delivery, now)) => {
                self.finish_deferred_journal(&mut deferred)?;
                self.submit(delivery, now).map(Some)
            }
            None => Ok(None),
        };
        self.finish_deferred_journal(&mut deferred)?;
        if let Some(error) = deferred.failure {
            return Err(error);
        }
        Ok((first, second))
    }

    /// The original scheduler still supplies its ordinary up. The native guard
    /// can only release that exact contribution while down persistence finishes.
    pub fn submit_guarded(&mut self, delivery: Delivery, source_now_us: u64,
        release_after_us: Counter) -> Result<Ack, String> {
        if !(1_000..=60_000_000).contains(&release_after_us.0)
            || !matches!(&delivery.intent, Intent::Held { controls } if !controls.is_empty()) {
            return Err("A desktop release guard requires nonempty Held input and 1–60000 ms dwell.".into());
        }
        self.check_guard_capacity(&delivery)?;
        self.submit_inner(delivery, source_now_us, Some(release_after_us), JournalMode::Immediate)
    }

    fn submit_inner(&mut self, delivery: Delivery, source_now_us: u64,
        release_after_us: Option<Counter>, journal: JournalMode<'_>) -> Result<Ack, String> {
        let revision = self.projection_revision;
        let expired = delivery.deadline_us.is_some_and(|d| source_now_us >= d.0)
            && !matches!(delivery.intent, Intent::Cleanup);
        let deadline_us = delivery.deadline_us.map(|d|
            Counter((self.clock)().saturating_add(d.0.saturating_sub(source_now_us))));
        let command = match release_after_us {
            Some(release_after_us) => Command::DeliverGuarded { delivery: delivery.clone(), release_after_us },
            None => Command::Deliver { delivery: delivery.clone() },
        };
        let mut next = self.reducer.clone();
        let stale = self.admission_is_stale(&command);
        let mut expected = None;
        if stale {
            // Validate bounded shape without reviving an owner removed by a
            // receipt. Only the helper may attest this stale command's skip.
            let mut probe = Reducer::default();
            probe.arm(delivery.producer.clone(), delivery.action_id.clone())?;
            probe.delivery(&delivery, true)?;
        } else { expected = Some(next.delivery(&delivery, expired)?); }
        let mut possible = next.held();
        if stale {
            if let Intent::Held { controls } = &delivery.intent {
                // Even an expected rejection retains the input's possible
                // acquisition until the complete acknowledgement proves it.
                crate::reducer::recovery_releases(controls)?;
                possible.extend(controls.iter().cloned());
            }
        }
        self.send_with_journal(command, deadline_us, possible, |coordinator, ack| {
            // Earlier guards may have expired before this ACK, including during
            // unrelated pointer traffic. Never restore a pre-ACK owner clone.
            if ack.scope_rejected {
                next = coordinator.reducer.clone();
            } else if revision != coordinator.projection_revision || ack.skipped {
                next = coordinator.reducer.clone();
                expected = Some(next.delivery(&delivery, expired || ack.skipped)?);
            }
            if !ack.scope_rejected {
                let expected = expected.as_ref().ok_or("Desktop stale admission was not rejected.")?;
                if ack.attempted != native_count(&expected.events) || ack.collision != expected.collision {
                    return Err("Desktop delivery acknowledgement has incorrect native counts or collision.".into());
                }
            }
            coordinator.reducer = next;
            Ok(coordinator.update_owners())
        }, journal)
    }

    pub fn renew(&mut self, run: RunToken) -> Result<(), String> {
        self.send(Command::Renew { run }, None, self.reducer.held())?;
        Ok(())
    }

    pub fn pause(&mut self, run: RunToken) -> Result<(), String> {
        let revision = self.projection_revision;
        let mut next = self.reducer.clone();
        next.pause(&run);
        self.send_with_commit(Command::Pause { run: run.clone() }, None, next.held(), |coordinator, _ack| {
            if revision != coordinator.projection_revision {
                next = coordinator.reducer.clone();
                next.pause(&run);
            }
            coordinator.reducer = next;
            coordinator.snapshot.phase = "paused".into();
            coordinator.update_owners();
            Ok(true)
        })?;
        Ok(())
    }
}
