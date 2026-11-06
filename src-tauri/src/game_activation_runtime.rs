//! Independent native game observer. Integration setup/readiness has no bearing
//! on whether an ordinary hardware profile can match a running executable.
use super::*;
use osg_profiles::activation::{GameIdentity, VerifiedGame};
impl GamesRuntime {
    pub(crate) fn try_observed_desktop_process(&self, game_id: &str)
        -> Result<crate::game_activation::CurrentProcessObservation, String> {
        if self.inner.closed.load(Ordering::Acquire) { return Err("The native game observer has stopped.".into()); }
        Ok(self.inner.activation.try_current_process(game_id))
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
                let mut watched_library = None;
                while !inner.closed.load(Ordering::Acquire) {
                    let observed = wake.observed();
                    #[cfg(windows)]
                    let library = inner.changes.context_wake.observed();
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
                        if watched_library != Some(library) {
                            if events.as_ref().is_none_or(|events| events.is_finished()) {
                                events = inner.platform.watch_activation_processes(&games, [wake.clone(), inner.changes.context_process_wake.clone()]);
                                watched_library = Some(library);
                            } else if let Some(events) = &events {
                                events.retire();
                            }
                        }
                    }
                    #[cfg(windows)]
                    let continuity = events.as_ref().and_then(|events| events.continuity());
                    #[cfg(not(windows))]
                    let continuity = None;
                    #[cfg(windows)]
                    inner.changes.set_context_process_scope(library, continuity.clone().filter(|_| watched_library == Some(library)));
                    let event_owned = continuity.is_some();
                    inner.activation.observe_with_continuity(inner.platform.as_ref(), &games, continuity);
                    if !inner.closed.load(Ordering::Acquire) {
                        let retry = (!event_owned).then_some(Duration::from_millis(500));
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
