//! Passive capture projection is isolated by integration. Scope commits wake
//! every worker, while a provider capture or inspection wakes only its worker.
use super::*;
use osg_streams::NativeWake;

pub(super) fn start(inner: Arc<Inner>, stop: Arc<AtomicBool>) -> Vec<thread::JoinHandle<()>> {
    inner.changes.vehicle_capture_wakes.iter().map(|(integration, wake)| {
        let inner = inner.clone();
        let stop = stop.clone();
        let wake = wake.clone();
        thread::spawn(move || run(&inner, &stop, integration, &wake))
    }).collect()
}

fn run(inner: &Inner, stop: &AtomicBool, integration: &str, wake: &NativeWake) {
    let _ = wake.bind_thread(thread::current());
    let deadline = || {
        inner.vehicles.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
            .capture_deadline_for(Some(integration))
    };
    let mut drained = None;
    let mut scope = 0;
    while !inner.closed.load(Ordering::Acquire) && !stop.load(Ordering::Acquire) {
        let observed = wake.observed();
        let current_scope = inner.changes.vehicle_scope_wake.observed();
        if Some(observed) != drained || observed == u64::MAX
            || deadline().is_some_and(|at| Instant::now() >= at)
        {
            inner.poll_vehicle_telemetry(integration, current_scope != scope || current_scope == u64::MAX);
        }
        scope = current_scope;
        drained = Some(observed);
        wake.wait_scheduled(observed, deadline().map(|at| at.saturating_duration_since(Instant::now())));
    }
}

impl Inner {
    pub(super) fn vehicle_telemetry_games(&self) -> Vec<Value> {
        let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        if settings.metadata_error.is_some() {
            return vec![];
        }
        settings.catalogue.iter()
            .filter(|game| crate::game_discovery::integration_key(game).is_some())
            .cloned()
            .collect()
    }
    pub(super) fn poll_vehicle_telemetry(&self, integration: &str, recheck_scope: bool) {
        let candidates = self.vehicles.lock().unwrap_or_else(|p| p.into_inner())
            .capture_candidates_for(Some(integration), recheck_scope);
        if candidates.is_empty() {
            return;
        }
        let revision = |games: &[Value], id: &str| {
            games.iter().find(|game| game["id"] == id)
                .map_or_else(String::new, |game| self.setup.vehicle_setup_revision(game))
        };
        let games = self.vehicle_telemetry_games();
        let verified = candidates.into_iter()
            .map(|candidate| candidate.verify(&games, |id| revision(&games, id), |_| None).without_renewal())
            .collect();
        let polls = self.update_vehicles(|vehicles| vehicles.begin_polls(verified));
        for poll in polls {
            let result = poll.read();
            let games = self.vehicle_telemetry_games();
            let result = result.verify(&games, |id| revision(&games, id), |_| None);
            self.update_vehicles(|vehicles| vehicles.apply_poll(result));
        }
    }
}
