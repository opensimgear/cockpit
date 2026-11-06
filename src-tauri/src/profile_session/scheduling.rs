//! Real reducer deadlines can shorten, never extend, remaining service waits.
use super::*;
use std::time::{Duration, Instant};

impl<H: Host> Coordinator<H> {
    pub(crate) fn service_interval(&self) -> Option<Duration> {
        if let Some(held) = &self.launch {
            let notified = self.held_run(&held.owner).is_ok_and(|run| self.host.hold_uses_notifications(run));
            return (!notified).then_some(Duration::from_millis(5));
        }
        self.status.owner.as_ref()?;
        self.manifest.active.as_ref().filter(|run| self.validates(&run.owner))
            .map_or(Some(Duration::from_millis(5)), |run| self.host.service_interval(run))
    }
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        if let Some(held) = &self.launch {
            let host = self.held_run(&held.owner).ok().and_then(|run| self.host.hold_deadline(run));
            return Some(host.map_or(held.deadline, |deadline| deadline.min(held.deadline)));
        }
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
pub(crate) fn service_wait(service: Option<Duration>, deadline: Option<Instant>, now: Instant) -> Option<Duration> {
    match (service, deadline.map(|at| at.saturating_duration_since(now))) {
        (Some(service), Some(deadline)) => Some(service.min(deadline)),
        (service, deadline) => service.or(deadline),
    }
}

/// Only wholly internal authored inputs can opt out of legacy adapter service.
/// Check the source document too: executable vehicle variants clear their tabs.
pub(crate) fn internal_sources_only(document: &ProfileDocument) -> bool {
    document.resources.iter().all(|resource| resource.kind == osg_profiles::ResourceKind::DebugController)
        && document.virtual_outputs.is_empty() && document.desktop_outputs.is_empty()
        && document.visibility.is_empty() && document.overrides.is_empty()
        && document.companion_assignments.is_empty() && document.vehicle_tabs.is_empty()
        && document.eligibility.game_ids.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_authored_deadlines_are_not_rounded_to_the_service_interval() {
        let before = Instant::now();
        let deadline = host_deadline(before, 10_000, 10_750);
        assert_eq!(service_wait(Some(Duration::from_millis(5)), Some(deadline), before), Some(Duration::from_micros(750)));
        assert_eq!(service_wait(None, Some(deadline), before), Some(Duration::from_micros(750)));
    }

    #[test]
    fn host_work_and_due_deadlines_never_restart_or_extend_a_wait() {
        let before = Instant::now();
        let deadline = host_deadline(before, 20_000, 24_000);
        assert_eq!(service_wait(None, Some(deadline), before + Duration::from_millis(3)), Some(Duration::from_millis(1)));
        assert_eq!(service_wait(None, Some(deadline), before + Duration::from_millis(5)), Some(Duration::ZERO));
        assert_eq!(host_deadline(before, 24_000, 20_000), before);
    }

    #[test]
    fn distant_deadlines_preserve_independent_adapter_service_bounds() {
        let now = Instant::now();
        let future = Some(now + Duration::from_secs(1));
        assert_eq!(service_wait(Some(Duration::from_millis(5)), future, now), Some(Duration::from_millis(5)));
        assert_eq!(service_wait(Some(Duration::from_millis(100)), future, now), Some(Duration::from_millis(100)));
        assert_eq!(service_wait(Some(Duration::from_millis(5)), None, now), Some(Duration::from_millis(5)));
        assert_eq!(service_wait(None, None, now), None);
    }

    #[test]
    fn external_sources_and_source_vehicle_variants_keep_adapter_service() {
        let mut source = osg_profiles::default_document();
        assert!(internal_sources_only(&source));
        source.eligibility.game_ids.push("dcs".into());
        assert!(!internal_sources_only(&source));
        source.eligibility.game_ids.clear();
        source.vehicle_tabs.push(osg_profiles::VehicleTab { id:"aircraft".into(), game_id:"dcs".into(), vehicle_id:"FA-18C_hornet".into(), vehicle_name:"Hornet".into(), node_ids:vec![] });
        assert!(!internal_sources_only(&source));
        source.vehicle_tabs.clear();
        source.visibility.push(osg_profiles::VisibilityIntent { resource_id:"external".into(), mode:osg_profiles::VisibilityMode::Visible });
        assert!(!internal_sources_only(&source));
    }
}
