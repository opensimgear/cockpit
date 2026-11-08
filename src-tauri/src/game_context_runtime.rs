//! Process observation is native background work. UI reads do not authorize an
//! applied profile, start a provider, or turn an installation into live identity.
use super::*;
use crate::game_context::{ContextToken, GamePreparation, Observation, ProfileOwner, ValidationFailure};
#[path = "games/context/process_cache.rs"]
pub(super) mod process_cache;
#[path = "games/context/capture_workers.rs"]
mod capture_workers;
#[path = "games/context/vehicle_openers.rs"]
mod vehicle_openers;
#[path = "games/context/projection.rs"]
mod projection;
#[path = "games/context/setup_workers.rs"]
mod setup_workers;
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
        self.prepare_observed_profile_game_context(id, None)
    }

    pub(crate) fn prepare_returning_profile_game_context(
        &self,
        id: &str,
    ) -> Result<Option<PreparedGame>, String> {
        if !self.inner.platform.shares_background_processes() {
            return self.prepare_profile_game_context(id);
        }
        let registered = self.registered_profile_game(id)?;
        if crate::game_discovery::integration_key(&registered).is_none() {
            return Ok(None);
        }
        let Some((_, _, Ok(Some(process)))) = self.inner.profile_background_process(&registered) else {
            return Ok(None);
        };
        // The background context projection owns discovery and setup. A
        // returning reader must agree with both of its committed results.
        self.prepare_observed_profile_game_context(id, Some(&process))
    }

    fn prepare_observed_profile_game_context(
        &self,
        id: &str,
        returning: Option<&crate::game_discovery::ProcessIdentity>,
    ) -> Result<Option<PreparedGame>, String> {
        let Some(proof) = self
            .inner
            .contexts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .preparation(id)?
        else {
            return Ok(None);
        };
        if returning.is_some_and(|process| *process != proof.process) {
            return Ok(None);
        }
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
        if returning.is_some() && !matches!(self.inner.profile_background_process(&game),
            Some((_, _, Ok(Some(process)))) if process == proof.process)
        {
            return Ok(None);
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
    pub(crate) fn watch_profile_process_exit(&self, proof: &GamePreparation) -> Option<ExitWatch> {
        self.inner.platform.process_monitoring_supported()
            .then(|| ExitWatch::open(&proof.process).ok()).flatten()
    }

    /// Capture telemetry proof away from the live health path.
    pub(crate) fn profile_validation_task(
        &self, proof: GamePreparation, context: Option<ContextToken>, exit: Option<ExitEvidence>,
    ) -> impl FnMut() -> Result<Option<Instant>, String> + Send + 'static {
        let inner = self.inner.clone();
        move || {
            let observed = observe_preparation_inner(&inner, &proof, |game, expected| {
                inner.observe_profile_process(game, expected, exit.as_ref())
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
        let capture_wakes = inner.changes.vehicle_capture_wakes.clone();
        let open_wakes = Arc::new(crate::game_vehicles::CaptureWakes::default());
        inner.update_vehicles(|vehicles| {
            vehicles.watch_capture_lanes(capture_wakes.clone());
            vehicles.watch_openers(open_wakes.clone());
        });
        *self.observer.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(thread::spawn(move || {
            let polling_closed = Arc::new(AtomicBool::new(false));
            let polling = capture_workers::start(inner.clone(), polling_closed.clone());
            let openers = vehicle_openers::start(inner.clone(), polling_closed.clone(), &open_wakes);
            let projections = Arc::new(projection::Workers::default());
            let qualification = projections.start(&inner);
            let setup = setup_workers::start(&inner);
            let context_wake = &inner.changes.context_wake;
            let process_wake = &inner.changes.context_process_wake;
            let worker_wake = &inner.changes.context_worker_wake;
            let _ = worker_wake.bind_thread(thread::current());
            let mut process_cache = process_cache::Cache::default();
            let mut last_library = None;
            let mut last_catalogue_scope = None;
            while !inner.closed.load(Ordering::Acquire) {
                let observed = worker_wake.observed();
                let started = Instant::now();
                let library = context_wake.observed();
                let processes = process_wake.observed();
                let epoch = (library, processes);
                if last_library != Some(library) {
                    inner.setup.retain_background_scopes(&inner.vehicle_telemetry_games());
                    last_library = Some(library);
                }
                let catalogue_scope = (library, inner.changes.vehicle_scope_wake.observed());
                if last_catalogue_scope != Some(catalogue_scope) {
                    context_vehicle_catalogues::retain_current(&inner);
                    last_catalogue_scope = Some(catalogue_scope);
                }
                inner.collect_game_contexts(SetupRefresh::Cached, |games| {
                    inner.read_context_inventory(&mut process_cache, epoch, games,
                        |games| inner.read_context_processes(epoch, games))
                }, Some((&projections, epoch)));
                if inner.closed.load(Ordering::Acquire) { break; }
                if observed == worker_wake.observed() {
                    let live = inner.changes.context_process_live(library) && process_cache.is_current(epoch);
                    let maintenance = context_sdk_watch::maintenance_delay(&inner, live, started);
                    worker_wake.wait_scheduled(observed, maintenance);
                }
            }
            polling_closed.store(true, Ordering::Release);
            capture_wakes.notify_all();
            open_wakes.notify_all();
            projections.stop();
            inner.changes.setup_watch_wakes.notify_all();
            for worker in polling.into_iter().chain(openers).chain(qualification).chain(setup) {
                let _ = worker.join();
            }
            inner.update_vehicles(|vehicles| vehicles.stop_openers());
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

}

#[cfg(test)]
#[path = "game_context_runtime_tests.rs"]
mod tests;

include!("games/context/validation.rs");
