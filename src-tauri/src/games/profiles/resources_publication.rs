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
        let result = Self::validate_game_delivery_owner(&state, game, reader);
        state.confirm_game_owner(game, reader, &result);
        result
    }
    pub(crate) fn try_validate_profile_game_delivery(&self, game: &str, reader: &str) -> Result<Option<()>, String> {
        if self.stop.load(Ordering::Acquire) { return Err("Resource inventory has stopped.".into()); }
        let state = match self.state.try_lock() {
            Ok(state) => state,
            Err(std::sync::TryLockError::WouldBlock) => return self.state.confirmed_game_owner(game, reader),
            Err(std::sync::TryLockError::Poisoned(poison)) => poison.into_inner(),
        };
        let result = Self::validate_game_delivery_owner(&state, game, reader);
        state.confirm_game_owner(game, reader, &result);
        result.map(Some)
    }
    /// Setup-only predecessor capture after outgoing admission has stopped.
    /// A dormant run must remember an older run's withdrawn reader for CAS.
    pub(crate) fn profile_game_reader(&self, game: &str) -> Result<Option<String>, String> {
        if self.stop.load(Ordering::Acquire) { return Err("Resource inventory has stopped.".into()); }
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.read_only { return Err("Repair retained inventory before acquiring game delivery.".into()); }
        Ok(state.game_delivery_readers.get(game).cloned())
    }
    pub(super) fn validate_game_delivery_owner(state: &State, game: &str, reader: &str) -> Result<(), String> {
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
        Self::publish_game_delivery_locked(&mut state, batch, || true).map(|_| ())
    }
    /// Live updates cannot claim an inventory scope. Only setup/admission may
    /// install a reader; an outgoing worker cannot replace or resurrect one.
    pub(crate) fn publish_current_profile_game_delivery(&self, batch: ProviderBatch,
        is_current: impl Fn() -> bool) -> Result<bool, String> {
        let _capture = self.capture_gate.lock().unwrap_or_else(|p| p.into_inner());
        if self.stop.load(Ordering::Acquire) { return Err("Resource inventory has stopped.".into()); }
        if !is_current() { return Ok(false); }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if !is_current() { return Ok(false); }
        let game = batch.scope.strip_prefix("profile-game:")
            .ok_or("Game delivery needs its dedicated native scope.")?;
        if !state.game_deliveries.get(game).is_some_and(|current| current.session == batch.session) {
            return Ok(false);
        }
        Self::publish_game_delivery_locked(&mut state, batch, is_current)
    }
    /// Returning readers compare against the last admitted reader even after
    /// its overlay was withdrawn. The volatile tombstone is never persisted.
    pub(crate) fn replace_profile_game_delivery(&self, batch: ProviderBatch, previous: Option<&str>,
        is_current: impl Fn() -> bool) -> Result<bool, String> {
        let _capture = self.capture_gate.lock().unwrap_or_else(|p| p.into_inner());
        if self.stop.load(Ordering::Acquire) { return Err("Resource inventory has stopped.".into()); }
        if !is_current() { return Ok(false); }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if !is_current() { return Ok(false); }
        let game = batch.scope.strip_prefix("profile-game:")
            .ok_or("Game delivery needs its dedicated native scope.")?;
        if state.game_delivery_readers.get(game).map(String::as_str) != previous { return Ok(false); }
        Self::publish_game_delivery_locked(&mut state, batch, is_current)
    }
    fn publish_game_delivery_locked(state: &mut State, batch: ProviderBatch, is_current: impl Fn() -> bool) -> Result<bool, String> {
        if state.read_only {
            return Err("Repair retained inventory before acquiring game delivery.".into());
        }
        let game = batch
            .scope
            .strip_prefix("profile-game:")
            .ok_or("Game delivery needs its dedicated native scope.")?
            .to_owned();
        let validation = validate_admitted_game_catalogues(&batch, &state.game_metadata);
        if !is_current() { return Ok(false); }
        if let Err(error) = validation {
            if state
                .game_deliveries
                .get(&game)
                .is_some_and(|old| old.session == batch.session)
            {
                state.game_deliveries.remove(&game);
                state.registry.retire_game_delivery(&batch.scope)?;
                state.pending_save = true;
            }
            return Err(error);
        }
        if !state.game_delivery_readers.contains_key(&game) && state.game_delivery_readers.len() >= 64 {
            return Err("Native game reader history exceeds its 64-game budget.".into());
        }
        state.registry.reconcile(batch.clone())?;
        state
            .game_command_withdrawals
            .retain(|(id, reader), _| id != &game || reader == &batch.session);
        state.game_delivery_readers.insert(game.clone(), batch.session.clone());
        state.game_deliveries.insert(game, batch);
        state.pending_save = true;
        Ok(true)
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
            state.registry.retire_game_delivery(&old.scope)?;
            state.pending_save = true;
        }
        Ok(())
    }
}
