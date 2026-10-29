fn validate_preparation_inner(inner: &Inner, proof: &GamePreparation) -> Result<(), String> {
    observe_preparation_inner(inner, proof).map(|_| ())
}

fn observe_preparation_inner(inner: &Inner, proof: &GamePreparation) -> Result<Instant, String> {
    let game = validate_preparation_state(inner, proof)?;
    inner.setup.validate_native_build(&game, false)?;
    // Expensive catalogue/build work precedes this exact process observation.
    // Its own duration still counts against the unchanged process lease.
    let observed = Instant::now();
    if inner.platform.integration_process(&game)?.as_ref() != Some(&proof.process)
        || !inner.platform.alive(&proof.process)?
    {
        return Err("The prepared game process exited, was replaced or became ambiguous.".into());
    }
    // A completed observation cannot publish over settings, catalogue, aircraft
    // or preparation changes that occurred while native process I/O was pending.
    if validate_preparation_state(inner, proof)? != game {
        return Err("The selected game installation changed.".into());
    }
    Ok(observed)
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
