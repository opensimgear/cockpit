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
    /// DCS aircraft identity comes from fresh native exports. Even aliases that
    /// share a module must retire the previous flight's command authority.
    pub vehicle_id: Option<String>,
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
    pub vehicle_id: Option<String>,
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
    pub vehicle_id: Option<String>,
}
impl ContextToken {
    pub(crate) fn matches_preparation(&self, prepared: &GamePreparation) -> bool {
        self.game_id == prepared.game_id
            && self.process == prepared.process
            && self.provider_setup_revision == prepared.provider_setup_revision
            && self.catalogue_revision == prepared.catalogue_revision
            && self.vehicle_id == prepared.vehicle_id
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
                    .is_some_and(|game| !osg_profiles::activation::valid_game_id(game))
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
        if !crate::game_discovery::valid_id(id)
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
                .vehicle_id
                .as_ref()
                .is_some_and(|vehicle| crate::game_vehicles::vehicle_id("dcs", vehicle).is_none())
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
                        vehicle_id: observation.vehicle_id.clone(),
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
    /// Withdrawing a binding also fences captures started before its removal.
    pub(crate) fn retain_bindings(&mut self, ids: &[String]) {
        let removed = self
            .games
            .keys()
            .filter(|id| !ids.contains(id))
            .cloned()
            .collect::<Vec<_>>();
        if removed.is_empty() {
            return;
        }
        if let Ok(epoch) = self.begin_observation() {
            for id in removed {
                self.observation_commits.insert(id.clone(), epoch);
                self.games.remove(&id);
            }
        }
    }
    pub(crate) fn preparation(&self, id: &str) -> Result<Option<GamePreparation>, String> {
        if !crate::game_discovery::valid_id(id) {
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
            vehicle_id: observed.vehicle_id.clone(),
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
#[path = "game_context_tests.rs"]
mod tests;
