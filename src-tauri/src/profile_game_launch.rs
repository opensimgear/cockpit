//! Hardware-first launch preparation. This module never applies a profile,
//! attaches a game branch, restores an owner or kills a process. Its caller
//! admits only the short spawn under native ownership admission; observation
//! happens afterwards, with a cancellation check before and after every read.
use super::GamesRuntime;
use crate::{
    game_discovery::ProcessIdentity,
    profile_session::{Candidate, RunOwner, State, Status},
};
use osg_profiles::{
    activation::{GameIdentity, Locks},
    session::SnapshotSummary,
    VisibilityMode,
};
use serde_json::Value;
use std::time::{Duration, Instant};

const PREPARATION_TTL: Duration = Duration::from_secs(60);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(45);

/// Explicit declarations which may change what a game enumerates at startup.
/// A declaration is a restart warning, never proof the game saw a controller.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct EnumerationIntents {
    pub hidden_resources: Vec<String>,
    pub virtual_controllers: Vec<String>,
}
pub(crate) fn enumeration_intents(document: &osg_profiles::ProfileDocument) -> EnumerationIntents {
    let mut result = EnumerationIntents {
        hidden_resources: document
            .visibility
            .iter()
            .filter(|intent| intent.mode == VisibilityMode::HiddenWhileActive)
            .map(|intent| intent.resource_id.clone())
            .collect(),
        virtual_controllers: document
            .virtual_outputs
            .iter()
            .map(|output| output.id.clone())
            .collect(),
    };
    result.hidden_resources.sort();
    result.virtual_controllers.sort();
    result
}
impl EnumerationIntents {
    pub(crate) fn requires_startup_notice(&self) -> bool {
        !self.hidden_resources.is_empty() || !self.virtual_controllers.is_empty()
    }
}

/// Native-only, deliberately neither Clone nor serializable. Consuming this
/// ticket cannot replay the Stage payload or a previous spawn attempt.
pub(crate) struct PreparedLaunch {
    candidate: Candidate,
    installation: Value,
    identity: GameIdentity,
    snapshot: SnapshotSummary,
    prepared_at: Instant,
    pub enumeration: EnumerationIntents,
}
/// Captured by Manager while its shared admission guard is held. Nothing here
/// is accepted from IPC; paused/recovery/operator latches remain authoritative.
pub(crate) struct LaunchAdmission {
    pub candidate: Candidate,
    pub status: Status,
    pub admission_epoch: u64,
    pub locks: Locks,
}
/// Binding after Apply commits is separate: Apply advances native admission,
/// while the consumed Stage keeps its original immutable candidate epoch.
pub(crate) struct ArmedLaunch {
    prepared: PreparedLaunch,
    owner: RunOwner,
    admission_epoch: u64,
}
pub(crate) struct SpawnedLaunch {
    identity: GameIdentity,
    installation: Value,
    owner: RunOwner,
    admission_epoch: u64,
    spawned_at: Instant,
}
#[derive(Debug)]
pub(crate) struct LaunchFailure {
    pub message: String,
    /// A disposition only. Manager must compare the current exact owner before
    /// neutralizing/restoring it; this helper never performs cleanup itself.
    pub cleanup_owner: Option<RunOwner>,
    pub process_may_have_started: bool,
}
#[derive(Debug)]
pub(crate) struct StartedGame {
    pub identity: GameIdentity,
    pub owner: RunOwner,
    pub process: ProcessIdentity,
}
impl PreparedLaunch {
    fn validate(&self, admission: &LaunchAdmission) -> Result<RunOwner, String> {
        if self.prepared_at.elapsed() > PREPARATION_TTL {
            return Err("The hardware-first launch preparation expired; prepare it again.".into());
        }
        if admission.candidate.game_id.is_some()
            || admission.candidate.read_only
            || admission.locks.paused
            || admission.locks.emergency_stopped
            || admission.locks.recovery_needed
            || admission.locks.faulted
            || admission.status.paused
            || admission.status.state != State::AwaitingGame
            || admission.status.fault.is_some()
            || admission.status.applied_snapshot.as_ref() != Some(&self.snapshot)
        {
            return Err("Launch requires the unchanged neutral hardware hold without Pause, Stop or recovery.".into());
        }
        self.candidate
            .stage
            .as_ref()
            .ok_or("The launch has no native profile preparation.")?
            .validate(&admission.candidate)?;
        let owner = admission
            .status
            .owner
            .as_ref()
            .filter(|owner| {
                owner.game_id.is_none()
                    && owner.profile_id == self.snapshot.profile_id
                    && owner.profile_revision == self.snapshot.revision
                    && owner.generation > 0
                    && uuid::Uuid::parse_str(&owner.session)
                        .is_ok_and(|id| id.to_string() == owner.session)
            })
            .ok_or("The hardware profile owner changed before launch.")?;
        Ok(owner.clone())
    }
}
impl GamesRuntime {
    /// Process evidence stays independent of optional simulator integration.
    pub(crate) fn validate_started_profile_game(
        &self,
        started: &StartedGame,
    ) -> Result<(), String> {
        started.identity.validate()?;
        ProcessIdentity::parse(&started.process.value())?;
        let (installation, identity) = self.launch_installation(&started.identity.game_id)?;
        if identity != started.identity
            || self.inner.platform.find(&installation)?.as_ref() != Some(&started.process)
            || !self.inner.platform.alive(&started.process)?
        {
            return Err(
                "The launched game installation or process changed; no context was admitted."
                    .into(),
            );
        }
        Ok(())
    }
    fn launch_installation(&self, id: &str) -> Result<(Value, GameIdentity), String> {
        if self.inner.closed.load(std::sync::atomic::Ordering::Acquire) || self.active() {
            return Err(
                "Close or recover the independent game session before unified launch.".into(),
            );
        }
        let game = self
            .inner
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .catalogue
            .iter()
            .find(|game| game["id"] == id)
            .cloned()
            .ok_or("Choose a current native game installation.")?;
        if game["launchSupported"] != true {
            return Err("This installation has no native game launcher.".into());
        }
        let identity = crate::game_activation::identity(self.inner.platform.as_ref(), &game)?;
        Ok((game, identity))
    }
    pub(crate) fn prepare_profile_launch(
        &self,
        game_id: &str,
        candidate: &Candidate,
    ) -> Result<PreparedLaunch, String> {
        self.prepare_profile_launch_for_selection(game_id, candidate, Some(game_id))
    }
    /// The effective context follows native Manual/Automatic policy. Launching
    /// one game does not select its integration over a higher-priority game.
    pub(crate) fn prepare_profile_launch_for_selection(
        &self,
        game_id: &str,
        candidate: &Candidate,
        selected_game: Option<&str>,
    ) -> Result<PreparedLaunch, String> {
        if selected_game.is_some_and(|id| {
            !matches!(
                id,
                "dcs" | "x-plane-11" | "x-plane-12" | "assetto-corsa-rally"
            )
        }) {
            return Err("Choose a supported native selected context.".into());
        }
        if candidate.game_id.is_some() || candidate.read_only || candidate.admission_epoch.is_none()
        {
            return Err("Prepare a writable hardware-only profile candidate before launch.".into());
        }
        candidate
            .stage
            .as_ref()
            .ok_or("Launch requires an actual native profile Stage.")?
            .validate(candidate)?;
        if selected_game.is_some_and(|game| {
            !candidate.document.eligibility.game_ids.is_empty()
                && !candidate
                    .document
                    .eligibility
                    .game_ids
                    .iter()
                    .any(|id| id == game)
        }) {
            return Err("The selected profile is not eligible for this game.".into());
        }
        let (installation, identity) = self.launch_installation(game_id)?;
        if self.inner.platform.find(&installation)?.is_some() {
            return Err("The game is already running; apply its profile with the startup restart notice instead.".into());
        }
        Ok(PreparedLaunch {
            candidate: candidate.clone(),
            installation,
            identity,
            snapshot: osg_profiles::session::AppliedSnapshot::capture(&candidate.document)?
                .summary()
                .clone(),
            prepared_at: Instant::now(),
            enumeration: enumeration_intents(&candidate.document),
        })
    }
    /// Called after the hardware-only Apply commits. This captures its exact
    /// owner and post-commit epoch without changing the original Stage payload.
    pub(crate) fn arm_profile_launch(
        &self,
        prepared: PreparedLaunch,
        admission: &LaunchAdmission,
    ) -> Result<ArmedLaunch, String> {
        let owner = prepared.validate(admission)?;
        Ok(ArmedLaunch {
            prepared,
            owner,
            admission_epoch: admission.admission_epoch,
        })
    }
    /// The callback acquires native shared admission and invokes `spawn` once
    /// with fresh captured state. It must release that guard before returning;
    /// neither a coordinator lock nor admission is retained by the wait phase.
    pub(crate) fn spawn_profile_launch(
        &self,
        armed: ArmedLaunch,
        admit_spawn: impl FnOnce(
            &mut dyn FnMut(LaunchAdmission) -> Result<(), String>,
        ) -> Result<(), String>,
    ) -> Result<SpawnedLaunch, LaunchFailure> {
        let mut called = false;
        let mut repeated = false;
        let mut owner = None;
        let mut epoch = 0;
        let mut attempted = false;
        let mut spawn_result = None;
        let prepared = armed.prepared;
        let result = admit_spawn(&mut |admission| {
            if called {
                repeated = true;
                return Err("A launch attempt cannot spawn twice.".into());
            }
            called = true;
            let captured = prepared.validate(&admission)?;
            if captured != armed.owner || admission.admission_epoch != armed.admission_epoch {
                return Err(
                    "The committed hardware owner or launch admission epoch changed.".into(),
                );
            }
            let (installation, identity) = self.launch_installation(&prepared.identity.game_id)?;
            if installation != prepared.installation || identity != prepared.identity {
                return Err(
                    "The native game installation changed after launch preparation.".into(),
                );
            }
            owner = Some(captured);
            epoch = admission.admission_epoch;
            // An error from spawning is potentially post-effect. Never retry.
            attempted = true;
            let result = self.inner.platform.spawn_process(&installation);
            spawn_result = Some(result.clone());
            result
        });
        let result = result.and_then(|_| {
            if repeated {
                return Err("A launch admission attempted to spawn twice.".into());
            }
            spawn_result
                .unwrap_or_else(|| Err("Native launch admission did not spawn a process.".into()))
        });
        if let Err(message) = result {
            return Err(LaunchFailure {
                message,
                cleanup_owner: owner,
                process_may_have_started: attempted,
            });
        }
        Ok(SpawnedLaunch {
            identity: prepared.identity,
            installation: prepared.installation,
            owner: owner.unwrap(),
            admission_epoch: epoch,
            spawned_at: Instant::now(),
        })
    }
    pub(crate) fn wait_profile_launch(
        &self,
        spawned: SpawnedLaunch,
        still_current: impl FnMut(&RunOwner, u64) -> Result<(), String>,
    ) -> Result<StartedGame, LaunchFailure> {
        self.wait_profile_launch_with(spawned, still_current, STARTUP_TIMEOUT, std::thread::sleep)
    }
    fn wait_profile_launch_with(
        &self,
        spawned: SpawnedLaunch,
        mut still_current: impl FnMut(&RunOwner, u64) -> Result<(), String>,
        timeout: Duration,
        mut pause: impl FnMut(Duration),
    ) -> Result<StartedGame, LaunchFailure> {
        let result = (|| loop {
            if self.inner.closed.load(std::sync::atomic::Ordering::Acquire) {
                return Err(
                    "Cockpit closed while waiting for the game; no launch was replayed.".into(),
                );
            }
            still_current(&spawned.owner, spawned.admission_epoch)?;
            if spawned.spawned_at.elapsed() >= timeout {
                return Err(
                    "The game did not appear before the startup deadline; no launch was replayed."
                        .into(),
                );
            }
            let (installation, identity) = self.launch_installation(&spawned.identity.game_id)?;
            if installation != spawned.installation || identity != spawned.identity {
                return Err("The native installation changed while waiting for startup.".into());
            }
            let process = self.inner.platform.find(&installation)?;
            let process = match process {
                Some(process) => {
                    ProcessIdentity::parse(&process.value())?;
                    self.inner.platform.alive(&process)?.then_some(process)
                }
                None => None,
            };
            still_current(&spawned.owner, spawned.admission_epoch)?;
            let elapsed = spawned.spawned_at.elapsed();
            if elapsed >= timeout {
                return Err(
                    "The game did not appear before the startup deadline; no launch was replayed."
                        .into(),
                );
            }
            if let Some(process) = process {
                return Ok(process);
            }
            pause(Duration::from_millis(100).min(timeout.saturating_sub(elapsed)));
        })();
        result
            .map(|process| StartedGame {
                identity: spawned.identity,
                owner: spawned.owner.clone(),
                process,
            })
            .map_err(|message| LaunchFailure {
                message,
                cleanup_owner: Some(spawned.owner),
                process_may_have_started: true,
            })
    }
}

#[cfg(test)]
#[path = "profile_game_launch_tests.rs"]
mod tests;
