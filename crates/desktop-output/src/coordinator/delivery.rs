//! Projections begin after reconciling any worker-owned release receipts.
use super::*;

impl Coordinator {
    pub fn arm(&mut self, producer: Token, action_id: String) -> Result<(), String> {
        let mut next = self.reducer.clone();
        next.arm(producer.clone(), action_id.clone())?;
        self.send_with_commit(Command::Arm { producer: producer.clone(), action_id: action_id.clone() },
            None, next.held(), |coordinator, ack| {
                if !ack.released_guards.is_empty() {
                    next = coordinator.reducer.clone();
                    next.arm(producer, action_id)?;
                }
                coordinator.reducer = next;
                coordinator.snapshot.phase = "active".into();
                coordinator.update_owners();
                Ok(true)
            })?;
        Ok(())
    }

    pub fn submit(&mut self, delivery: Delivery, source_now_us: u64) -> Result<Ack, String> {
        self.submit_inner(delivery, source_now_us, None)
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
        self.submit_inner(delivery, source_now_us, Some(release_after_us))
    }

    fn submit_inner(&mut self, delivery: Delivery, source_now_us: u64,
        release_after_us: Option<Counter>) -> Result<Ack, String> {
        let expired = delivery.deadline_us.is_some_and(|d| source_now_us >= d.0)
            && !matches!(delivery.intent, Intent::Cleanup);
        let deadline_us = delivery.deadline_us.map(|d|
            Counter((self.clock)().saturating_add(d.0.saturating_sub(source_now_us))));
        let mut next = self.reducer.clone();
        next.delivery(&delivery, expired)?;
        let command = match release_after_us {
            Some(release_after_us) => Command::DeliverGuarded { delivery: delivery.clone(), release_after_us },
            None => Command::Deliver { delivery: delivery.clone() },
        };
        self.send_with_commit(command, deadline_us, next.held(), |coordinator, ack| {
            // Earlier guards may have expired before this ACK, including during
            // unrelated pointer traffic. Never restore a pre-ACK owner clone.
            if !ack.released_guards.is_empty() || (ack.skipped && matches!(delivery.intent, Intent::Held { .. })) {
                next = coordinator.reducer.clone();
                next.delivery(&delivery, expired || ack.skipped)?;
            }
            coordinator.reducer = next;
            Ok(coordinator.update_owners())
        })
    }

    pub fn renew(&mut self, run: RunToken) -> Result<(), String> {
        self.send(Command::Renew { run }, None, self.reducer.held())?;
        Ok(())
    }

    pub fn pause(&mut self, run: RunToken) -> Result<(), String> {
        let mut next = self.reducer.clone();
        next.pause(&run);
        self.send_with_commit(Command::Pause { run: run.clone() }, None, next.held(), |coordinator, ack| {
            if !ack.released_guards.is_empty() {
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
