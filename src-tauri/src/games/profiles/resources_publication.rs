impl Runtime {
    pub(super) fn command_withdrawn(state: &State, token: &ResourceToken) -> bool {
        token.access == Access::Invoke
            && token
                .route_id
                .strip_prefix("profile-game:")
                .and_then(catalogue_game)
                .and_then(|game| {
                    state
                        .game_command_withdrawals
                        .get(&(game.into(), token.provider_session.clone()))
                })
                .is_some_and(|ports| ports.contains(&token.port_id))
    }
    /// Revoke only exact command ports of the current reader. Read tokens and
    /// the independently advancing telemetry catalogue remain unchanged.
    pub(crate) fn withdraw_profile_game_commands(
        &self,
        game: &str,
        reader: &str,
        ports: &[String],
    ) -> Result<(), String> {
        if self.stop.load(Ordering::Acquire) {
            return Err("Resource inventory has stopped.".into());
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let batch = state
            .game_deliveries
            .get(game)
            .filter(|batch| batch.session == reader)
            .ok_or(
                "The exact command reader is no longer current; its replacement was preserved.",
            )?;
        let known = batch
            .observations
            .iter()
            .flat_map(|item| item.ports.iter().flat_map(|p| &p.ports))
            .filter(|p| {
                p.direction == osg_profiles::Direction::Input
                    && matches!(p.contract, osg_profiles::PortContract::Action { .. })
            })
            .map(|p| p.id.as_str())
            .collect::<BTreeSet<_>>();
        if ports.len() > 128 || ports.iter().any(|port| !known.contains(port.as_str())) {
            return Err("Command withdrawal needs exact current native command ports.".into());
        }
        state
            .game_command_withdrawals
            .entry((game.into(), reader.into()))
            .or_default()
            .extend(ports.iter().cloned());
        Ok(())
    }
    /// Publication is deduplicated by native reader state. This cheap owner check
    /// still observes metadata revocation on every health tick without cloning
    /// or rescanning the provider catalogue.
    pub(crate) fn validate_profile_game_delivery(
        &self,
        game: &str,
        reader: &str,
    ) -> Result<(), String> {
        if self.stop.load(Ordering::Acquire) {
            return Err("Resource inventory has stopped.".into());
        }
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.read_only {
            return Err("Repair retained inventory before accepting game delivery.".into());
        }
        if !state
            .game_deliveries
            .get(game)
            .is_some_and(|batch| batch.session == reader)
        {
            return Err(
                "The native game reader lost its current inventory scope or verified metadata."
                    .into(),
            );
        }
        Ok(())
    }
    pub(crate) fn publish_profile_game_delivery(&self, batch: ProviderBatch) -> Result<(), String> {
        let _capture = self.capture_gate.lock().unwrap_or_else(|p| p.into_inner());
        if self.stop.load(Ordering::Acquire) {
            return Err("Resource inventory has stopped.".into());
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.read_only {
            return Err("Repair retained inventory before acquiring game delivery.".into());
        }
        let game = batch
            .scope
            .strip_prefix("profile-game:")
            .ok_or("Game delivery needs its dedicated native scope.")?
            .to_owned();
        let validation = validate_admitted_game_catalogues(&batch, &state.game_metadata);
        if let Err(error) = validation {
            if state
                .game_deliveries
                .get(&game)
                .is_some_and(|old| old.session == batch.session)
            {
                state.game_deliveries.remove(&game);
                state.registry.provider_failed(
                    &batch.scope,
                    "The acquired game reader lost validated sample metadata.",
                );
                state.pending_save = true;
            }
            return Err(error);
        }
        state.registry.reconcile(batch.clone())?;
        state
            .game_command_withdrawals
            .retain(|(id, reader), _| id != &game || reader == &batch.session);
        state.game_deliveries.insert(game, batch);
        state.pending_save = true;
        Ok(())
    }
    /// An outgoing run cannot revoke the incoming run's independently acquired reader.
    pub(crate) fn withdraw_profile_game_delivery(
        &self,
        game: &str,
        provider_session: &str,
    ) -> Result<(), String> {
        let _capture = self.capture_gate.lock().unwrap_or_else(|p| p.into_inner());
        if self.stop.load(Ordering::Acquire) {
            return Err("Resource inventory has stopped.".into());
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(old) = state
            .game_deliveries
            .get(game)
            .filter(|batch| batch.session == provider_session)
            .cloned()
        {
            state.game_deliveries.remove(game);
            state
                .game_command_withdrawals
                .remove(&(game.into(), provider_session.into()));
            state
                .registry
                .provider_failed(&old.scope, "The native profile game reader stopped.");
            state.pending_save = true;
        }
        Ok(())
    }
}
