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

#[derive(Clone, Debug)]
pub(crate) struct Candidate {
    pub document: ProfileDocument,
    pub assignments: Vec<LocalAssignment>,
    pub read_only: bool,
    /// Native selection only; a browser cannot provide process identity.
    pub game_id: Option<String>,
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
    fn enter(
        &mut self,
        phase: ApplyPhase,
        prepared: &Prepared,
        run: &OwnedRun,
    ) -> Result<(), String>;
    fn restore(&mut self, run: &OwnedRun, recovery: bool) -> Result<(), String>;
    fn health(&mut self, run: &OwnedRun) -> Result<Health, String>;
    fn discard_preparation(&mut self, owner: &RunOwner);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum State {
    Stopped,
    Running,
    Degraded,
    Paused,
    Faulted,
    RecoveryNeeded,
    Transitioning,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    pub state: State,
    pub paused: bool,
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
}

impl<H: Host> Coordinator<H> {
    pub(crate) fn new(host: H, path: Option<PathBuf>) -> Self {
        let (store, manifest, error) = Store::open(path);
        let pending = manifest.pending.is_some() || manifest.active.is_some();
        let status = Status {
            state: if error.is_some() || pending { State::RecoveryNeeded } else if manifest.paused { State::Paused } else { State::Stopped },
            paused: manifest.paused,
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
        }
    }
    pub(crate) fn status(&self) -> &Status {
        &self.status
    }
    pub(crate) fn validates(&self, owner: &RunOwner) -> bool {
        matches!(self.status.state, State::Running | State::Degraded)
            && self.status.owner.as_ref() == Some(owner)
            && !self.status.paused
    }
    pub(crate) fn apply(&mut self, candidate: Candidate) -> Result<(), String> {
        if self.status.paused {
            return Err(
                "The profile session is paused. Explicitly resume before applying a profile."
                    .into(),
            );
        }
        if self.status.state == State::RecoveryNeeded {
            return Err(
                "Recover the interrupted profile session before applying another profile.".into(),
            );
        }
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
        let result = self.takeover(candidate, owner.clone(), prepared);
        if result.is_err() {
            self.host.discard_preparation(&owner);
        }
        result
    }
    fn takeover(
        &mut self,
        candidate: Candidate,
        owner: RunOwner,
        prepared: Prepared,
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
                self.host.neutralize(previous)?;
            }
            self.host.validate_prepared(&prepared, &owner)?;
            for phase in APPLY_PHASES {
                let mut next = self.manifest.clone();
                next.pending
                    .as_mut()
                    .unwrap()
                    .incoming
                    .as_mut()
                    .unwrap()
                    .attempted
                    .push(phase);
                self.persist(next)?;
                let run = self
                    .manifest
                    .pending
                    .as_ref()
                    .unwrap()
                    .incoming
                    .as_ref()
                    .unwrap();
                if phase == ApplyPhase::Start {
                    self.host.set_authority(Some(&owner))?;
                }
                self.host.enter(phase, &prepared, run)?;
            }
            Ok::<_, String>(())
        })();
        if let Err(error) = effects {
            return self.fail_transition(error);
        }
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
        next.pending = None;
        if let Err(error) = self.persist(next) {
            return self.fail_transition(error);
        }
        self.active_candidate = Some(candidate);
        self.status = Status {
            state: if prepared.admission.optional_unavailable.is_empty() {
                State::Running
            } else {
                State::Degraded
            },
            paused: false,
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
        let _ = self.host.set_authority(None);
        self.status.owner = None;
        let neutralize = self.neutralize_owned();
        let result = self.compensate(false);
        self.status.state = State::RecoveryNeeded;
        self.status.fault = Some(match neutralize.and(result) { Ok(()) => format!("Profile takeover failed: {error}. Owned effects were compensated; explicitly recover before applying with fresh baselines."), Err(restore) => format!("Profile takeover failed: {error}. Recovery remains necessary: {restore}") });
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
    pub(crate) fn stop(&mut self) -> Result<(), String> {
        self.status.owner = None;
        let revoke = self.host.set_authority(None);
        // Emergency neutralization uses trusted in-memory owner fences even if
        // storage broke. Restoration still requires an intact owned journal.
        let neutralize = self.neutralize_owned();
        let restore = self.compensate(false);
        match revoke.and(neutralize).and(restore) {
            Ok(()) => {
                self.status.state = if self.status.paused {
                    State::Paused
                } else {
                    State::Stopped
                };
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
    pub(crate) fn pause(&mut self, paused: bool) -> Result<(), String> {
        // Persist the latch before releasing hiding or stopping workers. Resume
        // never reapplies a graph; a fresh explicit Apply is required.
        if !paused && self.status.state == State::RecoveryNeeded {
            return Err("Recover the profile session before resuming.".into());
        }
        if !paused && !self.status.paused {
            return Ok(());
        }
        let mut next = self.manifest.clone();
        next.paused = paused;
        if let Err(error) = self.persist(next) {
            if paused {
                self.status.paused = true;
                let emergency = self.stop().err();
                self.status.state = State::RecoveryNeeded;
                let message = format!("Pause could not be persisted: {error}. The run was revoked; recovery is required{}", emergency.map(|error|format!(": {error}")).unwrap_or_default());
                self.status.fault = Some(message.clone());
                return Err(message);
            }
            return Err(error);
        }
        self.status.paused = paused;
        if paused {
            self.stop()
        } else {
            self.status.state = State::Stopped;
            Ok(())
        }
    }
    pub(crate) fn recover(&mut self) -> Result<(), String> {
        self.status.owner = None;
        let revoke = self.host.set_authority(None);
        let neutralize = self.neutralize_owned();
        let restore = self.compensate(true);
        revoke.and(neutralize).and(restore)?;
        self.status.state = if self.status.paused {
            State::Paused
        } else {
            State::Stopped
        };
        self.status.fault = None;
        self.status.degradation.clear();
        Ok(())
    }
    pub(crate) fn poll(&mut self) -> Result<(), String> {
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
            return Err("The unified profile session owns this runtime state. Stop or recover it before making independent changes.".into());
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "profile_session_tests.rs"]
mod tests;
