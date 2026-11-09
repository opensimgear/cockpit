//! Projections begin after reconciling any worker-owned release receipts.
use super::*;
use super::transaction::{DeferredJournal, JournalMode};

impl Coordinator {
    fn is_key_acquisition(delivery: &Delivery) -> bool {
        matches!(&delivery.intent, Intent::Held { controls }
            if matches!(controls.as_slice(), [Control::Key { .. }]))
    }

    pub(super) fn is_key_release(delivery: &Delivery) -> bool {
        matches!(&delivery.intent, Intent::Held { controls } if controls.is_empty())
    }

    pub fn can_share_key_journal(first: &Delivery, second: &Delivery) -> bool {
        (Self::is_key_acquisition(first) && Self::is_key_acquisition(second))
            || (Self::is_key_release(first) && Self::is_key_release(second))
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

    /// Try two already queued presses as one native submission. A healthy
    /// decline leaves both operations untouched for ordinary ordered dispatch.
    pub fn try_submit_native_key_pair(
        &mut self,
        deliveries: [Delivery; 2],
        source_now_us: u64,
    ) -> Result<Option<[Ack; 2]>, String> {
        if crate::key_pair::validate(&deliveries).is_err() {
            return Ok(None);
        }
        let source_deadline = deliveries.iter().filter_map(|delivery| delivery.deadline_us).min_by_key(|deadline| deadline.0);
        if source_deadline.is_some_and(|deadline| source_now_us >= deadline.0) {
            return Ok(None);
        }
        let command = Command::DeliverKeyPair { deliveries: Box::new(deliveries.clone()) };
        if self.admission_is_stale(&command) {
            return Ok(None);
        }
        let Some((projected, _)) = crate::key_pair::project(&self.reducer, &deliveries)? else {
            return Ok(None);
        };
        let operations = deliveries.each_ref().map(|delivery| delivery.operation_id);
        let revision = self.projection_revision;
        let deadline = source_deadline.map(|deadline| Counter((self.clock)().saturating_add(deadline.0 - source_now_us)));
        let envelope = Request {
            sequence: Counter(self.sequence.saturating_add(1)),
            deadline_us: Some(Counter(u64::MAX)),
            admission: self.request_admission(&command),
            command: command.clone(),
        };
        if serde_json::to_vec(&envelope).map_err(|error| error.to_string())?.len() > FRAME_LIMIT {
            return Ok(None);
        }
        let ack = self.send_with_commit(command, deadline, projected.held(), |coordinator, ack| {
            let expected = if ack.skipped { 0 } else { 2 };
            if ack.operation_id != Some(operations[0]) || ack.attempted != expected || ack.collision {
                return Err("Desktop key pair acknowledgement has invalid identity or native counts.".into());
            }
            if ack.skipped {
                return Ok(false);
            }
            coordinator.reducer = if revision == coordinator.projection_revision {
                projected
            } else {
                crate::key_pair::project(&coordinator.reducer, &deliveries)?
                    .ok_or("Desktop key pair no longer matches its owner projection.")?.0
            };
            Ok(coordinator.update_owners())
        })?;
        if ack.skipped && !ack.scope_rejected {
            return Ok(None);
        }
        if !ack.skipped {
            if let Some(channel) = self.channel.audit_id() {
                crate::audit::record(crate::audit::Kind::KeyPairConfirmed,
                    &[channel, ack.sequence.0, operations[0].0, operations[1].0]);
            }
        }
        Ok(Some(operations.map(|operation| {
            let mut logical = ack.clone();
            logical.operation_id = Some(operation);
            logical.attempted /= 2;
            logical.confirmed /= 2;
            logical
        })))
    }

    /// An already queued KeyHold edge may share the previous receipt's durable
    /// write. Each request retains its own scope gate and ACK.
    pub fn submit_key_pair(
        &mut self,
        first: Delivery,
        source_now_us: u64,
        next: impl FnOnce(&Ack) -> Option<(Delivery, u64)>,
    ) -> Result<(Ack, Result<Option<Ack>, String>), String> {
        let mut deferred = DeferredJournal::default();
        let acquire = Self::is_key_acquisition(&first);
        let release = Self::is_key_release(&first);
        let mode = if acquire || release {
            JournalMode::Defer(&mut deferred)
        } else {
            JournalMode::Immediate
        };
        let first = self.submit_inner(first, source_now_us, None, mode)?;
        let second = match next(&first) {
            Some((delivery, now))
                if (acquire && Self::is_key_acquisition(&delivery))
                    || (release && Self::is_key_release(&delivery)) => self
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
