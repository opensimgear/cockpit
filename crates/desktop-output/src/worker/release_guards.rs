//! Bounded native-clock cleanup of precommitted tap contributions. Guards never
//! acquire controls and never replace the scheduler's ordinary up operation.
use super::*;
use osg_mapping::{desktop::Delivery, engine::Counter};
use std::collections::BTreeMap;

struct Guard {
    delivery: Delivery,
    duration: u64,
    due: u64,
}
#[derive(Default)]
pub(super) struct ReleaseGuards {
    active: BTreeMap<u64, Guard>,
    released: Vec<Counter>,
}
impl ReleaseGuards {
    pub(super) fn validate(&self, command: &Command) -> Result<Option<(Delivery, u64)>, String> {
        let Command::DeliverGuarded { delivery, release_after_us } = command else { return Ok(None) };
        if !(1_000..=60_000_000).contains(&release_after_us.0)
            || !matches!(&delivery.intent, Intent::Held { controls } if !controls.is_empty()) {
            return Err("A desktop release guard requires nonempty Held input and 1–60000 ms dwell.".into());
        }
        if self.active.len().saturating_add(self.released.len()) >= MAX_OWNERS {
            return Err("Desktop release guard limit exceeded.".into());
        }
        Ok(Some((delivery.clone(), release_after_us.0)))
    }
    pub(super) fn install(&mut self, sequence: u64, delivery: Delivery, duration: u64, now: u64) {
        self.active.insert(sequence, Guard { delivery, duration, due: now.saturating_add(duration) });
    }
    pub(super) fn anchor(&mut self, sequence: u64, now: u64) {
        if let Some(guard) = self.active.get_mut(&sequence) {
            guard.due = now.saturating_add(guard.duration);
        }
    }
    pub(super) fn prune(&mut self, reducer: &Reducer) {
        self.active.retain(|_, guard| reducer.owns_exact(&guard.delivery));
    }
    pub(super) fn clear(&mut self) { self.active.clear(); }
    pub(super) fn take_released(&mut self) -> Vec<Counter> { std::mem::take(&mut self.released) }
    pub(super) fn wait(&self, now: u64) -> Option<Duration> {
        self.active.values().map(|guard| Duration::from_micros(guard.due.saturating_sub(now)))
            .min()
    }
}
impl<A: Adapter> Worker<A> {
    /// `serve` reads the clock only after native submission and ACK construction.
    /// No newly installed guard can expire before its own ACK has been formed.
    pub(super) fn anchor_release_guard(&mut self, ack: &Ack, now: u64) {
        if ack.ok && !ack.skipped { self.guards.anchor(ack.sequence.0, now); }
    }
    pub(super) fn release_due(&mut self, now: u64) -> Result<(), String> {
        self.guards.prune(&self.reducer);
        let due = self.guards.active.iter().filter(|(_, guard)| guard.due <= now)
            .map(|(sequence, _)| *sequence).collect::<Vec<_>>();
        for sequence in due {
            let guard = self.guards.active.remove(&sequence).expect("due guard exists");
            let Some(events) = self.reducer.release_exact(&guard.delivery)? else { continue };
            let count = native_count(&events);
            let submitted = if count == 0 { Ok(0) } else {
                let _context = SubmissionGuard::enter(sequence, guard.delivery.operation_id.0, 7);
                self.adapter.submit(&events)
            };
            match submitted {
                Ok(confirmed) if confirmed == count => {
                    self.confirmed = self.reducer.held();
                    self.guards.released.push(Counter(sequence));
                }
                result => {
                    let error = result.err().unwrap_or_else(|| "Desktop guarded release returned a short count; delivery is uncertain.".into());
                    self.fault = Some(error.clone());
                    self.cleanup();
                    return Err(error);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "release_guard_tests.rs"]
mod tests;
