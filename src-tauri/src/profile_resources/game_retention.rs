//! The configured native game catalogue owns integration membership in every projection.
use super::*;

pub(super) fn catalogue_membership(batches: &PreparedBatches) -> Option<BTreeSet<String>> {
    let marker = batches.iter().find(|(scope, _)| scope == "game-catalogue")?;
    let (marker, _) = marker.1.as_ref().ok()?;
    if !marker.complete || !marker.observations.is_empty() || marker.validate().is_err() {
        return None;
    }
    let mut ids = BTreeSet::new();
    for (scope, result) in batches {
        let Some(game) = scope.strip_prefix("game:") else { continue };
        let (batch, _) = result.as_ref().ok()?;
        if !batch.complete || batch.validate().is_err() {
            return None;
        }
        if !game.contains(':') && batch.observations.iter().any(|item| {
            item.key == game
                && item.resource_type == osg_profiles::resources::ResourceType::GameIntegration
                && item.attributes.get("gameId").map(String::as_str) == Some(game)
                && item.attributes.get("integrationKey").is_some_and(|key| {
                    crate::game_discovery::integration_provider(key).is_some()
                })
        }) {
            ids.insert(game.into());
        }
    }
    Some(ids)
}

pub(super) fn retain(state: &mut State, ids: &BTreeSet<String>) -> Result<bool, String> {
    let changed = state.registry.retain_game_integrations(ids)?;
    let owned = |key: &str| ids.contains(key.split(':').next().unwrap_or(key));
    state.game_metadata.retain(|key, _| owned(key));
    state.game_deliveries.retain(|key, _| owned(key));
    state.game_command_withdrawals.retain(|(key, _), _| owned(key));
    state.fingerprints.retain(|scope, _| {
        scope.strip_prefix("game:").or_else(|| scope.strip_prefix("profile-game:"))
            .is_none_or(owned)
    });
    Ok(changed)
}

#[cfg(test)]
#[path = "game_retention_tests.rs"]
mod tests;
