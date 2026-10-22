//! Native automatic selection uses the same one-use staging and coordinator as
//! explicit Apply. Process sessions are reverified after slow preparation.
use super::*;
#[path = "profile_game_orchestration.rs"]
mod launch;
use crate::profile_automatic_store::{Settings, Store};
use osg_profiles::{
    activation::{Decision, Locks, Reconciler, Rule, Target, VerifiedGame},
    session::SelectionMode,
};
use std::{path::PathBuf, sync::atomic::AtomicBool};

struct State {
    settings: Settings,
    read_only: bool,
    epoch: u64,
    reconciler: Reconciler,
    status: &'static str,
    message: String,
    conflicts: Vec<String>,
    target: Option<Target>,
    last_attempt: Option<Target>,
    last_vehicle_attempt: Option<(crate::profile_session::RunOwner,String,std::time::Instant)>,
    last_peripheral_attempt: Option<(crate::profile_session::RunOwner,String,std::time::Instant)>,
}

pub(super) struct Runtime {
    store: Store,
    state: Mutex<State>,
    // Emergency inhibition never waits for a disk write or policy-state mutex.
    emergency: AtomicBool,
    pending_controls: AtomicUsize,
}
pub(super) struct ControlRequest<'a>(&'a AtomicUsize);
impl Drop for ControlRequest<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
impl Runtime {
    pub(super) fn new(path: Option<PathBuf>) -> Self {
        let store = Store::new(path);
        let (settings, error) = match store.load() {
            Ok(settings) => (settings.unwrap_or_default(), None),
            Err(error) => (Settings::default(), Some(error)),
        };
        Self {
            store,
            state: Mutex::new(State {
                settings,
                read_only: error.is_some(),
                epoch: 0,
                reconciler: Reconciler::default(),
                status: if error.is_some() {
                    "blocked"
                } else {
                    "waiting"
                },
                message: error.unwrap_or_default(),
                conflicts: vec![],
                target: None,
                last_attempt: None,
                last_vehicle_attempt: None,
                last_peripheral_attempt: None,
            }),
            emergency: AtomicBool::new(false),
            pending_controls: AtomicUsize::new(0),
        }
    }
    fn config(&self) -> (Settings, u64, bool) {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        (state.settings.clone(), state.epoch, state.read_only)
    }
    pub(super) fn view(&self) -> Value {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let (status, message) = if self.emergency.load(Ordering::Acquire) {
            ("blocked","Deactivation or a failed profile control prevents automatic activation. Resolve the reported error, then select Automatic explicitly to resume.")
        } else {
            (state.status, state.message.as_str())
        };
        json!({"schemaVersion":1,"mode":state.settings.mode,"manualProfileId":state.settings.manual_profile_id,"rules":state.settings.rules,"status":status,"message":message,"conflicts":state.conflicts,"lastAttempt":state.last_attempt,"target":state.target,"readOnly":state.read_only})
    }
    pub(super) fn settings_snapshot(&self) -> Value {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        json!({"settings":state.settings,"readOnly":state.read_only})
    }
    fn change(&self, edit: impl FnOnce(&mut Settings) -> Result<(), String>) -> Result<(), String> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.read_only {
            return Err(
                "Repair native automatic profile settings before changing selection rules.".into(),
            );
        }
        let mut next = state.settings.clone();
        edit(&mut next)?;
        next.validate()?;
        self.store.save(&next)?;
        state.epoch = state
            .epoch
            .checked_add(1)
            .ok_or("Automatic profile control epoch exhausted.")?;
        state.settings = next;
        state.conflicts.clear();
        state.status = if state.settings.mode == SelectionMode::Manual {
            "manual"
        } else {
            "waiting"
        };
        state.message = if state.settings.mode == SelectionMode::Manual {
            "Activate or deactivate profiles manually. Automatic mode follows your game rules.".into()
        } else {
            "Waiting for verified native game contexts.".into()
        };
        Ok(())
    }
    pub(super) fn control(&self, command: &str) {
        if command == "profile_stop" {
            self.emergency.store(true, Ordering::Release);
        }
    }
    pub(super) fn begin_control(&self, command: &str) -> Option<ControlRequest<'_>> {
        if !matches!(
            command,
            "profile_apply"
                | "profile_local_settings_save"
                | "profile_editor_save"
                | "profile_game_launch"
                | "profile_select"
                | "profile_selection_mode"
                | "profile_game_rule_save"
                | "profile_game_rule_remove"
                | "profile_game_preference_save"
                | "profile_game_preference_remove"
                | "profile_recover"
                | "profile_stop"
        ) {
            return None;
        }
        self.pending_controls.fetch_add(1, Ordering::AcqRel);
        Some(ControlRequest(&self.pending_controls))
    }
    pub(super) fn pending(&self) -> bool {
        self.pending_controls.load(Ordering::Acquire) != 0
    }
    pub(super) fn retry_event(&self) {
        // Cancellation/admission itself uses Manager's atomic epoch. This one
        // only permits an explicit operator event to retry an automatic intent.
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(next) = state.epoch.checked_add(1) {
            state.epoch = next;
        } else {
            state.read_only = true;
            state.status = "blocked";
            state.message = "Automatic profile control epoch exhausted.".into();
        }
    }
    fn note(&self, epoch: u64, decision: &Decision) -> Option<Target> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.epoch != epoch || state.read_only {
            return None;
        }
        match decision {
            Decision::Hold(message) => {
                state.status = if state.settings.mode == SelectionMode::Manual {
                    "manual"
                } else {
                    "blocked"
                };
                state.message = message.clone();
                state.conflicts.clear();
                None
            }
            Decision::Conflict(games) => {
                state.status = "conflict";
                state.message="Equal highest-priority game rules conflict; the active profile is unchanged.".into();
                state.conflicts = games.clone();
                None
            }
            Decision::Select(target) => {
                state.conflicts.clear();
                let next = state.reconciler.next(decision, epoch)?;
                state.target = Some(target.clone());
                state.last_attempt = Some(target.clone());
                state.status = "preparing";
                state.message =
                    "Preparing the saved profile for activation.".into();
                Some(next)
            }
        }
    }
    fn finish(
        &self,
        epoch: u64,
        target: Target,
        result: &Result<(), String>,
        enumeration_changed: bool,
    ) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.epoch != epoch {
            return;
        }
        let restart_notice = result.is_ok() && target.game_id.is_some() && enumeration_changed;
        state.target = Some(target);
        state.status = if result.is_ok() { "active" } else { "failed" };
        state.message=match result{Ok(())=>"Automatic mode activated the saved profile. Process polling does not replay its actions.".into(),Err(error)=>error.clone()};
        if restart_notice {
            state.message.push_str(" This profile changes controller availability; the running game may need a restart to see it.");
        }
    }
}

impl GatewayManager {
    /// Only peripherals captured as absent may request a native rebind. The
    /// running source stays immutable; fresh ownership and baselines use the
    /// same staged handoff as Apply and vehicle changes.
    fn poll_peripheral_profile(&self) {
        if self.closed.load(Ordering::Acquire)||self.profile_pending.load(Ordering::Acquire)!=0||self.profile_automatic.pending(){return;}
        let changed=self.profile_session.lock().unwrap_or_else(|p|p.into_inner()).peripheral_change();
        let Some((owner,fingerprint))=changed else{return;};
        {
            let state=self.profile_automatic.state.lock().unwrap_or_else(|p|p.into_inner());
            if state.last_peripheral_attempt.as_ref().is_some_and(|(previous,change,at)|previous==&owner&&change==&fingerprint&&at.elapsed()<std::time::Duration::from_secs(5)){return;}
        }
        let previous=self.profile_pending.fetch_add(1,Ordering::AcqRel);
        let _pending=PendingProfileApply(&self.profile_pending);
        if previous!=0{return;}
        let captured=(|| {
            let _admission=self.profile_admission.write().unwrap_or_else(|p|p.into_inner());
            if self.closed.load(Ordering::Acquire)||self.profile_pending.load(Ordering::Acquire)!=1||self.profile_automatic.pending(){return Err("Peripheral reconnection was cancelled by a newer native control.".to_owned());}
            let coordinator=self.profile_session.lock().unwrap_or_else(|p|p.into_inner());
            if coordinator.peripheral_change()!=Some((owner.clone(),fingerprint.clone())){return Err("Peripheral readiness changed before reconnection.".into());}
            let epoch=self.profile_admission_epoch.load(Ordering::Acquire);
            coordinator.peripheral_candidate(&owner,epoch)
        })();
        let Ok(candidate)=captured else{return;};
        let epoch=candidate.admission_epoch.unwrap();
        let activation_id=self.start_peripheral_profile_reconnect(&candidate).ok();
        self.profile_automatic.state.lock().unwrap_or_else(|p|p.into_inner()).last_peripheral_attempt=Some((owner.clone(),fingerprint.clone(),std::time::Instant::now()));
        let result=crate::profile_session::apply_staged_admitted_checked(
            &self.profile_session,candidate,|candidate|self.prepare_profile_at(candidate,epoch),
            |_| {
                let admission=self.profile_admission.write().unwrap_or_else(|p|p.into_inner());
                let game=self.game_admission.read().unwrap_or_else(|p|p.into_inner());
                if self.closed.load(Ordering::Acquire)||self.profile_admission_epoch.load(Ordering::Acquire)!=epoch||self.profile_automatic.pending(){return Err("Peripheral reconnection was cancelled by a newer native control.".to_owned());}
                self.guard_retired_recovery()?;
                if self.profile_session.lock().unwrap_or_else(|p|p.into_inner()).peripheral_change()!=Some((owner.clone(),fingerprint.clone())){return Err("Peripheral readiness changed during preparation; no stale device was activated.".into());}
                self.note_profile_activation(epoch,"reconnecting-peripherals","Applying fresh device baselines and reconnecting available profile devices…");
                Ok((admission,game))
            },
            ||{self.profile_admission_epoch.fetch_add(1,Ordering::AcqRel);},Some(&Some(owner.clone())));
        if let Some(id)=activation_id {
            self.finish_profile_activation(&id,&match result {
                Ok(())=>json!({"ok":true}),Err(error)=>json!({"ok":false,"error":error}),
            });
        }
    }
    /// Vehicle selection follows the running profile in both Manual and
    /// Automatic mode. Reuse the same guarded, one-use handoff as a saved edit.
    fn poll_vehicle_profile(&self) {
        if self.closed.load(Ordering::Acquire) || self.profile_pending.load(Ordering::Acquire)!=0 || self.profile_automatic.pending() {return;}
        let changed=self.profile_session.lock().unwrap_or_else(|p|p.into_inner()).vehicle_change();
        let Some((owner,selection))=changed else {return;};
        {
            let state=self.profile_automatic.state.lock().unwrap_or_else(|p|p.into_inner());
            if state.last_vehicle_attempt.as_ref().is_some_and(|(previous,vehicle,at)|previous==&owner&&vehicle==&selection&&at.elapsed()<std::time::Duration::from_secs(5)) {return;}
        }
        let previous=self.profile_pending.fetch_add(1,Ordering::AcqRel);
        let _pending=PendingProfileApply(&self.profile_pending);
        if previous!=0{return;}
        let capture=(|| {
            let _admission=self.profile_admission.write().unwrap_or_else(|p|p.into_inner());
            if self.closed.load(Ordering::Acquire) || self.profile_pending.load(Ordering::Acquire)!=1 || self.profile_automatic.pending(){return Err("Vehicle profile switch was cancelled by a newer native control.".to_owned());}
            let coordinator=self.profile_session.lock().unwrap_or_else(|p|p.into_inner());
            if !coordinator.validates(&owner){return Err("The active profile changed before vehicle switching.".into());}
            let epoch=self.profile_admission_epoch.load(Ordering::Acquire);
            coordinator.vehicle_candidate(&owner,epoch)
        })();
        if let Ok(candidate)=capture {
            let epoch=candidate.admission_epoch.unwrap();
            // Display the immutable running profile variant, even if its saved
            // source has since been edited. Progress must not block a handoff.
            let activation_id=self.start_vehicle_profile_switch(&candidate,&selection).ok();
            self.profile_automatic.state.lock().unwrap_or_else(|p|p.into_inner()).last_vehicle_attempt=Some((owner.clone(),selection,std::time::Instant::now()));
            let result=crate::profile_session::apply_staged_admitted_checked(
                &self.profile_session,candidate,|candidate|self.prepare_profile_at(candidate,epoch),
                |_| {
                    let admission=self.profile_admission.write().unwrap_or_else(|p|p.into_inner());
                    let game=self.game_admission.read().unwrap_or_else(|p|p.into_inner());
                    if self.closed.load(Ordering::Acquire)||self.profile_admission_epoch.load(Ordering::Acquire)!=epoch||self.profile_automatic.pending(){return Err("Vehicle profile preparation was cancelled by a newer native control.".to_owned());}
                    self.guard_retired_recovery()?;
                    self.note_profile_activation(epoch,"switching-vehicle","Applying the current vehicle settings and retaining Base settings…");
                    Ok((admission,game))
                },
                ||{self.profile_admission_epoch.fetch_add(1,Ordering::AcqRel);},Some(&Some(owner.clone())));
            if let Err(error)=&result {
                self.profile_session.lock().unwrap_or_else(|p|p.into_inner()).vehicle_switch_failed(&owner,error);
            }
            if let Some(id)=activation_id {
                self.finish_profile_activation(&id,&match result {
                    Ok(())=>json!({"ok":true}),Err(error)=>json!({"ok":false,"error":error}),
                });
            }
        }
    }
    pub(super) fn restore_automatic_selection(&self) {
        let legacy_stop = self.profile_session.lock().unwrap_or_else(|p| p.into_inner()).legacy_stop_requested();
        if legacy_stop { self.profile_automatic.control("profile_stop"); }
        let (settings, _, read_only) = self.profile_automatic.config();
        if read_only {
            return;
        }
        let mut profiles = self.profiles.lock().unwrap_or_else(|p| p.into_inner());
        let id = if profiles.compile_source(&settings.manual_profile_id).is_ok() {
            settings.manual_profile_id.as_str()
        } else {
            osg_profiles::DEFAULT_PROFILE_ID
        };
        let _ = profiles
            .request(&json!({"command":"profile_select","id":id,"selectionSource":"manual"}));
        let _ = profiles.request(&json!({"command":"profile_selection_mode","mode":settings.mode}));
        drop(profiles);
        // There are no workers to stop at startup. Pin directly so a failed
        // policy write cannot invoke Stop and drop the legacy journal first.
        if legacy_stop && self.automatic_manual_pin(id).is_ok() {
            let _ = self.profiles.lock().unwrap_or_else(|p| p.into_inner())
                .request(&json!({"command":"profile_selection_mode","mode":"manual"}));
            // Keep the old on-disk intent until Manual has committed. A failed
            // pin or journal write is safely retried at the next startup.
            let _ = self.profile_session.lock().unwrap_or_else(|p| p.into_inner()).complete_legacy_stop_migration();
        }
    }
    pub(super) fn automatic_response(&self) -> Result<Value, String> {
        let mut response = self
            .profiles
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .request(&json!({"command":"profile_status"}))?;
        self.decorate_profile_session(&mut response);
        Ok(response)
    }
    pub(super) fn automatic_manual_pin(&self, id: &str) -> Result<(), String> {
        // A malformed optional automation file already blocks every automatic
        // startup. Preserve it while permitting an explicit healthy profile to
        // run manually; its corruption must not disable Default-only operation.
        {
            let mut state = self
                .profile_automatic
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if state.read_only {
                let mut next = state.settings.clone();
                next.mode = SelectionMode::Manual;
                next.manual_profile_id = id.into();
                next.validate()?;
                state.epoch = state
                    .epoch
                    .checked_add(1)
                    .ok_or("Automatic profile control epoch exhausted.")?;
                state.settings = next;
                return Ok(());
            }
        }
        let result = self.profile_automatic.change(|settings| {
            settings.mode = SelectionMode::Manual;
            settings.manual_profile_id = id.into();
            Ok(())
        });
        if result.is_err() {
            self.profile_automatic
                .emergency
                .store(true, Ordering::Release);
        }
        result
    }
    pub(super) fn automatic_stop_pin(&self) -> Result<(), String> {
        // Called only after emergency neutralization, so a slow settings write
        // cannot delay revocation or leave held outputs running.
        let selected = self
            .profiles
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .request(&json!({"command":"profile_status"}))?["session"]["selectedProfileId"]
            .as_str()
            .unwrap_or(osg_profiles::DEFAULT_PROFILE_ID)
            .to_owned();
        if let Err(error) = self.automatic_manual_pin(&selected) {
            let _ = self
                .profile_session
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .stop();
            return Err(format!("Profile deactivated, but Manual mode could not be persisted: {error}. Explicitly repair/recover before Automatic."));
        }
        self.profiles
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .request(&json!({"command":"profile_selection_mode","mode":"manual"}))?;
        Ok(())
    }
    pub(super) fn automatic_request(&self, request: &Value) -> Result<Value, String> {
        let command = request["command"]
            .as_str()
            .ok_or("Choose an automatic profile operation.")?;
        let fields: &[&str] = match command {
            "profile_automatic_status" => &[],
            "profile_selection_mode" => &["mode"],
            "profile_game_rule_save" => &["gameId", "profileId", "priority", "enabled"],
            "profile_game_rule_remove" => &["gameId"],
            _ => return Err("Unknown automatic profile operation.".into()),
        };
        if request
            .as_object()
            .ok_or("Automatic profile control requires an object.")?
            .keys()
            .any(|key| key != "command" && !fields.contains(&key.as_str()))
        {
            return Err("Unknown field in automatic profile request.".into());
        }
        match command {
            "profile_automatic_status" => {}
            "profile_selection_mode" => {
                let mode: SelectionMode = serde_json::from_value(request["mode"].clone())
                    .map_err(|_| "Choose Manual or Automatic mode.")?;
                let selected = self
                    .profiles
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .request(&json!({"command":"profile_status"}))?["session"]["selectedProfileId"]
                    .as_str()
                    .ok_or("The native selected profile identity is missing.")?
                    .to_owned();
                self.profile_automatic.change(|settings| {
                    settings.mode = mode;
                    if mode == SelectionMode::Manual {
                        settings.manual_profile_id = selected;
                    }
                    Ok(())
                })?;
                self.profiles
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .request(request)?;
                if mode == SelectionMode::Automatic {
                    self.profile_automatic
                        .emergency
                        .store(false, Ordering::Release);
                }
            }
            "profile_game_rule_save" => {
                let game = request["gameId"]
                    .as_str()
                    .filter(|game| osg_profiles::activation::valid_game_id(game))
                    .ok_or("Choose a valid game rule.")?;
                let id = request["profileId"]
                    .as_str()
                    .ok_or("Choose a saved unified profile.")?;
                let priority = request["priority"]
                    .as_i64()
                    .and_then(|p| i16::try_from(p).ok())
                    .ok_or("Choose an explicit integer priority from -32768 to 32767.")?;
                let enabled = request["enabled"]
                    .as_bool()
                    .ok_or("Rule enabled must be a boolean.")?;
                let (document, _, read_only) = self
                    .profiles
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .compile_source(id)?;
                if read_only {
                    return Err("Repair profile storage before saving an activation rule.".into());
                }
                if enabled
                    && !document.eligibility.game_ids.is_empty()
                    && !document.eligibility.game_ids.iter().any(|id| id == game)
                {
                    return Err("The saved profile is not eligible for this game.".into());
                }
                let (settings, _, _) = self.profile_automatic.config();
                let identity = match self.games.profile_activation_identity(game) {
                    Ok(identity) => identity,
                    Err(error) if !enabled => settings
                        .rules
                        .iter()
                        .find(|rule| rule.identity.game_id == game)
                        .map(|rule| rule.identity.clone())
                        .ok_or(error)?,
                    Err(error) => return Err(error),
                };
                self.profile_automatic.change(|settings| {
                    settings.rules.retain(|rule| rule.identity.game_id != game);
                    settings.rules.push(Rule {
                        identity,
                        profile_id: id.into(),
                        priority,
                        enabled,
                    });
                    settings
                        .rules
                        .sort_by(|a, b| a.identity.game_id.cmp(&b.identity.game_id));
                    Ok(())
                })?;
            }
            _ => {
                let game = request["gameId"]
                    .as_str()
                    .ok_or("Choose a saved game rule.")?;
                self.profile_automatic.change(|settings| {
                    let before = settings.rules.len();
                    settings.rules.retain(|rule| rule.identity.game_id != game);
                    if settings.rules.len() == before {
                        return Err("No activation rule exists for this game.".into());
                    }
                    Ok(())
                })?;
            }
        }
        self.automatic_response()
    }
    fn automatic_decision(&self) -> Result<(Decision, Vec<VerifiedGame>, u64), String> {
        let (settings, epoch, read_only) = self.profile_automatic.config();
        if read_only {
            return Ok((
                Decision::Hold(
                    "Repair native automatic profile settings before activation.".into(),
                ),
                vec![],
                epoch,
            ));
        }
        let (status, active_game_context) = {
            let session = self.profile_session.lock().unwrap_or_else(|p| p.into_inner());
            (
                session.status().clone(),
                session.active_game_context().map(str::to_owned),
            )
        };
        let locks = Locks {
            emergency_stopped: self.profile_automatic.emergency.load(Ordering::Acquire),
            recovery_needed: status.state == crate::profile_session::State::RecoveryNeeded,
            faulted: status.state == crate::profile_session::State::Faulted,
        };
        if settings.mode == SelectionMode::Manual
            || locks.emergency_stopped
            || locks.recovery_needed
            || locks.faulted
        {
            return Ok((
                osg_profiles::activation::select(settings.mode, locks, &settings.rules, &[], &[])?,
                vec![],
                epoch,
            ));
        }
        let observed = match self.games.profile_activation_observations() {
            Ok(observed) => observed,
            Err(error) => return Ok((Decision::Hold(error), vec![], epoch)),
        };
        let profiles = self.context_documents()?;
        let mut rules = vec![];
        for game in &observed {
            match launch::context_choice(&settings, &game.identity.game_id, &profiles, None) {
                Ok(Some(profile)) => rules.push(Rule {
                    identity: game.identity.clone(),
                    profile_id: profile.id.clone(),
                    priority: settings.rules.iter()
                        .find(|rule| rule.enabled && rule.identity == game.identity && rule.profile_id == profile.id)
                        .map(|rule| rule.priority).unwrap_or(0),
                    enabled: true,
                }),
                Ok(None) => {}
                Err(error) => return Ok((Decision::Hold(error), observed, epoch)),
            }
        }
        if rules.is_empty() {
            return Ok((
                Decision::Hold("Waiting for a running game with a saved profile context; the active hardware profile is retained.".into()),
                observed,
                epoch,
            ));
        }
        let decision = osg_profiles::activation::select(settings.mode, locks, &rules, &observed, &profiles)?;
        if let Decision::Select(target) = &decision {
            if matches!(status.state, crate::profile_session::State::Running | crate::profile_session::State::Degraded)
                && status.owner.as_ref().is_some_and(|owner| owner.profile_id == target.profile_id)
                && active_game_context == target.game_id
            {
                return Ok((
                    Decision::Hold("The matching profile is active; game routes follow the current verified process.".into()),
                    observed,
                    epoch,
                ));
            }
        }
        Ok((decision, observed, epoch))
    }
    pub(super) fn poll_automatic(&self) {
        if self.closed.load(Ordering::Acquire)
            || self.profile_pending.load(Ordering::Acquire) != 0
            || self.profile_automatic.pending()
        {
            return;
        }
        let (decision, observed, epoch) = match self.automatic_decision() {
            Ok(decision) => decision,
            Err(_) => return,
        };
        if !matches!(decision, Decision::Select(_)) {
            self.profile_automatic.note(epoch, &decision);
            return;
        }
        let previous = self.profile_pending.fetch_add(1, Ordering::AcqRel);
        let _pending = PendingProfileApply(&self.profile_pending);
        if previous != 0 {
            return;
        }
        let (admission_epoch, target) = {
            let _admission = self
                .profile_admission
                .write()
                .unwrap_or_else(|p| p.into_inner());
            if self.profile_pending.load(Ordering::Acquire) != 1 || self.profile_automatic.pending()
            {
                return;
            }
            let Some(target) = self.profile_automatic.note(epoch, &decision) else {
                return;
            };
            self.invalidate_learning();
            (self.profile_admission_epoch.load(Ordering::Acquire), target)
        };
        let activation_id = match self.start_native_profile_activation(
            &target.profile_id, admission_epoch, "automatic", target.game_id.as_deref(),
        ) {
            Ok(id) => id,
            Err(error) => {
                self.profile_automatic.finish(epoch, target, &Err(error), false);
                return;
            }
        };
        let mut enumeration_changed = false;
        let result = (|| {
            let (document, assignments, read_only) = self
                .profiles
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .compile_source(&target.profile_id)?;
            self.capture_profile_activation_id(&activation_id, &document, &assignments);
            enumeration_changed =
                crate::games::launch::enumeration_intents(&document).requires_startup_notice();
            let proof = target
                .game_id
                .as_ref()
                .and_then(|id| {
                    observed.iter().find(|game| {
                        &game.identity.game_id == id
                            && Some(&game.process_session) == target.process_session.as_ref()
                    })
                })
                .cloned();
            crate::profile_session::apply_staged_admitted(
                &self.profile_session,
                crate::profile_session::Candidate { launch_preparation: false,
                    document,
                    assignments,
                    read_only,
                    game_id: target.game_id.clone(),
                    admission_epoch: Some(admission_epoch),
                    stage: None,
                },
                |candidate| self.prepare_profile_at(candidate,admission_epoch),
                |candidate| {
                    let admission = self
                        .profile_admission
                        .write()
                        .unwrap_or_else(|p| p.into_inner());
                    let game_admission = self
                        .game_admission
                        .read()
                        .unwrap_or_else(|p| p.into_inner());
                    if self.closed.load(Ordering::Acquire)
                        || self.profile_admission_epoch.load(Ordering::Acquire) != admission_epoch
                        || self.profile_pending.load(Ordering::Acquire) != 1
                        || self.profile_automatic.pending()
                    {
                        return Err("Automatic profile preparation was cancelled by a newer native control.".into());
                    }
                    self.guard_retired_recovery()?;
                    let (current, _, current_epoch) = self.automatic_decision()?;
                    if current_epoch != epoch || current != Decision::Select(target.clone()) {
                        return Err("Automatic profile selection changed during preparation; no stale context was activated.".into());
                    }
                    if let Some(proof) = &proof {
                        self.games.validate_profile_activation(proof)?;
                    }
                    let mut profiles = self.profiles.lock().unwrap_or_else(|p| p.into_inner());
                    let (document, assignments, read_only) =
                        profiles.compile_source(&target.profile_id)?;
                    candidate
                        .stage
                        .as_ref()
                        .ok_or("Automatic native preparation is missing.")?
                        .validate(&crate::profile_session::Candidate { launch_preparation: false,
                            document,
                            assignments,
                            read_only,
                            game_id: candidate.game_id.clone(),
                            admission_epoch: candidate.admission_epoch,
                            stage: None,
                        })?;
                    profiles.request(&json!({"command":"profile_select","id":target.profile_id,"selectionSource":"automatic"}))?;
                    Ok((admission, game_admission))
                },
                || {
                    self.profile_admission_epoch.fetch_add(1, Ordering::AcqRel);
                },
            )
        })();
        self.profile_automatic
            .finish(epoch, target, &result, enumeration_changed);
        self.finish_profile_activation(&activation_id, &match result {
            Ok(()) => json!({"ok":true}),
            Err(error) => json!({"ok":false,"error":error}),
        });
    }
    /// Explicitly install the native background owner for desktop/service hosts.
    /// A Weak reference avoids a permanent manager/worker ownership cycle.
    pub fn start_automatic(self: &Arc<Self>) {
        let mut worker = self
            .profile_automatic_worker
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if worker.is_some() {
            return;
        }
        let weak = Arc::downgrade(self);
        let closed = self.profile_closed.clone();
        *worker = std::thread::Builder::new()
            .name("profile-automatic-selection".into())
            .spawn(move || {
                while !closed.load(Ordering::Acquire) {
                    let Some(manager) = weak.upgrade() else {
                        break;
                    };
                    manager.poll_vehicle_profile();
                    manager.poll_peripheral_profile();
                    manager.poll_automatic();
                    drop(manager);
                    for _ in 0..5 {
                        if closed.load(Ordering::Acquire) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            })
            .ok();
    }
}

#[cfg(test)]
#[path = "profile_automatic_tests.rs"]
mod tests;
