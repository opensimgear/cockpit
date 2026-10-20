//! One native owner of an immutable, derived profile run. The durable manifest
//! contains recovery receipts, never an executable graph or replayable commands.
use osg_profiles::{
    compiler_models::ExecutionPlan, session::SnapshotSummary, store::LocalAssignment,
    ProfileDocument,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[path = "profile_session_journal.rs"]
mod journal;
use journal::{Manifest, Store, Transition};
#[path = "profile_session_stage.rs"]
mod staging;
pub(crate) use staging::{apply_staged, apply_staged_admitted, apply_staged_admitted_checked, Stage};
#[path = "profile_session_inspection.rs"]
pub(crate) mod inspection;
#[path = "profile_session_topology.rs"]
pub(crate) mod topology;
#[path = "profile_session_launch.rs"]
mod launch;

#[derive(Clone, Debug)]
pub(crate) struct Candidate {
    pub document: ProfileDocument,
    pub assignments: Vec<LocalAssignment>,
    pub read_only: bool,
    /// Native selection only; a browser cannot provide process identity.
    pub game_id: Option<String>,
    /// Native hardware-only launch staging; never accepted from IPC. Application
    /// scope is resolved only after the actual launched process is identified.
    pub launch_preparation:bool,
    /// Native cancellation fence captured before staging, never read from IPC.
    pub admission_epoch: Option<u64>,
    /// A native one-use preparation. It is never serialized or accepted over IPC.
    pub stage: Option<std::sync::Arc<Stage>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RunOwner {
    pub session: String,
    pub generation: u64,
    pub profile_id: String,
    pub profile_revision: u64,
    #[serde(deserialize_with = "required_option")]
    pub game_id: Option<String>,
}

fn required_option<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    Option::deserialize(deserializer)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum BlockerKind {
    Structural,
    Storage,
    RequiredResource,
    Backend,
    Layout,
    VisibilityScope,
    CompanionRevision,
    RouteConflict,
    GameSetup,
    Script,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AdmissionBlocker {
    pub kind: BlockerKind,
    pub message: String,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Admission {
    pub blockers: Vec<AdmissionBlocker>,
    /// Preparation may acquire these providers, but may not manufacture samples.
    pub waiting_baselines: Vec<String>,
    pub dormant_branches: Vec<String>,
    pub optional_unavailable: Vec<String>,
}
#[derive(Clone, Debug)]
pub(crate) struct Prepared {
    pub plan: ExecutionPlan,
    pub admission: Admission,
    pub program_hash: String,
    pub capture_fingerprint: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ApplyPhase {
    Overrides,
    Acquire,
    Baseline,
    Visibility,
    Start,
}
pub(crate) const APPLY_PHASES: [ApplyPhase; 5] = [
    ApplyPhase::Overrides,
    ApplyPhase::Acquire,
    ApplyPhase::Baseline,
    ApplyPhase::Visibility,
    ApplyPhase::Start,
];

/// A bounded native capture or reference to an existing subsystem journal. Each
/// adapter validates its exact receipt schema and compares current ownership
/// before restore. Unknown/ambiguous state is preserved and reported.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Receipt {
    pub kind: String,
    pub resource_id: String,
    pub state: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OwnedRun {
    pub owner: RunOwner,
    pub snapshot: SnapshotSummary,
    pub program_hash: String,
    pub capture_fingerprint: String,
    pub receipts: Vec<Receipt>,
    /// An attempted phase may have changed hardware despite an uncertain reply.
    pub attempted: Vec<ApplyPhase>,
}
/// Volatile native provenance for one exact outgoing worker. Never serialized
/// into the recovery manifest or accepted from an IPC request.
#[derive(Clone)]
pub(crate) struct HandoffSource {
    pub run: OwnedRun,
    pub admission_epoch: u64,
    pub output: Option<osg_virtual_controller::session::RunningOutput>,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Health {
    pub degraded: Vec<String>,
    /// A revoked owner/context stops all owned work. Per-source loss/fallback is
    /// handled inside the shared reducer and reported through degraded instead.
    pub stop: Option<String>,
}

/// Native adapters reuse the existing Games/virtual/HidHide/companion owners.
/// Prepare/capture/validate are read-only. enter is called only after its intent
/// is durable. All worker delivery must also validate this owner, not just route
/// freshness. restore(recovery=true) may neutralize/unhide/restore still-owned
/// setup, but must never rehide or replay input, commands, scripts or macros.
pub(crate) trait Host: Send {
    fn set_cleanup_progress(&mut self, _: Option<u64>) {}
    fn activation_progress(&self, _: Option<u64>, _: &str, _: &str) {}
    fn desktop_status(&self) -> Value { Value::Null }
    fn output_handoff(&mut self, _: &OwnedRun, _: u64) -> Result<Option<osg_virtual_controller::session::RunningOutput>, String> { Ok(None) }
    fn prepare(&mut self, candidate: &Candidate, owner: &RunOwner) -> Result<Prepared, String>;
    fn validate_prepared(&mut self, prepared: &Prepared, owner: &RunOwner) -> Result<(), String>;
    fn capture(
        &mut self,
        prepared: &Prepared,
        owner: &RunOwner,
        previous: Option<&OwnedRun>,
    ) -> Result<Vec<Receipt>, String>;
    fn set_authority(&mut self, owner: Option<&RunOwner>) -> Result<(), String>;
    fn neutralize(&mut self, run: &OwnedRun) -> Result<(), String>;
    /// Explicit Recover may rebuild a finished, unresolved cleanup worker from
    /// the same durable private receipt. This hook cannot acquire or replay any
    /// output; ordinary Stop and failed activation never invoke it.
    fn prepare_recovery(&mut self, _: &OwnedRun) -> Result<(), String> { Ok(()) }
    /// Native publication can renew CAN delivery identities after restoring an
    /// outgoing layout or settling an incoming one. Capture replacement cleanup
    /// receipts and exact preparation pins before the next phase is journaled;
    /// this read-only hook must preserve the authored graph and physical targets.
    fn recapture_phase(&mut self, _: ApplyPhase, _: &mut Prepared, _: &OwnedRun, _: Option<&OwnedRun>) -> Result<Option<Vec<Receipt>>, String> { Ok(None) }
    fn enter(
        &mut self,
        phase: ApplyPhase,
        prepared: &Prepared,
        run: &OwnedRun,
    ) -> Result<(), String>;
    fn restore(&mut self, run: &OwnedRun, recovery: bool) -> Result<(), String>;
    fn health(&mut self, run: &OwnedRun) -> Result<Health, String>;
    fn supports_launch_hold(&self) -> bool { false }
    fn hold_health(&mut self, _: &OwnedRun) -> Result<Health, String> {
        Err("This native adapter cannot renew a pending game launch.".into())
    }
    fn bind_launch_epoch(&mut self, _: &OwnedRun, _: u64) -> Result<(), String> {
        Err("This native adapter cannot bind a pending game launch.".into())
    }
    fn install_launch_context(&mut self, _: &OwnedRun, _: &Candidate) -> Result<Prepared, String> {
        Err("This native adapter cannot install a prepared launch context.".into())
    }
    fn inspect(&self, _: &OwnedRun, _: &inspection::Request) -> Result<inspection::Page, String> {
        Err("This native profile adapter has no applied-port inspector.".into())
    }
    fn inspect_topology(&self, _: &OwnedRun, _: &topology::Request) -> Result<topology::Page, String> {
        Err("This native profile adapter has no applied topology projection.".into())
    }
    fn discard_preparation(&mut self, owner: &RunOwner);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum State {
    Stopped,
    Running,
    Degraded,
    Faulted,
    RecoveryNeeded,
    Transitioning,
    AwaitingGame,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    pub state: State,
    pub applied_snapshot: Option<SnapshotSummary>,
    pub owner: Option<RunOwner>,
    pub degradation: Vec<String>,
    pub fault: Option<String>,
}

pub(crate) struct Coordinator<H: Host> {
    host: H,
    store: Store,
    manifest: Manifest,
    status: Status,
    session: String,
    generation: u64,
    active_candidate: Option<Candidate>,
    launch: Option<launch::Held>,
}

impl<H: Host> Coordinator<H> {
    pub(crate) fn new(host: H, path: Option<PathBuf>) -> Self {
        let (store, manifest, error) = Store::open(path);
        let pending = manifest.pending.is_some() || manifest.active.is_some();
        let status = Status {
            state: if error.is_some() || pending { State::RecoveryNeeded } else { State::Stopped },
            applied_snapshot: None,
            owner: None,
            degradation: vec![],
            fault: error.or_else(|| pending.then(|| "A previous profile run needs explicit recovery. No workers, hiding or commands were restarted.".into())),
        };
        Self {
            host,
            store,
            manifest,
            status,
            session: uuid::Uuid::new_v4().to_string(),
            generation: 0,
            active_candidate: None,
            launch: None,
        }
    }
    /// Upgrade old operator Pause intent to a durable Stop before Automatic can run.
    pub(crate) fn legacy_stop_requested(&self) -> bool {
        self.manifest.legacy_paused
    }
    pub(crate) fn complete_legacy_stop_migration(&mut self) -> Result<(), String> {
        if !self.manifest.legacy_paused { return Ok(()); }
        let mut next = self.manifest.clone();
        next.legacy_paused = false;
        self.persist(next)
    }
    pub(crate) fn status(&self) -> &Status {
        &self.status
    }
    pub(crate) fn check_expected_owner(&self, expected: &Option<RunOwner>) -> Result<(), String> {
        if self.status.owner.as_ref() != expected.as_ref() {
            return Err("The active profile changed. Refresh and confirm the activation again.".into());
        }
        Ok(())
    }
    pub(crate) fn desktop_status(&self) -> Value {
        let mut status=self.host.desktop_status();
        let desktop_pending=self.manifest.active.iter().chain(self.manifest.pending.iter().flat_map(|transition|transition.incoming.iter().chain(transition.outgoing.iter())))
            .any(|run|run.receipts.iter().any(|receipt|receipt.kind=="desktop-output"));
        if desktop_pending && matches!(self.status.state,State::RecoveryNeeded|State::Faulted) {
            if !status.is_object(){status=serde_json::json!({});}
            status["state"]=serde_json::json!("recovery-needed");status["error"]=serde_json::json!(self.status.fault);
        }
        status
    }
    pub(crate) fn validates(&self, owner: &RunOwner) -> bool {
        matches!(self.status.state, State::Running | State::Degraded)
            && self.status.owner.as_ref() == Some(owner)
    }
    /// Native-only provenance for off-lock visibility preparation. The adapter
    /// must still prove its exact receipt and driver state before takeover.
    pub(crate) fn visibility_handoff_source(&self) -> Result<Option<OwnedRun>, String> {
        self.check_apply()?;
        let Some(run) = self.manifest.active.as_ref() else {
            return Ok(None);
        };
        if !self.validates(&run.owner)
            || self.status.applied_snapshot.as_ref() != Some(&run.snapshot)
        {
            return Err("The outgoing native profile no longer has an exact active owner.".into());
        }
        Ok(Some(run.clone()))
    }
    /// Capture under the coordinator lock before slow read-only preparation.
    /// The host must revalidate this volatile proof against the same active run
    /// at capture, before the outgoing worker is quiesced.
    pub(crate) fn native_handoff_source(&mut self, candidate: &Candidate) -> Result<Option<HandoffSource>, String> {
        self.store.verify()?;
        let Some(run) = self.visibility_handoff_source()? else { return Ok(None); };
        let admission_epoch = candidate.admission_epoch.ok_or("Native handoff requires the candidate cancellation fence.")?;
        let output = if candidate.document.virtual_outputs.is_empty() { None }
            else { self.host.output_handoff(&run, admission_epoch)? };
        Ok(Some(HandoffSource { run, admission_epoch, output }))
    }
    pub(crate) fn apply(&mut self, candidate: Candidate) -> Result<(), String> {
        self.apply_mode(candidate, false)
    }
    fn apply_mode(&mut self, candidate: Candidate, hold: bool) -> Result<(), String> {
        if candidate.launch_preparation!=hold{return Err("Native launch preparation cannot be used as an ordinary active profile.".into())}
        self.check_apply()?;
        if candidate.read_only {
            return Err("Profile storage is read-only; repair it before activation.".into());
        }
        osg_profiles::validate_document(&candidate.document)?;
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or("Profile run generation exhausted.")?;
        let owner = RunOwner {
            session: self.session.clone(),
            generation: self.generation,
            profile_id: candidate.document.id.clone(),
            profile_revision: candidate.document.revision,
            game_id: candidate.game_id.clone(),
        };
        let prepared = match self.host.prepare(&candidate, &owner) {
            Ok(value) => value,
            Err(error) => {
                self.host.discard_preparation(&owner);
                return Err(error);
            }
        };
        let result = self.takeover(candidate, owner.clone(), prepared, hold);
        if result.is_err() {
            self.host.discard_preparation(&owner);
        }
        result
    }
    pub(crate) fn check_apply(&self) -> Result<(), String> {
        if self.launch.is_some() || self.manifest.pending.is_some() {
            return Err("Deactivate or recover the pending profile transition before another activation.".into());
        }
        if self.status.state == State::RecoveryNeeded {
            return Err(
                "Recover the interrupted profile session before activating another profile.".into(),
            );
        }
        Ok(())
    }
    fn takeover(
        &mut self,
        candidate: Candidate,
        owner: RunOwner,
        mut prepared: Prepared,
        hold: bool,
    ) -> Result<(), String> {
        if !prepared.admission.blockers.is_empty() {
            return Err(prepared
                .admission
                .blockers
                .iter()
                .map(|blocker| blocker.message.as_str())
                .collect::<Vec<_>>()
                .join("; "));
        }
        prepared.plan.program.compile()?;
        self.host.validate_prepared(&prepared, &owner)?;
        let previous = self.manifest.active.clone();
        let receipts = self.host.capture(&prepared, &owner, previous.as_ref())?;
        let snapshot = osg_profiles::session::AppliedSnapshot::capture(&candidate.document)?
            .summary()
            .clone();
        let incoming = OwnedRun {
            owner: owner.clone(),
            snapshot,
            program_hash: prepared.program_hash.clone(),
            capture_fingerprint: prepared.capture_fingerprint.clone(),
            receipts,
            attempted: vec![],
        };
        let mut next = self.manifest.clone();
        next.pending = Some(Transition {
            incoming: Some(incoming),
            outgoing: previous,
        });
        // Failure to record the first intent has not revoked the prior run.
        self.persist(next)?;
        self.status.state = State::Transitioning;
        self.status.owner = None;
        let effects = (|| {
            self.host.set_authority(None)?;
            if let Some(previous) = self.manifest.pending.as_ref().unwrap().outgoing.as_ref() {
                self.host.activation_progress(candidate.admission_epoch,"releasing-previous-profile","Releasing previous profile bindings and outputs");
                self.host.neutralize(previous)?;
            }
            for phase in APPLY_PHASES {
                if hold && phase == ApplyPhase::Start { break; }
                let incoming=self.manifest.pending.as_ref().unwrap().incoming.as_ref().unwrap().clone();
                let outgoing=self.manifest.pending.as_ref().unwrap().outgoing.clone();
                let refreshed=self.host.recapture_phase(phase,&mut prepared,&incoming,outgoing.as_ref())?;
                if phase==ApplyPhase::Overrides { self.host.validate_prepared(&prepared,&owner)?; }
                let mut next = self.manifest.clone();
                if let Some(receipts)=refreshed {
                    let incoming=next.pending.as_mut().unwrap().incoming.as_mut().unwrap();
                    incoming.receipts=receipts;
                    incoming.capture_fingerprint=prepared.capture_fingerprint.clone();
                }
                next.pending
                    .as_mut()
                    .unwrap()
                    .incoming
                    .as_mut()
                    .unwrap()
                    .attempted
                    .push(phase);
                let journal_started = std::time::Instant::now();
                self.persist(next)?;
                let journal_elapsed = journal_started.elapsed();
                let run = self
                    .manifest
                    .pending
                    .as_ref()
                    .unwrap()
                    .incoming
                    .as_ref()
                    .unwrap();
                let authority_started = std::time::Instant::now();
                if phase == ApplyPhase::Start {
                    self.host.set_authority(Some(&owner))?;
                }
                let authority_elapsed = authority_started.elapsed();
                let entry_started = std::time::Instant::now();
                let result = self.host.enter(phase, &prepared, run);
                if cfg!(debug_assertions) && option_env!("OSG_DEV_NATIVE_PROFILES") == Some("1") {
                    eprintln!("Native profile {phase:?}: journal={}ms authority={}ms entry={}ms ok={}",
                        journal_elapsed.as_millis(), authority_elapsed.as_millis(),
                        entry_started.elapsed().as_millis(), result.is_ok());
                }
                result?;
            }
            Ok::<_, String>(())
        })();
        if let Err(error) = effects {
            return self.fail_transition(error);
        }
        if hold {
            let run = self.manifest.pending.as_ref().unwrap().incoming.as_ref().unwrap();
            self.status = Status { state: State::AwaitingGame,
                applied_snapshot: Some(run.snapshot.clone()), owner: Some(owner.clone()),
                degradation: prepared.admission.optional_unavailable.clone(), fault: None };
            self.launch = Some(launch::Held::new(candidate, prepared, owner));
            return Ok(());
        }
        self.complete_transition(candidate, owner, prepared)
    }
    fn complete_transition(&mut self, candidate: Candidate, owner: RunOwner, prepared: Prepared) -> Result<(), String> {
        let run = self
            .manifest
            .pending
            .as_ref()
            .unwrap()
            .incoming
            .clone()
            .unwrap();
        let mut next = self.manifest.clone();
        next.active = Some(run.clone());
        let retired = next.pending.as_ref().and_then(|transition| transition.outgoing.as_ref()).map(|run| run.owner.clone());
        next.pending = None;
        if let Err(error) = self.persist(next) {
            return self.fail_transition(error);
        }
        // Durable ownership has moved. Keep the outgoing adapters through all
        // compensation paths, then release their stopped readers and buffers.
        if let Some(retired) = retired { self.host.discard_preparation(&retired); }
        self.active_candidate = Some(candidate);
        self.status = Status {
            state: if prepared.admission.optional_unavailable.is_empty() {
                State::Running
            } else {
                State::Degraded
            },
            applied_snapshot: Some(run.snapshot.clone()),
            owner: Some(owner),
            degradation: prepared.admission.optional_unavailable,
            fault: None,
        };
        Ok(())
    }
    fn persist(&mut self, next: Manifest) -> Result<(), String> {
        self.store.save(&next)?;
        self.manifest = next;
        Ok(())
    }
    fn fail_transition(&mut self, error: String) -> Result<(), String> {
        self.launch = None;
        let _ = self.host.set_authority(None);
        self.status.owner = None;
        let neutralize = self.neutralize_owned();
        let result = self.compensate(false);
        self.status.state = State::RecoveryNeeded;
        self.status.fault = Some(match neutralize.and(result) { Ok(()) => format!("Profile takeover failed: {error}. Owned effects were compensated; explicitly recover before activating with fresh baselines."), Err(restore) => format!("Profile takeover failed: {error}. Recovery remains necessary: {restore}") });
        Err(self.status.fault.clone().unwrap())
    }
    fn compensate(&mut self, recovery: bool) -> Result<(), String> {
        self.store.verify()?;
        let transition = self.manifest.pending.clone();
        if let Some(transition) = transition {
            if let Some(incoming) = &transition.incoming {
                self.host.neutralize(incoming)?;
                self.host.restore(incoming, recovery)?;
            }
            if let Some(outgoing) = &transition.outgoing {
                self.host.neutralize(outgoing)?;
                self.host.restore(outgoing, true)?;
            }
        } else if let Some(active) = &self.manifest.active {
            self.host.neutralize(active)?;
            self.host.restore(active, recovery)?;
        }
        let mut next = self.manifest.clone();
        next.active = None;
        next.pending = None;
        self.persist(next)?;
        self.active_candidate = None;
        self.status.applied_snapshot = None;
        Ok(())
    }
    pub(crate) fn cleanup_profile_id(&self) -> Option<&str> {
        self.manifest.pending.as_ref().and_then(|pending|pending.incoming.as_ref().or(pending.outgoing.as_ref()))
            .or(self.manifest.active.as_ref()).map(|run|run.owner.profile_id.as_str())
    }
    pub(crate) fn stop_with_progress(&mut self,epoch:u64) -> Result<(),String> {
        self.host.set_cleanup_progress(Some(epoch));
        self.host.activation_progress(Some(epoch),"cleanup-revoking-authority","Releasing profile output authority");
        let result=self.stop();
        self.host.set_cleanup_progress(None);
        result
    }
    pub(crate) fn recover_with_progress(&mut self,epoch:u64) -> Result<(),String> {
        self.host.set_cleanup_progress(Some(epoch));
        self.host.activation_progress(Some(epoch),"cleanup-validating-journal","Checking recorded output ownership for recovery");
        let result=self.recover();
        self.host.set_cleanup_progress(None);
        result
    }
    pub(crate) fn stop(&mut self) -> Result<(), String> {
        self.launch = None;
        self.status.owner = None;
        let revoke = self.host.set_authority(None);
        // Emergency neutralization uses trusted in-memory owner fences even if
        // storage broke. Restoration still requires an intact owned journal.
        let neutralize = self.neutralize_owned();
        let restore = self.compensate(false);
        match revoke.and(neutralize).and(restore) {
            Ok(()) => {
                self.status.state = State::Stopped;
                self.status.fault = None;
                self.status.degradation.clear();
                Ok(())
            }
            Err(error) => {
                self.status.state = State::RecoveryNeeded;
                self.status.fault = Some(error.clone());
                Err(error)
            }
        }
    }
    fn neutralize_owned(&mut self) -> Result<(), String> {
        let mut runs = Vec::new();
        if let Some(pending) = &self.manifest.pending {
            runs.extend(pending.incoming.iter().cloned());
            runs.extend(pending.outgoing.iter().cloned());
        } else {
            runs.extend(self.manifest.active.iter().cloned());
        }
        let mut errors = Vec::new();
        for run in runs {
            if let Err(error) = self.host.neutralize(&run) {
                errors.push(error);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
    pub(crate) fn recover(&mut self) -> Result<(), String> {
        self.launch = None;
        self.status.owner = None;
        let revoke = self.host.set_authority(None);
        let retry = self.store.verify().and_then(|_| {
            let mut runs = Vec::new();
            if let Some(pending) = &self.manifest.pending {
                runs.extend(pending.incoming.iter().cloned());
                runs.extend(pending.outgoing.iter().cloned());
            } else {
                runs.extend(self.manifest.active.iter().cloned());
            }
            for run in runs {
                self.host.prepare_recovery(&run)?;
            }
            Ok(())
        });
        let neutralize = self.neutralize_owned();
        let restore = self.compensate(true);
        revoke.and(retry).and(neutralize).and(restore)?;
        self.status.state = State::Stopped;
        self.status.fault = None;
        self.status.degradation.clear();
        Ok(())
    }
    pub(crate) fn poll(&mut self) -> Result<(), String> {
        if self.launch.is_some() { return self.poll_launch(); }
        let Some(active) = self
            .manifest
            .active
            .clone()
            .filter(|_| self.status.owner.is_some())
        else {
            return Ok(());
        };
        match self.host.health(&active) {
            Ok(health) if health.stop.is_none() => {
                self.status.degradation = health.degraded;
                self.status.state = if self.status.degradation.is_empty() {
                    State::Running
                } else {
                    State::Degraded
                };
                Ok(())
            }
            health => {
                let reason = health.map_or_else(|error| error, |health| health.stop.unwrap());
                self.stop()?;
                self.status.state = State::Faulted;
                self.status.fault = Some(reason);
                Ok(())
            }
        }
    }
    pub(crate) fn guard_request(&self, command: &str) -> Result<(), String> {
        if self.manifest.active.is_none()
            && self.manifest.pending.is_none()
            && self.status.state != State::RecoveryNeeded
        {
            return Ok(());
        }
        if command.starts_with("profile_")
            || matches!(
                command,
                "mapping_stop" | "integration_stop" | "dcs_stop" | "controller_visibility_restore"
            )
        {
            return Ok(());
        }
        if command.starts_with("mapping_")
            || command.starts_with("companion_")
            || command.starts_with("integration_")
            || command.starts_with("dcs_")
            || command.starts_with("game_")
            || matches!(
                command,
                "config_apply"
                    | "config_save"
                    | "config_discard"
                    | "config_reset"
                    | "claim"
                    | "action_invoke"
                    | "controller_visibility_hide"
            )
        {
            return Err("The active profile owns this runtime state. Deactivate or recover it before making independent changes.".into());
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "profile_session_tests.rs"]
mod tests;

#[cfg(test)]
impl<H: Host> Coordinator<H> {
    pub(crate) fn fail_journal_saves(&mut self, fail: bool) {
        self.store.fail_save = fail;
    }
    pub(crate) fn fail_save_when_attempting(&mut self, phase: ApplyPhase) {
        self.store.fail_save_phase = Some(phase);
    }
}
