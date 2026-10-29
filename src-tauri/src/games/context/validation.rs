fn validate_preparation_inner(inner: &Inner, proof: &GamePreparation) -> Result<(), String> {
        let result: Result<(), String> = (|| {
            inner.guard_installation_metadata()?;
            inner
                .contexts
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .validate_preparation(proof)?;
            let game = inner
                .settings
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .catalogue
                .iter()
                .find(|game| game["id"] == proof.game_id)
                .cloned()
                .ok_or("The selected game installation changed.")?;
            inner.setup.validate_native_build(&game, false)?;
            if inner.platform.integration_process(&game)?.as_ref() != Some(&proof.process)
                || !inner.platform.alive(&proof.process)?
            {
                return Err(
                    "The prepared game process exited, was replaced or became ambiguous.".into(),
                );
            }
            if inner.runtime_vehicle_id(&proof.game_id) != proof.vehicle_id {
                return Err("The loaded DCS aircraft changed; acquire its current module before using this branch.".into());
            }
            let integration = crate::game_discovery::integration_key(&game)
                .ok_or("The game integration binding changed.")?;
            if inner.setup.provider_setup_revision(integration)
                != proof.provider_setup_revision
                || inner
                    .setup
                    .current_catalogue_revision_for_vehicle(
                        integration,
                        proof.vehicle_id.as_deref(),
                    )
                    .as_ref()
                    != Some(&proof.catalogue_revision)
            {
                return Err("The prepared provider setup or catalogue changed.".into());
            }
            Ok(())
        })();
        result
}
