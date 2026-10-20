//! Native capability broker for profile-owned simulator branches. Installation
//! discovery and traffic are never evidence of an applied profile or live game.
use crate::game_discovery::ProcessIdentity;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProfileOwner {
    pub session: String,
    pub profile_id: String,
    pub applied_revision: u64,
    pub run_generation: u64,
    /// The native coordinator selects at most one verified game context. A
    /// manually applied ordinary hardware profile can have no game context.
    pub game_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Observation {
    pub process: Option<ProcessIdentity>,
    pub supported: bool,
    pub setup_ready: bool,
    pub provider_setup_revision: String,
    pub catalogue_revision: Option<String>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ContextToken {
    pub session: String,
    pub game_id: String,
    pub owner: ProfileOwner,
    pub process: ProcessIdentity,
    pub provider_setup_revision: String,
    pub catalogue_revision: String,
}
/// Read-only preparation evidence. It can authorize native source acquisition,
/// never a simulator command or an applied-profile owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GamePreparation {
    pub session: String,
    pub game_id: String,
    pub process: ProcessIdentity,
    pub provider_setup_revision: String,
    pub catalogue_revision: String,
}
impl ContextToken {
    pub(crate) fn matches_preparation(&self, prepared: &GamePreparation) -> bool {
        self.game_id == prepared.game_id
            && self.process == prepared.process
            && self.provider_setup_revision == prepared.provider_setup_revision
            && self.catalogue_revision == prepared.catalogue_revision
    }
}
struct Record {
    observation: Observation,
    token: Option<ContextToken>,
    preparation: Option<GamePreparation>,
}
#[derive(Default)]
pub(crate) struct Broker {
    owner: Option<ProfileOwner>,
    games: BTreeMap<String, Record>,
    observation_epoch: u64,
    observation_commits: BTreeMap<String, u64>,
    observations_exhausted: bool,
}
fn supported(id: &str) -> bool {
    matches!(
        id,
        "dcs" | "assetto-corsa-rally" | "x-plane-11" | "x-plane-12"
    )
}
fn canonical_uuid(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id)
}
fn revision(id: &str) -> bool {
    id.strip_prefix("sha256:").is_some_and(|value| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}
impl Broker {
    /// Reserve before observing settings, catalogue or processes. Slow native
    /// I/O must not publish an older snapshot over a newer completed capture.
    pub(crate) fn begin_observation(&mut self) -> Result<u64, String> {
        if !self.observations_exhausted {
            if let Some(next) = self.observation_epoch.checked_add(1) {
                self.observation_epoch = next;
                return Ok(next);
            }
        }
        self.observations_exhausted = true;
        for record in self.games.values_mut() {
            record.token = None;
            record.preparation = None;
            record.observation.process = None;
            record.observation.catalogue_revision = None;
            record.observation.error = Some("Native game observation counter exhausted.".into());
        }
        Err("Native game observation counter exhausted.".into())
    }
    /// Called only by the native applied-profile coordinator, never from IPC.
    pub(crate) fn set_owner(&mut self, owner: Option<ProfileOwner>) -> Result<(), String> {
        if owner.as_ref().is_some_and(|owner| {
            !canonical_uuid(&owner.session)
                || !canonical_uuid(&owner.profile_id)
                || owner.applied_revision == 0
                || owner.run_generation == 0
                || owner
                    .game_id
                    .as_deref()
                    .is_some_and(|game| !supported(game))
        }) {
            return Err("Invalid applied profile game owner.".into());
        }
        if self.owner != owner {
            self.owner = owner;
            for (id, record) in &mut self.games {
                record.token = Self::mint(self.owner.as_ref(), id, &record.observation);
            }
        }
        Ok(())
    }
    pub(crate) fn observe(
        &mut self,
        id: &str,
        epoch: u64,
        observation: Observation,
    ) -> Result<(), String> {
        if !supported(id)
            || self.observations_exhausted
            || epoch == 0
            || epoch > self.observation_epoch
        {
            return Err("Invalid native game observation capture.".into());
        }
        if self
            .observation_commits
            .get(id)
            .is_some_and(|last| epoch <= *last)
        {
            return Ok(());
        }
        self.observation_commits.insert(id.into(), epoch);
        if !revision(&observation.provider_setup_revision)
            || observation
                .catalogue_revision
                .as_deref()
                .is_some_and(|value| !revision(value))
            || observation
                .process
                .as_ref()
                .is_some_and(|process| ProcessIdentity::parse(&process.value()).is_err())
        {
            if let Some(record) = self.games.get_mut(id) {
                record.token = None;
                record.preparation = None;
                record.observation.process = None;
                record.observation.catalogue_revision = None;
                record.observation.error=Some("Native game observation could not be validated. Reacquire verified identity and provider revisions.".into());
            }
            return Err("Invalid verified game observation.".into());
        }
        if self
            .games
            .get(id)
            .is_some_and(|record| record.observation == observation)
        {
            return Ok(());
        }
        let token = Self::mint(self.owner.as_ref(), id, &observation);
        let preparation =
            if observation.supported && observation.setup_ready && observation.error.is_none() {
                observation
                    .process
                    .as_ref()
                    .zip(observation.catalogue_revision.as_ref())
                    .map(|(process, catalogue)| GamePreparation {
                        session: uuid::Uuid::new_v4().to_string(),
                        game_id: id.into(),
                        process: process.clone(),
                        provider_setup_revision: observation.provider_setup_revision.clone(),
                        catalogue_revision: catalogue.clone(),
                    })
            } else {
                None
            };
        self.games.insert(
            id.into(),
            Record {
                observation,
                token,
                preparation,
            },
        );
        Ok(())
    }
    pub(crate) fn preparation(&self, id: &str) -> Result<Option<GamePreparation>, String> {
        if !supported(id) {
            return Err("Choose a supported game context.".into());
        }
        Ok(self
            .games
            .get(id)
            .and_then(|record| record.preparation.clone()))
    }
    pub(crate) fn validate_preparation(&self, proof: &GamePreparation) -> Result<(), String> {
        if self.preparation(&proof.game_id)?.as_ref() == Some(proof) {
            Ok(())
        } else {
            Err("The verified game preparation changed; acquire a fresh process/setup/catalogue proof.".into())
        }
    }
    pub(crate) fn invalidate(&mut self, id: &str, reason: String) {
        // Direct validation already proved this current capture invalid. Fence
        // every observation that was in flight before that validation failed.
        if let Ok(epoch) = self.begin_observation() {
            self.observation_commits.insert(id.into(), epoch);
        }
        if let Some(record) = self.games.get_mut(id) {
            record.token = None;
            record.preparation = None;
            record.observation.process = None;
            record.observation.error = Some(reason);
        }
    }
    /// A late reader/preflight failure only retires its own still-current
    /// proof. A replacement observation or owner must retain its capability.
    pub(crate) fn invalidate_preparation(
        &mut self,
        proof: &GamePreparation,
        reason: String,
    ) -> bool {
        if self
            .games
            .get(&proof.game_id)
            .and_then(|record| record.preparation.as_ref())
            != Some(proof)
        {
            return false;
        }
        self.invalidate(&proof.game_id, reason);
        true
    }
    fn mint(
        owner: Option<&ProfileOwner>,
        id: &str,
        observed: &Observation,
    ) -> Option<ContextToken> {
        let owner = owner.filter(|owner| owner.game_id.as_deref() == Some(id))?;
        if !observed.supported || !observed.setup_ready || observed.error.is_some() {
            return None;
        }
        Some(ContextToken {
            session: uuid::Uuid::new_v4().to_string(),
            game_id: id.into(),
            owner: owner.clone(),
            process: observed.process.clone()?,
            provider_setup_revision: observed.provider_setup_revision.clone(),
            catalogue_revision: observed.catalogue_revision.clone()?,
        })
    }
    pub(crate) fn token(&self, id: &str) -> Result<ContextToken, String> {
        self.games.get(id).and_then(|record|record.token.clone()).ok_or_else(||"This integration is dormant until an applied profile owns its matching verified game process and current provider setup.".into())
    }
    pub(crate) fn validate(&self, token: &ContextToken) -> Result<(), String> {
        if self.token(&token.game_id)?.eq(token) {
            Ok(())
        } else {
            Err("The applied profile, game process, setup or catalogue context changed. Acquire a fresh native context before using this branch.".into())
        }
    }
    pub(crate) fn view(&self, id: &str) -> Value {
        let Some(record) = self.games.get(id) else {
            return json!({"status":"waiting-game","message":"Waiting for native game process inspection.","processVerified":false,"contextSession":null,"providerSetupRevision":null,"catalogueRevision":null});
        };
        let observation = &record.observation;
        let (status, message) = if !observation.supported {
            (
                "unsupported-platform",
                "Native process identity verification is unavailable on this platform.",
            )
        } else if observation.error.is_some() {
            ("error", observation.error.as_deref().unwrap())
        } else if !observation.setup_ready || observation.catalogue_revision.is_none() {
            ("setup-required","Complete this game's provider setup and catalogue selection before using its integration.")
        } else if self
            .owner
            .as_ref()
            .is_none_or(|owner| owner.game_id.as_deref() != Some(id))
        {
            ("waiting-profile","Waiting for an applied profile with this matching game context. Opening or saving an editor does not activate integrations.")
        } else if observation.process.is_none() {
            (
                "waiting-game",
                "Waiting for the matching verified game process.",
            )
        } else {
            ("ready","The applied profile owns the matching verified game process and current provider revisions.")
        };
        json!({"status":status,"message":message,"processVerified":observation.process.is_some(),"contextSession":record.token.as_ref().map(|token|&token.session),"providerSetupRevision":observation.provider_setup_revision,"catalogueRevision":observation.catalogue_revision})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn owner(game: Option<&str>) -> ProfileOwner {
        ProfileOwner {
            session: "96d54c8b-a2a4-433c-be95-eb0b91ee22d3".into(),
            profile_id: "e37f3e1f-ae05-4f26-a506-040be2849309".into(),
            applied_revision: 1,
            run_generation: 1,
            game_id: game.map(str::to_owned),
        }
    }
    fn observed() -> Observation {
        Observation {
            process: Some(ProcessIdentity {
                pid: 42,
                path: "/games/DCS.exe".into(),
                epoch: 10,
            }),
            supported: true,
            setup_ready: true,
            provider_setup_revision: format!("sha256:{}", "a".repeat(64)),
            catalogue_revision: Some(format!("sha256:{}", "b".repeat(64))),
            error: None,
        }
    }
    fn observe(broker: &mut Broker, id: &str, observation: Observation) -> Result<(), String> {
        let epoch = broker.begin_observation()?;
        broker.observe(id, epoch, observation)
    }
    #[test]
    fn older_observations_cannot_replace_a_new_preparation_or_applied_context() {
        let mut broker = Broker::default();
        let old = broker.begin_observation().unwrap();
        let mut old_snapshot = observed();
        old_snapshot.setup_ready = false;
        observe(&mut broker, "dcs", observed()).unwrap();
        let prepared = broker.preparation("dcs").unwrap().unwrap();
        broker.set_owner(Some(owner(Some("dcs")))).unwrap();
        let token = broker.token("dcs").unwrap();
        broker.observe("dcs", old, old_snapshot).unwrap();
        assert_eq!(broker.preparation("dcs").unwrap(), Some(prepared.clone()));
        assert_eq!(broker.token("dcs").unwrap(), token);
        let mut malformed = observed();
        malformed.provider_setup_revision = "stale invalid data".into();
        broker.observe("dcs", old, malformed).unwrap();
        assert!(broker.validate_preparation(&prepared).is_ok());
        assert!(broker.validate(&token).is_ok());

        // Ordering is per game: a slow capture of another game still commits.
        broker.observe("x-plane-12", old, observed()).unwrap();
        assert!(broker.preparation("x-plane-12").unwrap().is_some());
        assert!(broker.token("x-plane-12").is_err());
    }
    #[test]
    fn explicit_current_invalidation_fences_inflight_results_but_allows_fresh_evidence() {
        let mut broker = Broker::default();
        observe(&mut broker, "dcs", observed()).unwrap();
        let prepared = broker.preparation("dcs").unwrap().unwrap();
        broker.set_owner(Some(owner(Some("dcs")))).unwrap();
        let token = broker.token("dcs").unwrap();
        let inflight = broker.begin_observation().unwrap();
        assert!(broker.invalidate_preparation(&prepared, "Current process exited".into()));
        broker.observe("dcs", inflight, observed()).unwrap();
        assert!(broker.preparation("dcs").unwrap().is_none());
        assert!(broker.validate(&token).is_err());
        observe(&mut broker, "dcs", observed()).unwrap();
        assert_ne!(broker.preparation("dcs").unwrap(), Some(prepared));
        assert!(broker.validate(&token).is_err());
        assert!(broker.token("dcs").is_ok());
    }
    #[test]
    fn invalid_or_exhausted_observation_tickets_cannot_grant_or_restore_authority() {
        let mut broker = Broker::default();
        assert!(broker.observe("dcs", 0, observed()).is_err());
        assert!(broker.observe("dcs", 1, observed()).is_err());
        observe(&mut broker, "dcs", observed()).unwrap();
        broker.set_owner(Some(owner(Some("dcs")))).unwrap();
        let token = broker.token("dcs").unwrap();
        broker.observation_epoch = u64::MAX;
        assert!(broker.begin_observation().is_err());
        assert!(broker.preparation("dcs").unwrap().is_none());
        assert!(broker.validate(&token).is_err());
        assert!(broker.observe("dcs", u64::MAX, observed()).is_err());
        assert!(broker.begin_observation().is_err());
        assert_eq!(broker.view("dcs")["status"], "error");
    }
    #[test]
    fn preparation_proves_observation_without_granting_applied_authority() {
        let mut broker = Broker::default();
        observe(&mut broker, "dcs", observed()).unwrap();
        let proof = broker.preparation("dcs").unwrap().unwrap();
        assert!(broker.token("dcs").is_err());
        assert!(broker.validate_preparation(&proof).is_ok());
        broker.set_owner(Some(owner(Some("x-plane-12")))).unwrap();
        assert!(broker.token("dcs").is_err());
        assert!(broker.validate_preparation(&proof).is_ok());
        broker.set_owner(Some(owner(Some("dcs")))).unwrap();
        let token = broker.token("dcs").unwrap();
        assert!(token.matches_preparation(&proof));
        broker.set_owner(None).unwrap();
        assert!(broker.token("dcs").is_err());
        assert!(broker.validate_preparation(&proof).is_ok());
    }
    #[test]
    fn preparation_rejects_forgery_replacement_and_failed_new_evidence() {
        let mut broker = Broker::default();
        observe(&mut broker, "dcs", observed()).unwrap();
        let proof = broker.preparation("dcs").unwrap().unwrap();
        let mut forged = proof.clone();
        forged.session = uuid::Uuid::new_v4().to_string();
        assert!(broker.validate_preparation(&forged).is_err());
        let mut replacement = observed();
        replacement.process.as_mut().unwrap().epoch += 1;
        observe(&mut broker, "dcs", replacement).unwrap();
        assert!(broker.validate_preparation(&proof).is_err());
        let new = broker.preparation("dcs").unwrap().unwrap();
        let mut malformed = observed();
        malformed.provider_setup_revision = "bad".into();
        assert!(observe(&mut broker, "dcs", malformed).is_err());
        assert!(broker.validate_preparation(&new).is_err());
        assert!(broker.preparation("unknown").is_err());
    }
    #[test]
    fn late_preparation_failure_cannot_revoke_replacement_owner_but_current_failure_does() {
        let mut broker = Broker::default();
        observe(&mut broker, "dcs", observed()).unwrap();
        let old = broker.preparation("dcs").unwrap().unwrap();
        let mut replacement = observed();
        replacement.process.as_mut().unwrap().epoch += 1;
        observe(&mut broker, "dcs", replacement).unwrap();
        let current = broker.preparation("dcs").unwrap().unwrap();
        let mut next = owner(Some("dcs"));
        next.run_generation += 1;
        broker.set_owner(Some(next)).unwrap();
        let token = broker.token("dcs").unwrap();
        assert!(!broker.invalidate_preparation(&old, "Old preflight failed".into()));
        assert!(broker.validate(&token).is_ok());
        let mut forged = current.clone();
        forged.provider_setup_revision = format!("sha256:{}", "f".repeat(64));
        assert!(!broker.invalidate_preparation(&forged, "Wrong setup".into()));
        assert!(broker.validate(&token).is_ok());
        assert!(broker.invalidate_preparation(&current, "Current process inspection failed".into()));
        assert!(broker.validate(&token).is_err());
        assert!(broker.preparation("dcs").unwrap().is_none());
    }
    #[test]
    fn closed_wrong_and_unapplied_game_contexts_cannot_mint_capabilities() {
        let mut broker = Broker::default();
        observe(&mut broker, "dcs", observed()).unwrap();
        assert_eq!(broker.view("dcs")["status"], "waiting-profile");
        assert_eq!(broker.view("dcs")["processVerified"], true);
        assert!(broker.token("dcs").is_err());
        broker.set_owner(Some(owner(Some("x-plane-12")))).unwrap();
        assert!(broker.token("dcs").is_err());
        broker.set_owner(Some(owner(None))).unwrap();
        assert!(broker.token("dcs").is_err());
        broker.set_owner(Some(owner(Some("dcs")))).unwrap();
        assert!(broker.token("dcs").is_ok());
        let mut missing = observed();
        missing.process = None;
        observe(&mut broker, "dcs", missing).unwrap();
        assert_eq!(broker.view("dcs")["status"], "waiting-game");
        assert!(broker.token("dcs").is_err());
    }
    #[test]
    fn process_exit_replacement_and_pid_reuse_revoke_old_source_and_command_tokens() {
        let mut broker = Broker::default();
        broker.set_owner(Some(owner(Some("dcs")))).unwrap();
        observe(&mut broker, "dcs", observed()).unwrap();
        let original = broker.token("dcs").unwrap();
        observe(&mut broker, "dcs", observed()).unwrap();
        assert_eq!(broker.token("dcs").unwrap(), original);
        let mut replacement = observed();
        replacement.process.as_mut().unwrap().epoch += 1;
        observe(&mut broker, "dcs", replacement).unwrap();
        assert!(broker.validate(&original).is_err());
        let replacement = broker.token("dcs").unwrap();
        let mut exit = observed();
        exit.process = None;
        observe(&mut broker, "dcs", exit).unwrap();
        assert!(broker.validate(&replacement).is_err());
        observe(&mut broker, "dcs", observed()).unwrap();
        assert!(broker.validate(&original).is_err());
    }
    #[test]
    fn profile_stop_setup_and_capability_changes_require_new_admission() {
        let mut broker = Broker::default();
        broker.set_owner(Some(owner(Some("dcs")))).unwrap();
        observe(&mut broker, "dcs", observed()).unwrap();
        let first = broker.token("dcs").unwrap();
        let mut setup = observed();
        setup.provider_setup_revision = format!("sha256:{}", "c".repeat(64));
        observe(&mut broker, "dcs", setup).unwrap();
        assert!(broker.validate(&first).is_err());
        let setup = broker.token("dcs").unwrap();
        let mut catalogue = observed();
        catalogue.catalogue_revision = Some(format!("sha256:{}", "d".repeat(64)));
        observe(&mut broker, "dcs", catalogue).unwrap();
        assert!(broker.validate(&setup).is_err());
        let catalogue = broker.token("dcs").unwrap();
        broker.set_owner(None).unwrap();
        assert!(broker.validate(&catalogue).is_err());
        broker.set_owner(Some(owner(Some("dcs")))).unwrap();
        assert!(broker.validate(&catalogue).is_err());
    }
    #[test]
    fn missing_setup_unsupported_platform_and_native_errors_are_distinct() {
        let mut broker = Broker::default();
        let mut observation = observed();
        observation.setup_ready = false;
        observe(&mut broker, "dcs", observation).unwrap();
        assert_eq!(broker.view("dcs")["status"], "setup-required");
        let mut observation = observed();
        observation.supported = false;
        observe(&mut broker, "dcs", observation).unwrap();
        assert_eq!(broker.view("dcs")["status"], "unsupported-platform");
        let mut observation = observed();
        observation.error = Some("Process inspection failed".into());
        observe(&mut broker, "dcs", observation).unwrap();
        assert_eq!(broker.view("dcs")["status"], "error");
        assert_eq!(broker.view("unknown")["status"], "waiting-game");
    }
    #[test]
    fn malformed_authority_and_forged_token_cannot_admit() {
        let mut broker = Broker::default();
        let mut invalid = owner(Some("dcs"));
        invalid.session = "editor-selected-profile".into();
        assert!(broker.set_owner(Some(invalid)).is_err());
        assert!(observe(&mut broker, "unknown", observed()).is_err());
        let mut invalid = observed();
        invalid.provider_setup_revision = "draft".into();
        assert!(observe(&mut broker, "dcs", invalid).is_err());
        broker.set_owner(Some(owner(Some("dcs")))).unwrap();
        observe(&mut broker, "dcs", observed()).unwrap();
        let mut token = broker.token("dcs").unwrap();
        token.session = uuid::Uuid::new_v4().to_string();
        assert!(broker.validate(&token).is_err());
    }
    #[test]
    fn malformed_new_evidence_withdraws_an_already_admitted_context() {
        let mut broker = Broker::default();
        broker.set_owner(Some(owner(Some("dcs")))).unwrap();
        observe(&mut broker, "dcs", observed()).unwrap();
        let token = broker.token("dcs").unwrap();
        let mut invalid = observed();
        invalid.process.as_mut().unwrap().epoch = 0;
        assert!(observe(&mut broker, "dcs", invalid).is_err());
        assert!(broker.validate(&token).is_err());
        assert_eq!(broker.view("dcs")["status"], "error");
        assert_eq!(broker.view("dcs")["processVerified"], false);
        observe(&mut broker, "dcs", observed()).unwrap();
        assert!(broker.validate(&token).is_err());
    }
}
