fn validate_preparation_inner(inner: &Inner, proof: &GamePreparation) -> Result<(), String> {
    observe_preparation_inner(inner, proof, |game, expected| inner.exact_process_current(game, expected)).map(|_| ())
}

fn observe_preparation_inner(inner: &Inner, proof: &GamePreparation,
    verify: impl FnOnce(&Value, &crate::game_discovery::ProcessIdentity) -> Result<bool, String>,
) -> Result<Instant, String> {
    let game = validate_preparation_state(inner, proof)?;
    inner.setup.validate_native_build(&game, false)?;
    // Expensive catalogue/build work precedes this exact process observation.
    // Its own duration still counts against the unchanged process lease.
    let observed = Instant::now();
    if !verify(&game, &proof.process)? {
        return Err("The prepared game process exited, was replaced or became ambiguous.".into());
    }
    // A completed observation cannot publish over settings, catalogue, aircraft
    // or preparation changes that occurred while native process I/O was pending.
    if validate_preparation_state(inner, proof)? != game {
        return Err("The selected game installation changed.".into());
    }
    Ok(observed)
}

impl Inner {
    fn current_process_epoch(&self) -> Option<(u64, u64)> {
        let library = self.changes.context_wake.observed();
        let processes = self.changes.context_process_wake.observed();
        (!self.closed.load(Ordering::Acquire) && self.changes.context_process_live(library))
            .then_some((library, processes))
    }

    fn exact_process_current(&self, game: &Value, expected: &crate::game_discovery::ProcessIdentity) -> Result<bool, String> {
        Ok(self.platform.integration_process(game)?.as_ref() == Some(expected) && self.platform.alive(expected)?)
    }

    fn observe_profile_process(&self, game: &Value, expected: &crate::game_discovery::ProcessIdentity) -> Result<bool, String> {
        if let Some(epoch) = self.current_process_epoch() {
            let snapshot = self.context_process_inventory.lock().unwrap_or_else(|p| p.into_inner()).clone();
            if let Some(process) = snapshot.as_ref().and_then(|snapshot| snapshot.process(game, epoch)) {
                // Even inside a live inventory epoch, the exact process must
                // still be alive. Commands use independent strict discovery.
                let live = if process.as_ref() == Some(expected) { self.platform.alive(expected) } else { Ok(false) };
                if self.current_process_epoch() == Some(epoch) { return live; }
            }
        }
        self.exact_process_current(game, expected)
    }
}

fn validate_preparation_state(inner: &Inner, proof: &GamePreparation) -> Result<Value, String> {
    inner.guard_installation_metadata()?;
    inner.contexts.lock().unwrap_or_else(|p| p.into_inner()).validate_preparation(proof)?;
    let game = inner.settings.lock().unwrap_or_else(|p| p.into_inner()).catalogue.iter()
        .find(|game| game["id"] == proof.game_id).map(preparation_installation)
        .ok_or("The selected game installation changed.")?;
    if inner.runtime_vehicle_id(&proof.game_id) != proof.vehicle_id {
        return Err("The loaded DCS aircraft changed; acquire its current module before using this branch.".into());
    }
    let integration = crate::game_discovery::integration_key(&game)
        .ok_or("The game integration binding changed.")?;
    if inner.setup.provider_setup_revision(integration) != proof.provider_setup_revision
        || inner.setup.current_catalogue_revision_for_vehicle(integration, proof.vehicle_id.as_deref())
            .as_ref() != Some(&proof.catalogue_revision)
    {
        return Err("The prepared provider setup or catalogue changed.".into());
    }
    Ok(game)
}

/// Capture only native process/build inputs and installation eligibility. Library
/// presentation and unselected installation metadata cannot revoke this reader.
fn preparation_installation(game: &Value) -> Value {
    const FIELDS: &[&str] = &[
        "id", "provider", "integrationKey", "integrationCompatible", "executable",
        "installPath", "processExecutables", "selectedInstallationId", "installed",
        "processMonitoringSupported", "launchSupported",
    ];
    Value::Object(FIELDS.iter().filter_map(|key| game.get(*key)
        .map(|value| ((*key).to_owned(), value.clone()))).collect())
}
