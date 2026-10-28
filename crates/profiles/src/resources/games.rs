use super::*;

impl Registry {
    /// Retain cached game metadata only for games in the authoritative native catalogue.
    ///
    /// Browser supplied game IDs must never be passed here. Physical and companion
    /// resources are intentionally outside this catalogue membership policy.
    pub fn retain_game_integrations(
        &mut self,
        game_ids: &BTreeSet<String>,
    ) -> Result<bool, String> {
        let mut next = self.clone();
        let mut removed_game_sighting = false;
        for entry in next.entries.values_mut() {
            let needs_prune = entry.saved.resource_type == ResourceType::GameIntegration
                && entry.saved.sightings.iter().any(|sighting| {
                    sighting_game_id(sighting).is_none_or(|id| !game_ids.contains(id))
                });
            if !needs_prune {
                continue;
            }
            let entry = Arc::make_mut(entry);
            let previous_count = entry.saved.sightings.len();
            entry.saved.sightings.retain(|sighting| {
                sighting_game_id(sighting).is_some_and(|id| game_ids.contains(id))
            });
            if entry.saved.sightings.len() == previous_count {
                continue;
            }
            removed_game_sighting = true;
            let retained = entry
                .saved
                .sightings
                .iter()
                .map(|sighting| (sighting.scope.as_str(), sighting.key.as_str()))
                .collect::<BTreeSet<_>>();
            entry
                .sighting_validation
                .retain(|key, _| retained.contains(&(key.0.as_str(), key.1.as_str())));
            entry
                .live
                .retain(|key, _| retained.contains(&(key.0.as_str(), key.1.as_str())));
            entry.invalidate_saved();
            entry.generation = entry
                .generation
                .checked_add(1)
                .ok_or("Resource generation exhausted.")?;
        }
        next.entries.retain(|_, entry| {
            entry.saved.resource_type != ResourceType::GameIntegration
                || !entry.saved.sightings.is_empty()
        });
        next.redirects
            .retain(|_, target| next.entries.contains_key(target));
        next.scopes
            .retain(|scope, _| scope_game_id(scope).is_none_or(|id| game_ids.contains(id)));
        next.failures
            .retain(|scope, _| scope_game_id(scope).is_none_or(|id| game_ids.contains(id)));

        let changed = next.entries.len() != self.entries.len()
            || removed_game_sighting
            || next.redirects != self.redirects
            || next.scopes.len() != self.scopes.len()
            || next.failures.len() != self.failures.len();
        if !changed {
            return Ok(false);
        }

        next.validate_saved()?;
        next.revision = self
            .revision
            .checked_add(1)
            .ok_or("Resource inventory revision exhausted.")?;
        *self = next;
        Ok(true)
    }

    /// Retire one acquired profile-game delivery while retaining its base authoring catalogue.
    pub fn retire_game_delivery(&mut self, scope: &str) -> Result<bool, String> {
        scope
            .strip_prefix("profile-game:")
            .filter(|game_id| !game_id.contains(':') && crate::activation::valid_game_id(game_id))
            .ok_or("Choose a valid profile-game delivery scope.")?;
        self.retire_game_deliveries(&BTreeSet::from([scope.to_owned()]))
    }

    /// Remove persisted profile-game overlays after startup, when no delivery reader exists.
    pub fn retire_restored_game_deliveries(&mut self) -> Result<bool, String> {
        let scopes = self
            .entries
            .values()
            .flat_map(|entry| entry.saved.sightings.iter().map(|sighting| &sighting.scope))
            .chain(
                self.entries
                    .values()
                    .flat_map(|entry| entry.live.keys().map(|(scope, _)| scope)),
            )
            .chain(self.scopes.keys())
            .chain(self.failures.keys())
            .filter(|scope| scope.starts_with("profile-game:"))
            .cloned()
            .collect();
        self.retire_game_deliveries(&scopes)
    }

    fn retire_game_deliveries(&mut self, scopes: &BTreeSet<String>) -> Result<bool, String> {
        if scopes.is_empty() {
            return Ok(false);
        }
        let mut next = self.clone();
        let mut changed_entry = false;
        for entry in next.entries.values_mut() {
            let needs_retirement = entry.saved.resource_type == ResourceType::GameIntegration
                && (entry
                    .saved
                    .sightings
                    .iter()
                    .any(|sighting| scopes.contains(&sighting.scope))
                    || entry.live.keys().any(|(scope, _)| scopes.contains(scope)));
            if !needs_retirement {
                continue;
            }
            let entry = Arc::make_mut(entry);
            let before_sightings = entry.saved.sightings.len();
            entry
                .saved
                .sightings
                .retain(|sighting| !scopes.contains(&sighting.scope));
            let saved_changed = before_sightings != entry.saved.sightings.len();
            let before_live = entry.live.len();
            entry
                .live
                .retain(|(entry_scope, _), _| !scopes.contains(entry_scope));
            let live_changed = before_live != entry.live.len();
            if !saved_changed && !live_changed {
                continue;
            }
            if saved_changed {
                entry
                    .sighting_validation
                    .retain(|(entry_scope, _), _| !scopes.contains(entry_scope));
                entry.invalidate_saved();
            } else {
                entry.invalidate_observed();
            }
            entry.generation = entry
                .generation
                .checked_add(1)
                .ok_or("Resource generation exhausted.")?;
            changed_entry = true;
        }
        let previous_count = next.entries.len();
        next.entries.retain(|_, entry| {
            entry.saved.resource_type != ResourceType::GameIntegration
                || !entry.saved.sightings.is_empty()
        });
        next.redirects
            .retain(|_, target| next.entries.contains_key(target));
        let previous_scopes = next.scopes.len();
        next.scopes.retain(|scope, _| !scopes.contains(scope));
        let scope_changed = previous_scopes != next.scopes.len();
        let previous_failures = next.failures.len();
        next.failures.retain(|scope, _| !scopes.contains(scope));
        let failure_changed = previous_failures != next.failures.len();
        let changed = changed_entry
            || previous_count != next.entries.len()
            || next.redirects != self.redirects
            || scope_changed
            || failure_changed;
        if !changed {
            return Ok(false);
        }

        next.validate_saved()?;
        next.revision = self
            .revision
            .checked_add(1)
            .ok_or("Resource inventory revision exhausted.")?;
        *self = next;
        Ok(true)
    }
}

fn scope_game_id(scope: &str) -> Option<&str> {
    let (kind, rest) = scope.split_once(':')?;
    matches!(kind, "game" | "profile-game").then(|| rest.split(':').next().unwrap_or(rest))
}

fn sighting_game_id(sighting: &SavedSighting) -> Option<&str> {
    if let Some(id) = sighting.attributes.get("gameId") {
        return Some(id);
    }
    sighting
        .evidence
        .iter()
        .find(|evidence| evidence.kind == EvidenceKind::GameScope)
        .map(|evidence| evidence.value.as_str())
        .or_else(|| sighting.scope.strip_prefix("game:"))
        .map(|value| value.split(':').next().unwrap_or(value))
}

#[cfg(test)]
#[path = "games_tests.rs"]
mod tests;
