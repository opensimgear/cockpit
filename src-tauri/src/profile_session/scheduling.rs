//! Real reducer deadlines can shorten, never extend, remaining service waits.
use super::*;
use std::time::{Duration, Instant};

impl<H: Host> Coordinator<H> {
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        if self.launch.is_some() { return None; }
        let run = self.manifest.active.as_ref().filter(|run| self.validates(&run.owner))?;
        self.host.next_deadline(run)
    }
}

/// `before` precedes the native clock read. Any delay while reading that clock
/// therefore makes this estimate conservative; host work never restarts a wait.
pub(crate) fn host_deadline(before: Instant, sampled_us: u64, deadline_us: u64) -> Instant {
    before.checked_add(Duration::from_micros(deadline_us.saturating_sub(sampled_us)))
        .unwrap_or(before)
}

/// The fallback still services adapters without completion/lease notifications.
/// It is not an input/report cadence and is not full reactive qualification.
pub(crate) fn service_wait(active: bool, deadline: Option<Instant>, now: Instant) -> Duration {
    let service = Duration::from_millis(if active { 5 } else { 100 });
    deadline.map_or(service, |at| service.min(at.saturating_duration_since(now)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_authored_deadlines_are_not_rounded_to_the_service_interval() {
        let before = Instant::now();
        let deadline = host_deadline(before, 10_000, 10_750);
        assert_eq!(service_wait(true, Some(deadline), before), Duration::from_micros(750));
        assert_eq!(service_wait(false, Some(deadline), before), Duration::from_micros(750));
    }

    #[test]
    fn host_work_and_due_deadlines_never_restart_or_extend_a_wait() {
        let before = Instant::now();
        let deadline = host_deadline(before, 20_000, 24_000);
        assert_eq!(service_wait(true, Some(deadline), before + Duration::from_millis(3)), Duration::from_millis(1));
        assert_eq!(service_wait(true, Some(deadline), before + Duration::from_millis(5)), Duration::ZERO);
        assert_eq!(host_deadline(before, 24_000, 20_000), before);
    }

    #[test]
    fn distant_deadlines_preserve_independent_adapter_service_bounds() {
        let now = Instant::now();
        let future = Some(now + Duration::from_secs(1));
        assert_eq!(service_wait(true, future, now), Duration::from_millis(5));
        assert_eq!(service_wait(false, future, now), Duration::from_millis(100));
        assert_eq!(service_wait(true, None, now), Duration::from_millis(5));
        assert_eq!(service_wait(false, None, now), Duration::from_millis(100));
    }
}
