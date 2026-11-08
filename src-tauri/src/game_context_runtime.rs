//! Process observation is native background work. UI reads do not authorize an
//! applied profile, start a provider, or turn an installation into live identity.
use super::*;
use crate::game_context::{ContextToken, GamePreparation, Observation, ProfileOwner, ValidationFailure};
#[path = "games/context/process_cache.rs"]
pub(super) mod process_cache;
#[derive(Clone, Debug)]
pub(crate) struct PreparedGame {
    pub proof: GamePreparation,
    pub integration_key: String,
    pub connection: Value,
    pub catalogue: Value,
    pub aircraft_module: Option<String>,
}
enum SetupRefresh {
    Cached,
    Explicit,
    Library,
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
        self.validate_game_preparation_strict(proof).map_err(Into::into)
    }
    fn validate_game_preparation_strict(&self, proof: &GamePreparation) -> Result<(), ValidationFailure> {
        match observe_preparation_inner(&self.inner, proof, |game, expected| self.inner.exact_process_current(game, expected)) {
            Ok(ProcessEvidence::Verified(_)) => Ok(()),
            Ok(ProcessEvidence::Unavailable(error)) => Err(ValidationFailure::ProcessUnavailable(error)),
            Err(error) => {
                self.inner.contexts.lock().unwrap_or_else(|p| p.into_inner())
                    .invalidate_preparation(proof, error.clone());
                Err(ValidationFailure::Invalid(error))
            }
        }
    }
    /// Capture identity for strict validation away from the live health path.
    pub(crate) fn profile_validation_task(
        &self, proof: GamePreparation, context: Option<ContextToken>,
    ) -> impl FnMut() -> Result<Option<Instant>, String> + Send + 'static {
        let inner = self.inner.clone();
        move || {
            let observed = observe_preparation_inner(&inner, &proof, |game, expected| {
                inner.observe_profile_process(game, expected)
            })?;
            if let Some(token) = &context {
                if !token.matches_preparation(&proof) {
                    return Err("The delivered game context differs from its preparation.".into());
                }
                inner.contexts.lock().unwrap_or_else(|p| p.into_inner()).validate(token)?;
            }
            Ok(match observed {
                ProcessEvidence::Verified(at) => Some(at),
                ProcessEvidence::Unavailable(_) => None,
            })
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
    ) -> Result<(), ValidationFailure> {
        let result = self.validate_game_preparation_strict(proof);
        if matches!(result, Err(ValidationFailure::Invalid(_))) { return result; }
        if !token.matches_preparation(proof) {
            return Err(ValidationFailure::Invalid("The delivered game context differs from its preparation.".into()));
        }
        self.inner
            .contexts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .validate(token)
            .map_err(ValidationFailure::Invalid)?;
        result
    }
    pub(super) fn start_context_observer(&self) {
        if !self.inner.platform.integration_setup_supported() {
            return;
        }
        let inner = self.inner.clone();
        let capture_wake = inner.changes.vehicle_capture_wake.clone();
        inner.update_vehicles(|vehicles| vehicles.watch_captures(capture_wake.clone()));
        *self.observer.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(thread::spawn(move || {
            let polling_closed = Arc::new(AtomicBool::new(false));
            let polling_inner = inner.clone();
            let polling_stop = polling_closed.clone();
            let worker_wake = capture_wake.clone();
            // Setup inspection and catalogue parsing can exceed a freshness
            // deadline. Handle capture, scope changes and exit independently.
            let polling = thread::spawn(move || {
                let _ = worker_wake.bind_thread(thread::current());
                let mut drained = None;
                let mut scope = 0;
                while !polling_inner.closed.load(Ordering::Acquire)
                    && !polling_stop.load(Ordering::Acquire)
                {
                    let observed = worker_wake.observed();
                    let current_scope = polling_inner.changes.vehicle_scope_wake.observed();
                    if Some(observed) != drained || observed == u64::MAX || matches!(polling_inner.vehicles.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
                        .capture_deadline(), Some(deadline) if Instant::now() >= deadline)
                    {
                        polling_inner.poll_vehicle_telemetry(current_scope != scope || current_scope == u64::MAX);
                    }
                    scope = current_scope;
                    drained = Some(observed);
                    let delay = polling_inner.vehicles.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
                        .capture_deadline().map(|at| at.saturating_duration_since(Instant::now()));
                    worker_wake.wait_scheduled(observed, delay);
                }
            });
            let context_wake = &inner.changes.context_wake;
            let process_wake = &inner.changes.context_process_wake;
            let worker_wake = &inner.changes.context_worker_wake;
            let _ = worker_wake.bind_thread(thread::current());
            let mut process_cache = process_cache::Cache::default();
            let mut last_library = None;
            let mut sdk_watches = context_sdk_watch::SdkWatches::default();
            let mut assetto_watches = context_assetto_watch::AssettoWatches::default();
            let mut dcs_watch = context_dcs_watch::DcsWatch::default();
            while !inner.closed.load(Ordering::Acquire) {
                let observed = worker_wake.observed();
                let started = Instant::now();
                let library = context_wake.observed();
                let processes = process_wake.observed();
                let epoch = (library, processes);
                let sdk_changed = sdk_watches.refresh(&inner);
                assetto_watches.refresh(&inner);
                dcs_watch.refresh(&inner);
                let scope = inner.changes.context_process_live(library).then_some(epoch);
                let setup = if last_library == Some(library) { SetupRefresh::Cached } else { SetupRefresh::Library };
                let setup_current = inner.refresh_game_contexts_with_inventory(setup, |games| {
                    let rows = process_cache.read_using(games, scope.filter(|_| inner.current_process_epoch() == Some(epoch)),
                        |games| inner.read_context_processes(epoch, games));
                    let snapshot = if inner.current_process_epoch() == Some(epoch) { process_cache.snapshot() } else { None };
                    *inner.context_process_inventory.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = snapshot;
                    rows
                });
                if sdk_changed { context_sdk_watch::publish_change(&inner); }
                if setup_current { last_library = Some(library); }
                if inner.closed.load(Ordering::Acquire) { break; }
                if observed == worker_wake.observed() {
                    let live = inner.changes.context_process_live(library) && process_cache.is_current(epoch);
                    let watch_retry = [sdk_watches.retry_at(), assetto_watches.retry_at(), dcs_watch.retry_at()]
                        .into_iter().flatten().min().map(|at| at.saturating_duration_since(Instant::now()));
                    let maintenance = context_sdk_watch::maintenance_delay(&inner, live, started);
                    worker_wake.wait_scheduled(observed, maintenance.into_iter().chain(watch_retry).min());
                }
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
        let contexts = self.inner.contexts.lock().unwrap_or_else(|p| p.into_inner());
        contexts.ensure_observed_process(id)?;
        contexts.token(id)
    }
    /// Source attachment, telemetry consumption and commands must use this native
    /// check rather than a browser's cached contextSession label.
    pub(crate) fn validate_profile_game_context(&self, token: &ContextToken) -> Result<(), String> {
        self.inner.refresh_game_contexts(true);
        let contexts = self.inner.contexts.lock().unwrap_or_else(|p| p.into_inner());
        contexts.ensure_observed_process(&token.game_id)?;
        contexts.validate(token)
    }
}
impl Inner {
    // Reader destruction may unsubscribe in bounded batches and sleep. Retire
    // under the mutex, then drop its owned batch only after releasing the guard.
    fn update_vehicles<R>(&self, update: impl FnOnce(&mut crate::game_vehicles::Tracker) -> R) -> R {
        let (value, retired) = { let mut vehicles = self.vehicles.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let value = update(&mut vehicles); (value, vehicles.take_retired()) };
        drop(retired);
        value
    }
    fn vehicle_telemetry_games(&self) -> Vec<Value> {
        let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        if settings.metadata_error.is_some() {
            return vec![];
        }
        settings.catalogue.iter()
            .filter(|game| crate::game_discovery::integration_key(game).is_some())
            .cloned()
            .collect()
    }
    fn poll_vehicle_telemetry(&self, recheck_scope: bool) {
        let candidates = self.vehicles.lock().unwrap_or_else(|p| p.into_inner())
            .scoped_capture_candidates(recheck_scope);
        if candidates.is_empty() {
            return;
        }
        let revision = |games: &[Value], id: &str| {
            games.iter().find(|game| game["id"] == id)
                .map_or_else(String::new, |game| self.setup.vehicle_setup_revision(game))
        };
        let games = self.vehicle_telemetry_games();
        let verified = candidates.into_iter()
            .map(|candidate| candidate.verify(&games, |id| revision(&games, id), |_| None).without_renewal())
            .collect();
        let polls = self.update_vehicles(|vehicles| vehicles.begin_polls(verified));
        for poll in polls {
            let result = poll.read();
            let games = self.vehicle_telemetry_games();
            let result = result.verify(&games, |id| revision(&games, id), |_| None);
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
        let setup = if force_setup { SetupRefresh::Explicit } else { SetupRefresh::Cached };
        self.refresh_game_contexts_with_inventory(setup, |games| self.platform.integration_processes(games));
    }

    fn refresh_game_contexts_with_inventory(
        &self,
        setup: SetupRefresh,
        inventory: impl FnOnce(&[&Value]) -> Vec<crate::game_discovery::ProcessLookup>,
    ) -> bool {
        let epoch = match self
            .contexts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .begin_observation()
        {
            Ok(epoch) => epoch,
            Err(_) => return false,
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
        match setup {
            SetupRefresh::Cached => {}
            SetupRefresh::Explicit => self.setup.refresh(&games),
            SetupRefresh::Library => {
                // A newer observation cannot publish an unchecked library
                // baseline while explicit preparation owns setup admission.
                if !self.setup.refresh_background(&games) { return false; }
            }
        }
        self.setup.decorate_source(&mut games);
        let admission: Vec<_> = games.iter().map(|game| {
            let supported = self.platform.integration_setup_supported()
                && game["processMonitoringSupported"]
                    .as_bool()
                    .unwrap_or(game["launchSupported"] == true);
            let eligible: Result<bool, String> = (|| {
                installation_metadata.as_ref().map_err(Clone::clone)?;
                if !supported || game["installed"] != true {
                    return Ok(false);
                }
                self.platform.validate(
                    game["id"].as_str().ok_or("Choose a game identity.")?,
                    game["executable"]
                        .as_str()
                        .ok_or("Choose a game executable before process verification.")?,
                )?;
                Ok(true)
            })();
            (supported, eligible)
        }).collect();
        let selected: Vec<_> = games.iter().zip(&admission)
            .filter_map(|(game, (_, eligible))| matches!(eligible, Ok(true)).then_some(game)).collect();
        let mut processes = inventory(&selected).into_iter();
        for (game, (supported, eligible)) in games.iter().zip(admission) {
            use crate::game_discovery::ProcessLookupError;
            let eligible_for_retention = matches!(eligible, Ok(true));
            let process = eligible.map_err(ProcessLookupError::Unavailable).and_then(|eligible| {
                if !eligible { return Ok(None); }
                let process = processes.next().ok_or_else(||
                    ProcessLookupError::Unavailable("Game process inventory was incomplete.".into()))??;
                match process {
                    Some(process) if self.platform.alive(&process).map_err(ProcessLookupError::Unavailable)? => Ok(Some(process)),
                    _ => Ok(None),
                }
            });
            let Some(id) = game["id"].as_str() else { continue; };
            let Some(integration) = crate::game_discovery::integration_key(game) else { continue; };
            let revision = self.setup.provider_setup_revision(integration);
            let lookup_unavailable = eligible_for_retention && matches!(process, Err(ProcessLookupError::Unavailable(_)));
            let error = process.as_ref().err().map(ToString::to_string);
            let connection = self
                .setup
                .provider_connection_snapshot(integration)
                .ok()
                .map(|(connection, _, _)| connection);
            if let Some(connection) = connection {
                let work = self.update_vehicles(|vehicles| vehicles.begin_inventory_observation(&game, &connection, process.clone(), &revision,
                        supported && game["integrationSetup"]["state"] == "ready", epoch));
                if let Some(work) = work {
                    let observed = work.read();
                    let current_games = self.settings.lock().unwrap_or_else(|p| p.into_inner()).catalogue.clone();
                    let observed = observed.verify(&current_games, &self.setup.provider_setup_revision(integration),
                        |process| self.platform.alive(process));
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
                installation: preparation_installation(game),
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
                .observe(id, epoch, observation, lookup_unavailable);
        }
        true
    }
}

#[cfg(test)]
#[path = "game_context_runtime_tests.rs"]
mod tests;

include!("games/context/validation.rs");
