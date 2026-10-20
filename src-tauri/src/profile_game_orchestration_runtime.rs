//! Native-only hardware-first launch. Slow catalogue/process preparation never
//! retains admission or the coordinator; only exact held owners can complete.
use super::*;
use crate::profile_session::{Candidate, RunOwner, State as SessionState, Status};
use std::{
    time::{Duration, Instant},
};

struct Capture {
    settings: Settings,
    policy_epoch: u64,
    policy_read_only: bool,
    admission_epoch: u64,
    requested: GameIdentity,
    intent: LaunchIntent,
    explicit_profile_id: Option<String>,
    candidate: Candidate,
}
fn locks(status: &Status, emergency: bool) -> Locks {
    Locks {
        emergency_stopped: emergency,
        recovery_needed: status.state == SessionState::RecoveryNeeded,
        faulted: status.state == SessionState::Faulted,
    }
}
impl GatewayManager {
    fn launch_saved(
        &self,
        original: &Candidate,
        game_id: Option<String>,
        epoch: u64,
    ) -> Result<Candidate, String> {
        let (document, assignments, read_only) = self
            .profiles
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .compile_source(&original.document.id)?;
        let saved = Candidate { launch_preparation: original.launch_preparation,
            document,
            assignments,
            read_only,
            game_id,
            admission_epoch: Some(epoch),
            stage: None,
        };
        original
            .stage
            .as_ref()
            .ok_or("Native launch preparation is missing.")?
            .validate(&saved)?;
        Ok(saved)
    }
    /// Called under short native admission. The request owns exactly one pending
    /// slot/control guard; another queued control cancels it before effects.
    fn launch_current(&self, capture: &Capture, epoch: u64) -> Result<(), String> {
        self.guard_retired_recovery()?;
        if self.closed.load(Ordering::Acquire)
            || self.profile_admission_epoch.load(Ordering::Acquire) != epoch
            || self.profile_pending.load(Ordering::Acquire) != 1
            || self
                .profile_automatic
                .pending_controls
                .load(Ordering::Acquire)
                != 1
        {
            return Err("The game launch was cancelled by a newer native control.".into());
        }
        let (settings, policy_epoch, read_only) = self.profile_automatic.config();
        if settings != capture.settings
            || policy_epoch != capture.policy_epoch
            || read_only != capture.policy_read_only
        {
            return Err("The saved launch selection policy changed during preparation.".into());
        }
        if self
            .games
            .profile_activation_identity(&capture.requested.game_id)?
            != capture.requested
        {
            return Err(
                "The requested native game installation changed during preparation.".into(),
            );
        }
        if launch_intent(
            &settings,
            &capture.requested,
            &self.context_documents()?,
            capture.explicit_profile_id.as_deref(),
        )? != capture.intent
        {
            return Err(
                "Native launch priority or saved profile selection changed; prepare again.".into(),
            );
        }
        self.launch_saved(&capture.candidate, None, capture.admission_epoch)?;
        Ok(())
    }
    fn launch_held(
        &self,
        capture: &Capture,
        owner: &RunOwner,
        epoch: u64,
    ) -> Result<Status, String> {
        let _admission = self
            .profile_admission
            .read()
            .unwrap_or_else(|p| p.into_inner());
        self.launch_current(capture, epoch)?;
        if self.profile_automatic.emergency.load(Ordering::Acquire) {
            return Err("Deactivation cancelled this pending game launch.".into());
        }
        self.profile_session
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .check_launch_hold(owner, epoch)
    }
    fn launch_decision(
        &self,
        capture: &Capture,
        requested: &VerifiedGame,
    ) -> Result<Decision, String> {
        self.games.validate_profile_activation(requested)?;
        let running = self.games.profile_activation_observations()?;
        if !running.contains(requested) {
            return Err(
                "The exact launched process is no longer confirmed by the native watcher.".into(),
            );
        }
        let documents = self.context_documents()?;
        if launch_intent(&capture.settings, &capture.requested, &documents, capture.explicit_profile_id.as_deref())?
            != capture.intent
        {
            return Err(
                "The confirmed game priority no longer matches the prepared hardware profile."
                    .into(),
            );
        }
        let decision = Decision::Select(Target {
                profile_id: capture.intent.profile_id.clone(),
                game_id: capture.intent.game_id.clone(),
                process_session: capture.intent.game_id.as_ref().map(|_| requested.process_session.clone()),
            });
        match &decision {
            Decision::Select(target)
                if target.profile_id == capture.intent.profile_id
                    && target.game_id == capture.intent.game_id =>
            {
                if let Some(game) = &target.game_id {
                    let selected = running
                        .iter()
                        .find(|item| {
                            &item.identity.game_id == game
                                && Some(&item.process_session) == target.process_session.as_ref()
                        })
                        .ok_or("The selected process session changed during game startup.")?;
                    self.games.validate_profile_activation(selected)?;
                }
                Ok(decision)
            }
            _ => Err("The confirmed native selection cannot finish this hardware launch.".into()),
        }
    }
    fn cancel_profile_launch(&self, owner: Option<&RunOwner>, epoch: u64) -> Result<(), String> {
        let _admission = self
            .profile_admission
            .write()
            .unwrap_or_else(|p| p.into_inner());
        let mut coordinator = self
            .profile_session
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let owns = owner.is_some_and(|owner| coordinator.status().owner.as_ref() == Some(owner));
        // A late error must not inhibit an explicitly admitted newer selection.
        if owns || self.profile_admission_epoch.load(Ordering::Acquire) == epoch {
            self.profile_automatic
                .emergency
                .store(true, Ordering::Release);
        }
        if let Some(owner) = owner {
            coordinator.cancel_launch(owner)?;
        }
        Ok(())
    }
    pub(crate) fn launch_profile_game(
        &self,
        game_id: &str,
        profile_id: Option<&str>,
        remember_choice: bool,
        admission_epoch: u64,
        expected_owner: Option<&Option<RunOwner>>,
    ) -> Result<Value, String> {
        if self.profile_admission_epoch.load(Ordering::Acquire) != admission_epoch {
            return Err("The launch request was cancelled before native preparation.".into());
        }
        {
            let coordinator = self.profile_session.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(expected) = expected_owner { coordinator.check_expected_owner(expected)?; }
            coordinator.check_apply()?;
        }
        let requested = self.games.profile_activation_identity(game_id)?;
        let (settings, _, _) = self.profile_automatic.config();
        let intent = launch_intent(
            &settings,
            &requested,
            &self.context_documents()?,
            profile_id,
        )?;
        if remember_choice {
            let selected = profile_id.ok_or("Choose a matching profile before remembering a launch choice.")?;
            self.profile_automatic.change(|settings| { settings.preferred_profiles.insert(game_id.into(), selected.into()); Ok(()) })?;
        }
        let (settings, policy_epoch, policy_read_only) = self.profile_automatic.config();
        let (document, assignments, read_only) = self
            .profiles
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .compile_source(&intent.profile_id)?;
        if read_only {
            return Err("Repair profile storage before launching a game.".into());
        }
        let activation_id = self.start_native_profile_activation(
            &intent.profile_id, admission_epoch, "game-launch", Some(game_id),
        )?;
        self.capture_profile_activation_id(&activation_id, &document, &assignments);
        let result = (|| {
            let mut candidate = Candidate { launch_preparation: true,
                document,
                assignments,
                read_only,
                game_id: None,
                admission_epoch: Some(admission_epoch),
                stage: None,
            };
            candidate.stage = Some(self.prepare_profile_at(&candidate, admission_epoch)?);
            self.note_profile_activation(admission_epoch, "preparing-game-launch", "Checking the game installation and launch settings…");
            let prepared = self.games.prepare_profile_launch_for_selection(
                game_id,
                &candidate,
                intent.game_id.as_deref(),
            )?;
            let capture = Capture {
                settings,
                policy_epoch,
                policy_read_only,
                admission_epoch,
                requested,
                intent,
                explicit_profile_id: profile_id.map(str::to_owned),
                candidate,
            };
            let mut held_owner = None;
            let mut held_epoch = admission_epoch;
            let mut attempted = false;
            let mut actual_decision = None;
            let mut completed_response = None;
            let result = (|| {
                let armed = {
                    let _admission = self
                        .profile_admission
                        .write()
                        .unwrap_or_else(|p| p.into_inner());
                    let _game = self
                        .game_admission
                        .read()
                        .unwrap_or_else(|p| p.into_inner());
                    self.launch_current(&capture, admission_epoch)?;
                    let mut coordinator = self
                        .profile_session
                        .lock()
                        .unwrap_or_else(|p| p.into_inner());
                    if let Some(expected) = expected_owner { coordinator.check_expected_owner(expected)?; }
                    coordinator.check_apply()?;
                    let previous_stop = self.profile_automatic.emergency.load(Ordering::Acquire);
                    self.profiles.lock().unwrap_or_else(|p| p.into_inner()).request(&json!({"command":"profile_select","id":capture.intent.profile_id,"selectionSource":if capture.settings.mode==SelectionMode::Manual{"manual"}else{"automatic"}}))?;
                    attempted = true;
                    if previous_stop {
                        self.profile_automatic
                            .emergency
                            .store(false, Ordering::Release);
                        if let Err(error) = self.launch_current(&capture, admission_epoch) {
                            self.profile_automatic
                                .emergency
                                .store(true, Ordering::Release);
                            return Err(error);
                        }
                    }
                    let owner = coordinator.begin_launch(capture.candidate.clone())?;
                    held_owner = Some(owner.clone());
                    // Keep the coordinator guard across this atomic fence and bind:
                    // its background health worker cannot see a mixed epoch.
                    held_epoch = self
                        .profile_admission_epoch
                        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                            (current == admission_epoch)
                                .then(|| current.checked_add(1))
                                .flatten()
                        })
                        .map_err(|_| "A newer native control cancelled launch admission.")?
                        .checked_add(1)
                        .ok_or("Native launch admission epoch exhausted.")?;
                    self.rebind_profile_activation_epoch(&activation_id, admission_epoch, held_epoch)?;
                    coordinator.bind_launch_epoch(&owner, held_epoch)?;
                    self.games.arm_profile_launch(
                        prepared,
                        &crate::games::launch::LaunchAdmission {
                            candidate: capture.candidate.clone(),
                            status: coordinator.status().clone(),
                            admission_epoch: held_epoch,
                            locks: locks(
                                coordinator.status(),
                                self.profile_automatic.emergency.load(Ordering::Acquire),
                            ),
                        },
                    )?
                };
                let game_name = self.games.resource_catalogue().into_iter()
                    .find(|game| game["id"] == game_id)
                    .and_then(|game| game["name"].as_str().map(str::to_owned))
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| game_id.to_owned());
                self.note_profile_activation(held_epoch, "launching", &format!("Launching {game_name}…"));
                let spawned = self
                    .games
                    .spawn_profile_launch(armed, |spawn| {
                        let _admission = self
                            .profile_admission
                            .write()
                            .unwrap_or_else(|p| p.into_inner());
                        let _game = self
                            .game_admission
                            .read()
                            .unwrap_or_else(|p| p.into_inner());
                        self.launch_current(&capture, held_epoch)?;
                        let owner = held_owner.as_ref().unwrap();
                        let status = self
                            .profile_session
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .check_launch_hold(owner, held_epoch)?;
                        spawn(crate::games::launch::LaunchAdmission {
                            candidate: capture.candidate.clone(),
                            locks: locks(
                                &status,
                                self.profile_automatic.emergency.load(Ordering::Acquire),
                            ),
                            status,
                            admission_epoch: held_epoch,
                        })
                    })
                    .map_err(|failure| failure.message)?;
                self.note_profile_activation(held_epoch, "launching", &format!("Waiting for {game_name} to start…"));
                let started = self
                    .games
                    .wait_profile_launch(spawned, |owner, epoch| {
                        self.launch_held(&capture, owner, epoch).map(|_| ())
                    })
                    .map_err(|failure| failure.message)?;
                self.note_profile_activation(held_epoch, "launching", &format!("Verifying the {game_name} process…"));
                let deadline = Instant::now() + Duration::from_secs(3);
                let confirmed = loop {
                    self.launch_held(&capture, &started.owner, held_epoch)?;
                    if let Some(confirmed) = self.games.confirm_started_profile_activation(&started)? {
                        break confirmed;
                    }
                    if Instant::now() >= deadline {
                        return Err("The native watcher did not confirm the exact launched process before its deadline.".into());
                    }
                    std::thread::sleep(Duration::from_millis(25));
                };
                let decision = self.launch_decision(&capture, &confirmed)?;
                self.note_profile_activation(held_epoch, "configuring-game-context", &format!("Connecting {game_name} mappings and telemetry…"));
                let mut candidate = self
                    .profile_session
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .launch_candidate(
                        &started.owner,
                        capture.intent.game_id.as_deref(),
                        held_epoch,
                    )?;
                candidate.stage = Some(
                    crate::profile_session_host::NativeProfileHost::stage_launch_context(
                        &self.profile_resources,
                        &self.games,
                        &self.controllers,
                        self.profile_native,
                        &candidate,
                        started,
                    )?,
                );
                {
                    let _admission = self
                        .profile_admission
                        .write()
                        .unwrap_or_else(|p| p.into_inner());
                    let _game = self
                        .game_admission
                        .read()
                        .unwrap_or_else(|p| p.into_inner());
                    self.launch_current(&capture, held_epoch)?;
                    if self.launch_decision(&capture, &confirmed)? != decision {
                        return Err(
                            "The selected native process changed during context preparation.".into(),
                        );
                    }
                    // Pin saved graph/assignments against the final one-use stage.
                    let (document, assignments, read_only) = self
                        .profiles
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .compile_source(&capture.intent.profile_id)?;
                    candidate.stage.as_ref().unwrap().validate(&Candidate { launch_preparation: candidate.launch_preparation,
                        document,
                        assignments,
                        read_only,
                        game_id: candidate.game_id.clone(),
                        admission_epoch: Some(held_epoch),
                        stage: None,
                    })?;
                    if self.profile_automatic.emergency.load(Ordering::Acquire) {
                        return Err("Deactivation cancelled this pending game launch.".into());
                    }
                    let mut coordinator = self
                        .profile_session
                        .lock()
                        .unwrap_or_else(|p| p.into_inner());
                    coordinator.check_launch_hold(held_owner.as_ref().unwrap(), held_epoch)?;
                    coordinator.finish_launch(held_owner.as_ref().unwrap(), candidate)?;
                    // Adopt the real watcher session before the parent releases its
                    // pending guards, so automatic polling cannot reapply this run.
                    self.profile_automatic.note(capture.policy_epoch, &decision);
                    if let Decision::Select(target) = &decision {
                        self.profile_automatic.finish(
                            capture.policy_epoch,
                            target.clone(),
                            &Ok(()),
                            false,
                        );
                    }
                    actual_decision = Some(decision);
                    drop(coordinator);
                    self.launch_current(&capture, held_epoch)?;
                    let mut response = self.automatic_response()?;
                    response["launch"] = json!({"gameId":game_id,"profileId":capture.intent.profile_id,"selectedGameId":capture.intent.game_id,"state":"started"});
                    self.launch_current(&capture, held_epoch)?;
                    completed_response = Some(response);
                }
                Ok(())
            })();
            if let Err(error) = result {
                if attempted {
                    let cleanup = self.cancel_profile_launch(held_owner.as_ref(), held_epoch);
                    if let Some(Decision::Select(target)) = actual_decision {
                        self.profile_automatic.finish(
                            capture.policy_epoch,
                            target,
                            &Err(error.clone()),
                            false,
                        );
                    }
                    if let Err(cleanup) = cleanup {
                        return Err(format!("{error} Cleanup requires attention: {cleanup}"));
                    }
                }
                return Err(error);
            }
            completed_response.ok_or_else(|| "The exact completed launch response is missing.".into())
        })();
        self.finish_profile_activation(&activation_id, &match &result {
            Ok(_) => json!({"ok":true}),
            Err(error) => json!({"ok":false,"error":error}),
        });
        result
    }
}

#[cfg(test)]
#[path = "profile_game_orchestration_runtime_tests.rs"]
mod tests;
