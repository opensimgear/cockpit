//! A native acquired game reader publishes delivery separately from catalogue
//! metadata. No browser request can publish samples or acquire this authority.
use super::*;
use osg_profiles::{
    compiler_models::{ExecutionPlan, PlanReadiness},
    resources::{EvidenceKind, ResourceType, Transport},
};

fn catalogue_game(key: &str) -> Option<&str> {
    let (game, module) = key
        .split_once(':')
        .map(|(game, module)| (game, Some(module)))
        .unwrap_or((key, None));
    (crate::game_discovery::valid_id(game)
        && module.is_none_or(|module| {
            !module.is_empty()
                && module.trim() == module
                && module.len() <= 63
                && module
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_- ".contains(&byte))
        }))
    .then_some(game)
}
pub(super) fn game_metadata_valid(item: &Observation, key: &str) -> bool {
    let Some(game) = catalogue_game(key) else {
        return false;
    };
    let Some(integration) = item
        .attributes
        .get("integrationKey")
        .filter(|key| crate::game_discovery::integration_provider(key).is_some())
    else {
        return false;
    };
    item.key == key
        && item.resource_type == ResourceType::GameIntegration
        && item.attributes.get("gameId").map(String::as_str) == Some(game)
        && (key == game
            || integration == "dcs"
                && item.attributes.get("aircraftModule").map(String::as_str)
                    == key.split_once(':').map(|(_, module)| module))
        && (item.attributes.get("aircraftModule").map(String::as_str) != Some("CommonData")
            || item.ports.as_ref().is_some_and(|ports| {
                ports
                    .ports
                    .iter()
                    .all(|port| port.direction == osg_profiles::Direction::Output)
            }))
        && item.attributes.get("processVerified").map(String::as_str) == Some("true")
        && matches!(
            item.attributes.get("gameContextStatus").map(String::as_str),
            Some("ready" | "waiting-profile")
        )
        && ["providerSetupRevision", "catalogueRevision"]
            .iter()
            .all(|field| item.attributes.get(*field).is_some_and(|s| !s.is_empty()))
        && item
            .evidence
            .iter()
            .any(|e| e.kind == EvidenceKind::GameScope && e.value == key)
        && item.route.as_ref().is_some_and(|r| {
            r.id == format!("game:{key}")
                && r.transport == Transport::Integration
                && !r.baseline_ready
                && !r.writable
        })
        && item
            .ports
            .as_ref()
            .is_some_and(|p| p.complete && p.validate().is_ok())
}

/// Created only when a changed native metadata batch is admitted. Keeping the
/// observation private and immutable ties its fully validated port index to the
/// exact provider fingerprint, setup and catalogue for every later read.
pub(super) struct GameMetadata {
    observation: Observation,
    fingerprint: String,
    contracts: BTreeMap<String, String>,
}
impl GameMetadata {
    pub(super) fn capture(item: Observation, fingerprint: String) -> Option<Arc<Self>> {
        if !game_metadata_valid(&item, &item.key) {
            return None;
        }
        let contracts = item
            .ports
            .as_ref()?
            .ports
            .iter()
            .map(|port| Ok((port.id.clone(), port.contract_hash()?)))
            .collect::<Result<_, String>>()
            .ok()?;
        Some(Arc::new(Self {
            observation: item,
            fingerprint,
            contracts,
        }))
    }
}

#[cfg(test)]
fn validate_game_delivery(batch: &ProviderBatch, metadata: &Observation) -> Result<(), String> {
    batch.validate()?;
    if !batch.complete || batch.observations.len() != 1 {
        return Err("Fresh game delivery requires one complete native game observation and current verified metadata.".into());
    }
    let metadata = GameMetadata::capture(metadata.clone(), "test-native-capture".into())
        .ok_or("Game delivery requires valid current native metadata.")?;
    validate_game_item(batch, &batch.observations[0], &metadata)
}
fn validate_game_item(
    batch: &ProviderBatch,
    item: &Observation,
    admitted: &GameMetadata,
) -> Result<(), String> {
    let metadata = &admitted.observation;
    let key = metadata.key.as_str();
    let game = catalogue_game(key).ok_or("Unknown game catalogue scope.")?;
    if admitted.fingerprint.is_empty() || batch.scope != format!("profile-game:{game}") {
        return Err(
            "Game delivery requires current verified metadata for its exact native module.".into(),
        );
    }
    if item.key != key
        || item.resource_type != ResourceType::GameIntegration
        || item.evidence != metadata.evidence
        || item.attributes.get("gameId") != metadata.attributes.get("gameId")
        || item.attributes.get("gameReaderSession") != Some(&batch.session)
        || !item
            .attributes
            .get("gamePreparationSession")
            .is_some_and(|proof| osg_profiles::resources_ports::text(proof, 256).is_ok())
        || [
            "integrationKey",
            "providerSetupRevision",
            "catalogueRevision",
            "aircraftModule",
        ]
        .iter()
        .any(|field| item.attributes.get(*field) != metadata.attributes.get(*field))
    {
        return Err(
            "Game delivery identity, preparation owner, setup or catalogue changed.".into(),
        );
    }
    let route = item
        .route
        .as_ref()
        .ok_or("Game delivery needs its actual native route.")?;
    if route.id != format!("profile-game:{key}")
        || route.transport != Transport::Integration
        || route.priority != 100
        || (route.writable && !route.baseline_ready)
        || [
            "gameId",
            "integrationKey",
            "providerSetupRevision",
            "catalogueRevision",
            "gamePreparationSession",
            "gameReaderSession",
            "aircraftModule",
        ]
        .iter()
        .any(|field| route.attributes.get(*field) != item.attributes.get(*field))
    {
        return Err("Game delivery route does not match the acquired native reader.".into());
    }
    let ports = item
        .ports
        .as_ref()
        .filter(|p| p.complete)
        .ok_or("Game delivery needs its exact complete selected-port catalogue.")?;
    for port in &ports.ports {
        let native = admitted
            .contracts
            .get(&port.id)
            .ok_or("Game delivery contains a port absent from the current native catalogue.")?;
        if *native != port.contract_hash()? {
            return Err(
                "Game delivery port contract changed; reconcile its local assignment.".into(),
            );
        }
    }
    Ok(())
}
#[cfg(test)]
pub(super) fn validate_game_catalogues(
    batch: &ProviderBatch,
    metadata: &BTreeMap<String, Observation>,
) -> Result<(), String> {
    let metadata = metadata
        .iter()
        .map(|(key, item)| {
            GameMetadata::capture(item.clone(), "test-native-capture".into())
                .map(|item| (key.clone(), item))
                .ok_or("Game delivery requires valid current native metadata.")
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    validate_admitted_game_catalogues(batch, &metadata)
}
pub(super) fn validate_admitted_game_catalogues(
    batch: &ProviderBatch,
    metadata: &BTreeMap<String, Arc<GameMetadata>>,
) -> Result<(), String> {
    batch.validate()?;
    let game = batch
        .scope
        .strip_prefix("profile-game:")
        .filter(|game| crate::game_discovery::valid_id(game))
        .ok_or("Unknown native game delivery scope.")?;
    if !batch.complete
        || batch.observations.is_empty()
        || batch.observations.len()
            > if batch
                .observations
                .first()
                .and_then(|item| item.attributes.get("integrationKey"))
                .map(String::as_str)
                == Some("dcs")
            {
                2
            } else {
                1
            }
    {
        return Err("Game delivery must contain one native catalogue, or DCS CommonData plus its single active aircraft catalogue.".into());
    }
    let mut keys = BTreeSet::new();
    let mut module_count = 0;
    let first = &batch.observations[0];
    for item in &batch.observations {
        if !keys.insert(&item.key) || catalogue_game(&item.key) != Some(game) {
            return Err("Game delivery contains duplicate or foreign catalogue identities.".into());
        }
        if item.key.contains(':') {
            module_count += 1;
        }
        if module_count > 1
            || [
                "integrationKey",
                "providerSetupRevision",
                "gamePreparationSession",
                "gameReaderSession",
            ]
            .iter()
            .any(|field| item.attributes.get(*field) != first.attributes.get(*field))
        {
            return Err(
                "Game delivery catalogues must belong to one acquired aircraft reader.".into(),
            );
        }
        validate_game_item(
            batch,
            item,
            metadata
                .get(&item.key)
                .ok_or("The game has no current verified native module metadata.")?,
        )?;
    }
    Ok(())
}
include!("games/profiles/resources_publication.rs");
include!("games/profiles/resources_binding.rs");

#[cfg(test)]
#[path = "profile_resources_game_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "profile_resources_game_persistence_tests.rs"]
mod persistence_tests;
