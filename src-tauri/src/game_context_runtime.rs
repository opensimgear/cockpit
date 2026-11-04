//! Process observation is native background work. UI reads do not authorize an
//! applied profile, start a provider, or turn an installation into live identity.
use super::*;
use crate::game_context::{ContextToken, GamePreparation, Observation, ProfileOwner};
#[derive(Clone, Debug)]
pub(crate) struct PreparedGame {
    pub proof: GamePreparation,
    pub integration_key: String,
    pub connection: Value,
    pub catalogue: Value,
    pub aircraft_module: Option<String>,
}
impl GamesRuntime {
    /// Foreground desktop output needs exact process identity independently of
    /// optional simulator telemetry setup and catalogue qualification.
    pub(crate) fn desktop_process(
        &self,
        id: &str,
    ) -> Result<Option<crate::game_discovery::ProcessIdentity>, String> {
        let game = self.registered_profile_game(id)?;
        let process = self.inner.platform.find(&game)?;
        match process {
            Some(process) if self.inner.platform.alive(&process)? => Ok(Some(process)),
            _ => Ok(None),
        }
    }
    pub(crate) fn prepare_profile_game_context(
        &self,
        id: &str,
    ) -> Result<Option<PreparedGame>, String> {
        let registered = self.registered_profile_game(id)?;
        // A context-only game is a valid profile owner and desktop scope, but
        // never acquires simulator preparation or provider command authority.
        if crate::game_discovery::integration_key(&registered).is_none() {
            return Ok(None);
        }
        self.inner.refresh_game_contexts(true);
        let Some(proof) = self
            .inner
            .contexts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .preparation(id)?
        else {
            return Ok(None);
        };
        let game = self
            .inner
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .catalogue
            .iter()
            .find(|game| game["id"] == id)
            .cloned()
            .ok_or("The selected game installation changed.")?;
        if let Err(error) = self.inner.setup.validate_native_build(&game, true) {
            self.inner
                .contexts
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .invalidate_preparation(&proof, error.clone());
            return Err(error);
        }
        let (connection, revision, saved_aircraft_module) =
            self.inner.setup.provider_connection_snapshot(
                crate::game_discovery::integration_key(&game)
                    .ok_or("The game integration binding changed.")?,
            )?;
        let catalogue = self
            .inner
            .setup
            .resource_integration_catalogue_for_vehicle(
                crate::game_discovery::integration_key(&game)
                    .ok_or("The game integration binding changed.")?,
                proof.vehicle_id.as_deref(),
            )?;
        let aircraft_module = if crate::game_discovery::integration_key(&game) == Some("dcs") {
            catalogue["aircraftModule"].as_str().map(str::to_owned)
        } else {
            saved_aircraft_module
        };
        if revision != proof.provider_setup_revision
            || catalogue["providerSetupRevision"] != revision
            || catalogue["catalogueRevision"] != proof.catalogue_revision
        {
            return Err(
                "The game provider settings changed during preparation; prepare it again.".into(),
            );
        }
        self.validate_game_preparation(&proof)?;
        Ok(Some(PreparedGame {
            integration_key: crate::game_discovery::integration_key(&game)
                .ok_or("The game integration binding changed.")?
                .into(),
            proof,
            connection,
            catalogue,
            aircraft_module,
        }))
    }
    pub(crate) fn validate_game_preparation(&self, proof: &GamePreparation) -> Result<(), String> {
        let result = validate_preparation_inner(&self.inner, proof);
        if let Err(error) = &result {
            self.inner.contexts.lock().unwrap_or_else(|p| p.into_inner())
                .invalidate_preparation(proof, error.clone());
        }
        result
    }
    /// Capture identity for strict validation away from the live health path.
    pub(crate) fn profile_validation_task(
        &self, proof: GamePreparation, context: Option<ContextToken>,
    ) -> impl FnMut() -> Result<Instant, String> + Send + 'static {
        let inner = self.inner.clone();
        move || {
            let observed = observe_preparation_inner(&inner, &proof)?;
            if let Some(token) = &context {
                if !token.matches_preparation(&proof) {
                    return Err("The delivered game context differs from its preparation.".into());
                }
                inner.contexts.lock().unwrap_or_else(|p| p.into_inner()).validate(token)?;
            }
            Ok(observed)
        }
    }
    pub(crate) fn admit_prepared_game_context(
        &self,
        proof: &GamePreparation,
    ) -> Result<ContextToken, String> {
        self.validate_game_preparation(proof)?;
        let token = self
            .inner
            .contexts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .token(&proof.game_id)?;
        if !token.matches_preparation(proof) {
            return Err("The applied context does not match its native game preparation.".into());
        }
        self.inner
            .contexts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .validate(&token)?;
        Ok(token)
    }
    pub(crate) fn validate_admitted_game_context(
        &self,
        token: &ContextToken,
        proof: &GamePreparation,
    ) -> Result<(), String> {
        self.validate_game_preparation(proof)?;
        if !token.matches_preparation(proof) {
            return Err("The delivered game context differs from its preparation.".into());
        }
        self.inner
            .contexts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .validate(token)
    }
    pub(super) fn start_context_observer(&self) {
        if !self.inner.platform.integration_setup_supported() {
            return;
        }
        let inner = self.inner.clone();
        *self.observer.lock().unwrap_or_else(|p| p.into_inner()) = Some(thread::spawn(move || {
            let polling_closed = Arc::new(AtomicBool::new(false));
            let polling_inner = inner.clone();
            let polling_stop = polling_closed.clone();
            let capture_wake = Arc::new(osg_streams::NativeWake::default());
            inner.update_vehicles(|vehicles| vehicles.watch_captures(capture_wake.clone()));
            let worker_wake = capture_wake.clone();
            // Catalogue parsing and setup inspection can take longer than the
            // native freshness window. Keep exact process checks and telemetry
            // draining independent of that work, without extending either lease.
            let polling = thread::spawn(move || {
                let _ = worker_wake.bind_thread(thread::current());
                let mut next_maintenance = Instant::now();
                let mut drained = worker_wake.observed();
                while !polling_inner.closed.load(Ordering::Acquire)
                    && !polling_stop.load(Ordering::Acquire)
                {
                    let observed = worker_wake.observed();
                    let now = Instant::now();
                    if now >= next_maintenance {
                        polling_inner.poll_vehicle_telemetry(false);
                        next_maintenance = Instant::now() + Duration::from_millis(100);
                    } else if observed != drained {
                        polling_inner.poll_vehicle_telemetry(true);
                    }
                    drained = observed;
                    worker_wake.wait_scheduled(observed, Some(next_maintenance.saturating_duration_since(Instant::now())));
                }
            });
            let context_wake = &inner.changes.context_wake;
            let _ = context_wake.bind_thread(thread::current());
            let mut last_library = context_wake.observed();
            let mut sdk_watches = context_sdk_watch::SdkWatches::default();
            let mut assetto_watches = context_assetto_watch::AssettoWatches::default();
            while !inner.closed.load(Ordering::Acquire) {
                let library = context_wake.observed();
                let sdk_changed = sdk_watches.refresh(&inner);
                assetto_watches.refresh(&inner);
                inner.refresh_game_contexts(library != last_library);
                if sdk_changed { context_sdk_watch::publish_change(&inner); }
                last_library = library;
                if inner.closed.load(Ordering::Acquire) { break; }
                context_wake.wait_scheduled(library, context_sdk_watch::maintenance_delay(&inner));
            }
            polling_closed.store(true, Ordering::Release);
            capture_wake.notify();
            let _ = polling.join();
        }));
    }
    pub(super) fn decorate_game_contexts(&self, games: &mut [Value]) {
        self.decorate_game_contexts_with(games, |id| {
            self.current_vehicle_id(id).and_then(|vehicle| {
                self.inner.setup.cached_integration_catalogue_metadata_for_vehicle("dcs", Some(&vehicle))
                    .and_then(|catalogue| catalogue["aircraftModule"].as_str().map(str::to_owned))
            })
        });
    }
    fn decorate_game_contexts_with(&self, games: &mut [Value], mut active_module: impl FnMut(&str) -> Option<String>) {
        for game in games {
            let id = game["id"].as_str().unwrap_or("").to_owned();
            if crate::game_discovery::integration_key(game).is_none() {
                if let Some(game) = game.as_object_mut() {
                    game.remove("integrationContext");
                }
                continue;
            }
            if crate::game_discovery::integration_key(game) == Some("dcs") {
                game["integrationSetup"]["aircraftModuleAutomatic"] = json!(true);
                game["integrationSetup"]["activeAircraftModule"] = json!(active_module(&id));
            }
            // Provider metadata may scan/parse catalogue files. Only copy the
            // broker view under its mutex; decoration never retains authority.
            let mut context = self.inner.contexts.lock().unwrap_or_else(|p| p.into_inner()).view_for(game);
            if context["providerSetupRevision"].is_null() {
                context["providerSetupRevision"] =
                    game["integrationSetup"]["providerSetupRevision"].clone();
            }
            if !self.inner.platform.integration_setup_supported() {
                context["status"] = json!("unsupported-platform");
                context["message"] =
                    json!("Native game process identity verification requires Windows.");
            }
            game["integrationContext"] = context;
        }
    }
    pub(crate) fn resource_integration_catalogue(&self, id: &str) -> Result<Value, String> {
        let game = self.registered_profile_game(id)?;
        self.inner.setup.resource_integration_catalogue(
            crate::game_discovery::integration_binding(&game)
                .ok_or("Attach a compatible integration first.")?,
        )
    }
    /// P4's native coordinator supplies the immutable applied owner. This method
    /// is deliberately absent from IPC and does not start integration workers.
    pub(crate) fn set_profile_context_owner(
        &self,
        owner: Option<ProfileOwner>,
    ) -> Result<(), String> {
        self.inner
            .contexts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .set_owner(owner)?;
        // Owner publication only remints authority from the retained native
        // observation. Admission still verifies current process/setup/catalogue;
        // discovery here would block renewal of already acquired output leases.
        Ok(())
    }
    pub(crate) fn profile_game_context(&self, id: &str) -> Result<ContextToken, String> {
        self.inner.refresh_game_contexts(true);
        self.inner
            .contexts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .token(id)
    }
    /// Source attachment, telemetry consumption and commands must use this native
    /// check rather than a browser's cached contextSession label.
    pub(crate) fn validate_profile_game_context(&self, token: &ContextToken) -> Result<(), String> {
        self.inner.refresh_game_contexts(true);
        self.inner
            .contexts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .validate(token)
    }
}

impl Inner {
    // Reader destruction may unsubscribe in bounded batches and sleep. Retire
    // under the mutex, then drop its owned batch only after releasing the guard.
    fn update_vehicles<R>(&self, update: impl FnOnce(&mut crate::game_vehicles::Tracker) -> R) -> R {
        let (value, retired) = { let mut vehicles = self.vehicles.lock().unwrap_or_else(|p| p.into_inner());
            let value = update(&mut vehicles); (value, vehicles.take_retired()) };
        drop(retired);
        value
    }
    fn vehicle_telemetry_games(&self, capture_only: bool) -> Vec<Value> {
        if self.guard_installation_metadata().is_ok() {
            self.settings
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .catalogue
                .iter()
                .filter(|game| crate::game_discovery::integration_key(game)
                    .is_some_and(|key| !capture_only || key.starts_with("assetto-corsa")
                        || matches!(key, "dcs" | "x-plane-11" | "x-plane-12")))
                .cloned()
                .collect()
        } else {
            vec![]
        }
    }
    fn poll_vehicle_telemetry(&self, capture_only: bool) {
        let games = self.vehicle_telemetry_games(capture_only);
        let candidates = {
            let mut vehicles = self.vehicles.lock().unwrap_or_else(|p| p.into_inner());
            if capture_only { vehicles.capture_candidates() } else { vehicles.poll_candidates() }
        };
        // Capture wakes check configuration without renewing the process
        // observation or querying Windows for each packet. The independent
        // maintenance pass still checks process identity.
        let alive = |process: &crate::game_discovery::ProcessIdentity| {
            capture_only || self.platform.alive(process).unwrap_or(false)
        };
        let verified = candidates.into_iter().map(|candidate| {
            let checked = candidate.verify(&games, |id| {
                let key = games.iter().find(|game| game["id"] == id)
                    .and_then(crate::game_discovery::integration_key).unwrap_or("");
                self.setup.provider_setup_revision(key)
            }, alive);
            if capture_only { checked.without_renewal() } else { checked }
        }).collect();
        let polls = self.update_vehicles(|vehicles| vehicles.begin_polls(verified));
        for poll in polls {
            let result = poll.read();
            let current_games = self.vehicle_telemetry_games(capture_only);
            let result = result.verify(&current_games, |id| {
                let key = current_games.iter().find(|game| game["id"] == id)
                    .and_then(crate::game_discovery::integration_key).unwrap_or("");
                self.setup.provider_setup_revision(key)
            }, alive);
            self.update_vehicles(|vehicles| vehicles.apply_poll(result));
        }
    }

    fn runtime_vehicle_id(&self, id: &str) -> Option<String> {
        // Other games already select their profile vehicle independently. DCS
        // additionally binds decoder/command authority to its exact aircraft.
        (self
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .catalogue
            .iter()
            .find(|game| game["id"] == id)
            .and_then(crate::game_discovery::integration_key)
            == Some("dcs"))
        .then(|| {
            self.vehicles
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .current(id)
        })
        .flatten()
        .and_then(|vehicle| vehicle["id"].as_str().map(str::to_owned))
    }
    pub(super) fn refresh_game_contexts(&self, force_setup: bool) {
        let epoch = match self
            .contexts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .begin_observation()
        {
            Ok(epoch) => epoch,
            Err(_) => return,
        };
        let installation_metadata = self.guard_installation_metadata();
        let mut games = self
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .catalogue
            .iter()
            .filter(|game| crate::game_discovery::integration_key(game).is_some())
            .cloned()
            .collect::<Vec<_>>();
        let bound_ids = games
            .iter()
            .filter_map(|game| game["id"].as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        self.contexts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .retain_bindings(&bound_ids);
        let mut checked = self
            .context_setup_check
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if force_setup || checked.is_none() {
            self.setup.refresh(&games);
            *checked = Some(Instant::now());
        } else if checked.is_some_and(|last| last.elapsed() >= Duration::from_secs(2)) {
            self.setup.refresh_external_setup(&games);
            *checked = Some(Instant::now());
        }
        drop(checked);
        self.setup.decorate_source(&mut games);
        for game in games {
            let Some(id) = game["id"].as_str() else {
                continue;
            };
            let Some(integration) = crate::game_discovery::integration_key(&game) else {
                continue;
            };
            let revision = self.setup.provider_setup_revision(integration);
            let supported = self.platform.integration_setup_supported()
                && game["processMonitoringSupported"]
                    .as_bool()
                    .unwrap_or(game["launchSupported"] == true);
            let process: Result<Option<crate::game_discovery::ProcessIdentity>, String> = (|| {
                if let Err(error) = &installation_metadata {
                    return Err(error.clone());
                }
                if !supported || game["installed"] != true {
                    return Ok(None);
                }
                self.platform.validate(
                    id,
                    game["executable"]
                        .as_str()
                        .ok_or("Choose a game executable before process verification.")?,
                )?;
                let process = self.platform.integration_process(&game)?;
                match process {
                    Some(process) if self.platform.alive(&process)? => Ok(Some(process)),
                    _ => Ok(None),
                }
            })(
            );
            let vehicle_process = process.as_ref().ok().cloned().flatten();
            let error = process.as_ref().err().cloned();
            let connection = self
                .setup
                .provider_connection_snapshot(integration)
                .ok()
                .map(|(connection, _, _)| connection);
            if let Some(connection) = connection {
                let work = self.update_vehicles(|vehicles| vehicles.begin_observation(&game, &connection, vehicle_process, &revision,
                        supported && game["integrationSetup"]["state"] == "ready", epoch));
                if let Some(work) = work {
                    let observed = work.read();
                    let current_games = self.settings.lock().unwrap_or_else(|p| p.into_inner()).catalogue.clone();
                    let observed = observed.verify(&current_games, &self.setup.provider_setup_revision(integration),
                        |process| self.platform.alive(process).unwrap_or(false));
                    self.update_vehicles(|vehicles| vehicles.apply_observation(observed));
                }
            }
            let vehicle_id = self.runtime_vehicle_id(id);
            let catalogue = self
                .setup
                .integration_catalogue_metadata_for_vehicle(integration, vehicle_id.as_deref());
            let setup_matches = game["integrationSetup"]["providerSetupRevision"].as_str()
                == Some(revision.as_str());
            let observation = Observation {
                process: process.unwrap_or(None),
                supported,
                setup_ready: setup_matches
                    && game["integrationSetup"]["state"] == "ready"
                    && (!integration.starts_with("assetto-corsa")
                        || game["integrationSetup"]["verified"] == true),
                provider_setup_revision: revision.clone(),
                catalogue_revision: catalogue
                    .ok()
                    .and_then(|value| value["catalogueRevision"].as_str().map(str::to_owned)),
                vehicle_id,
                error,
            };
            let _ = self
                .contexts
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .observe(id, epoch, observation);
        }
    }
}

#[cfg(test)]
#[path = "game_context_runtime_tests.rs"]
mod tests;

include!("games/context/validation.rs");
