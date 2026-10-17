//! Independent native game observer. Integration setup/readiness has no bearing
//! on whether an ordinary hardware profile can match a running executable.
use super::*;
use osg_profiles::activation::{GameIdentity, VerifiedGame};
impl GamesRuntime {
    fn process_monitoring_supported(&self) -> bool {
        self.inner
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
                while !inner.closed.load(Ordering::Acquire) {
                    let games = inner
                        .settings
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .catalogue
                        .clone();
                    inner.activation.observe(inner.platform.as_ref(), &games);
                    for _ in 0..5 {
                        if inner.closed.load(Ordering::Acquire) {
                            break;
                        }
                        thread::sleep(Duration::from_millis(100));
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
    pub(crate) fn profile_activation_identity(
        &self,
        game_id: &str,
    ) -> Result<GameIdentity, String> {
        self.inner.guard_installation_metadata()?;
        let game = self
            .inner
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .catalogue
            .iter()
            .find(|game| game["id"] == game_id)
            .cloned()
            .ok_or("Choose a current native game installation.")?;
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
