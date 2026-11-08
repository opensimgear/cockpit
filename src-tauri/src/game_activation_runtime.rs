//! Independent native game observer. Integration setup/readiness has no bearing
//! on whether an ordinary hardware profile can match a running executable.
use super::*;
use crate::game_discovery::{BackgroundProcesses, ProcessLookup, ProcessLookupError, ACTIVE_PROCESS_PROOF_AGE, PROCESS_DISCOVERY_PERIOD};
use osg_profiles::activation::{GameIdentity, VerifiedGame};

pub(super) struct BackgroundScan {
    epoch: (u64, u64),
    started: Instant,
    event_owned: bool,
    rows: BackgroundProcesses,
}

impl BackgroundScan {
    pub(super) fn new(epoch: (u64, u64), started: Instant, event_owned: bool, rows: BackgroundProcesses) -> Self {
        Self { epoch, started, event_owned, rows }
    }

    fn fits_profile_proof(&self) -> bool {
        self.event_owned || self.started.elapsed() + ACTIVE_PROCESS_PROOF_AGE < PROCESS_DISCOVERY_PERIOD * 2
    }

    fn changes_reacquisition(&self, previous: &Self) -> bool {
        self.epoch != previous.epoch || self.event_owned != previous.event_owned
            || !self.rows.same_integrations(&previous.rows)
            || !previous.fits_profile_proof() && self.fits_profile_proof()
    }
}

impl Inner {
    pub(super) fn read_context_processes(&self, epoch: (u64, u64), games: &[&Value]) -> Vec<ProcessLookup> {
        if self.platform.shares_background_processes() {
            self.background_integration_processes(epoch, games)
        } else {
            self.platform.integration_processes(games)
        }
    }

    fn background_integration_processes(&self, epoch: (u64, u64), games: &[&Value]) -> Vec<ProcessLookup> {
        let scan = self.background_processes.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(scan) = scan.as_ref().filter(|scan| scan.epoch == epoch &&
            (self.changes.context_process_live(epoch.0) || scan.started.elapsed() < PROCESS_DISCOVERY_PERIOD * 2))
        {
            return scan.rows.integration(games);
        }
        games.iter().map(|_| Err(ProcessLookupError::Unavailable("Waiting for the shared game process observation.".into()))).collect()
    }

    pub(super) fn profile_background_process(&self, game: &Value) -> Option<((u64, u64), Instant, ProcessLookup)> {
        let scope = || {
            let library = self.changes.context_wake.observed();
            let process = self.changes.context_process_wake.observed();
            ((library, process), self.changes.context_process_live(library))
        };
        let (epoch, event_owned) = scope();
        let scan = self.background_processes.lock().unwrap_or_else(|p| p.into_inner());
        let scan = scan.as_ref()?;
        if self.closed.load(Ordering::Acquire) || scan.epoch != epoch || scan.event_owned != event_owned {
            return None;
        }
        // A new short liveness proof must fit inside the original uniqueness
        // window. Reading a cached scan cannot move either end of that window.
        if !scan.fits_profile_proof() {
            return None;
        }
        let row = scan.rows.integration_row(game)?;
        (scope() == (epoch, event_owned)).then_some((epoch, scan.started, row))
    }
}

impl GamesRuntime {
    pub(crate) fn game_reacquisition_revision(&self) -> Option<u64> {
        self.inner.platform.shares_background_processes()
            .then(|| self.inner.changes.game_reacquisition_revision())
    }

    pub(crate) fn process_event_preference_changed(&self) {
        self.inner.changes.activation_wake.notify();
    }
    pub(crate) fn try_observed_desktop_process(&self, game_id: &str)
        -> Result<crate::game_activation::CurrentProcessObservation, String> {
        if self.inner.closed.load(Ordering::Acquire) { return Err("The native game observer has stopped.".into()); }
        Ok(self.inner.activation.try_current_process(game_id))
    }
    pub(crate) fn desktop_process_observation(&self, game_id: &str)
        -> (crate::game_activation::CurrentProcessObservation, Option<crate::game_activation::ProcessObservationLease>) {
        if self.inner.closed.load(Ordering::Acquire) || !self.inner.activation.worker_live() {
            return (crate::game_activation::CurrentProcessObservation::Unavailable, None);
        }
        self.inner.activation.try_current_process_with_lease(game_id)
    }
    /// Membership for current native selections is distinct from the syntax
    /// checks used to read persisted profiles and removed-game recovery state.
    pub(crate) fn registered_profile_game(&self, game_id: &str) -> Result<Value, String> {
        self.inner.guard_installation_metadata()?;
        if !osg_profiles::activation::valid_game_id(game_id) {
            return Err("Choose a valid native game identity.".into());
        }
        self.inner
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .catalogue
            .iter()
            .find(|game| game["id"] == game_id)
            .cloned()
            .ok_or_else(|| "The selected game is no longer registered on this computer.".into())
    }
    fn process_monitoring_supported(&self) -> bool {
        self.inner.platform.process_monitoring_supported()
            || self
                .inner
                .settings
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .catalogue
                .iter()
                .any(|game| game["processMonitoringSupported"] == true)
    }
    pub(super) fn start_activation_observer(&self) {
        if self.inner.guard_installation_metadata().is_err() || !self.process_monitoring_supported()
        {
            return;
        }
        let inner = self.inner.clone();
        let worker = thread::Builder::new()
            .name("profile-game-context".into())
            .spawn(move || {
                let wake = &inner.changes.activation_wake;
                if wake.bind_thread(thread::current()).is_err() {
                    return;
                }
                let _worker = inner.activation.worker_scope();
                #[cfg(windows)]
                let mut events: Option<crate::xplane::process_events::InventoryEvents> = None;
                #[cfg(windows)]
                let mut watched_scope = None;
                #[cfg(windows)]
                let mut restarted_library = None;
                while !inner.closed.load(Ordering::Acquire) {
                    let observed = wake.observed();
                    let library = inner.changes.context_wake.observed();
                    #[cfg(windows)]
                    let enabled = inner.process_preferences.as_ref()
                        .is_some_and(|preferences| preferences.cached("game-process-events") == true);
                    let games = if inner.guard_installation_metadata().is_ok() {
                        inner
                            .settings
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .catalogue
                            .clone()
                    } else {
                        vec![]
                    };
                    #[cfg(windows)]
                    {
                        let scope = (library, enabled);
                        let replacement = watched_scope != Some(scope);
                        // Recover once after live continuity is lost. Initial
                        // permission denial must not become a retry loop.
                        let restart = enabled && !replacement && restarted_library != Some(library)
                            && events.as_ref().is_some_and(|events| events.failed_after_live());
                        if replacement || restart {
                            if events.as_ref().is_none_or(|events| events.is_finished()) {
                                // Advance the context epoch before waking the owner that captures it.
                                events = enabled.then(|| inner.platform.watch_activation_processes(&games,
                                    [inner.changes.context_process_wake.clone(), wake.clone()])).flatten();
                                watched_scope = Some(scope);
                                restarted_library = restart.then_some(library);
                            } else if let Some(events) = &events {
                                events.retire();
                                if replacement { watched_scope = None; }
                            }
                        }
                    }
                    #[cfg(windows)]
                    let continuity = (enabled && watched_scope == Some((library, true)))
                        .then(|| events.as_ref().and_then(|events| events.continuity())).flatten();
                    #[cfg(not(windows))]
                    let continuity = None;
                    #[cfg(windows)]
                    inner.changes.set_context_process_scope(library, continuity.clone());
                    let event_owned = continuity.is_some();
                    let epoch = (library, inner.changes.context_process_wake.observed());
                    let started = Instant::now();
                    let background = inner.platform.background_processes(&games);
                    if background.is_some()
                        && epoch != (inner.changes.context_wake.observed(), inner.changes.context_process_wake.observed())
                    {
                        continue;
                    }
                    let retry_inventory = background.as_ref().is_some_and(BackgroundProcesses::needs_retry);
                    inner.activation.observe_with_continuity(inner.platform.as_ref(), &games, continuity, background.as_ref());
                    if let Some(rows) = background {
                        let next = BackgroundScan::new(epoch, started, event_owned, rows);
                        let mut published = inner.background_processes.lock().unwrap_or_else(|p| p.into_inner());
                        let changed = published.as_ref().is_none_or(|previous| next.changes_reacquisition(previous));
                        *published = Some(next);
                        drop(published);
                        inner.changes.context_update_wake.notify();
                        if changed { inner.changes.mark_game_reacquisition(); }
                        else { inner.changes.wake_routing(); }
                    }
                    if !inner.closed.load(Ordering::Acquire) {
                        let retry = (!event_owned || retry_inventory).then_some(PROCESS_DISCOVERY_PERIOD)
                            .filter(|_| games.iter().any(crate::game_activation::process_candidate));
                        wake.wait_scheduled(observed, retry);
                    }
                }
            })
            .ok();
        *self
            .activation_observer
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = worker;
    }
    pub(crate) fn profile_activation_observations(&self) -> Result<Vec<VerifiedGame>, String> {
        self.inner.guard_installation_metadata()?;
        if !self.process_monitoring_supported() {
            return Err(
                "Automatic game profile selection requires native Windows process monitoring."
                    .into(),
            );
        }
        self.inner.activation.snapshot()
    }
    pub(crate) fn confirm_started_profile_activation(
        &self,
        started: &super::launch::StartedGame,
    ) -> Result<Option<VerifiedGame>, String> {
        self.validate_started_profile_game(started)?;
        Ok(self
            .inner
            .activation
            .confirmed_process(&started.identity, &started.process))
    }
    pub(crate) fn has_started_profile_process_proof(
        &self,
        started: &super::launch::StartedGame,
    ) -> bool {
        self.inner.activation.confirmed_process(&started.identity, &started.process).is_some()
    }
    pub(crate) fn profile_activation_identity(
        &self,
        game_id: &str,
    ) -> Result<GameIdentity, String> {
        let game = self.registered_profile_game(game_id)?;
        crate::game_activation::identity(self.inner.platform.as_ref(), &game)
    }
    pub(crate) fn validate_profile_activation(
        &self,
        expected: &VerifiedGame,
    ) -> Result<(), String> {
        self.inner.guard_installation_metadata()?;
        let game = self
            .inner
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .catalogue
            .iter()
            .find(|game| game["id"] == expected.identity.game_id)
            .cloned()
            .ok_or("The selected game installation changed during preparation.")?;
        self.inner
            .activation
            .validate(self.inner.platform.as_ref(), &game, expected)
    }
}

#[cfg(test)]
#[path = "game_activation_runtime_tests.rs"]
mod tests;
