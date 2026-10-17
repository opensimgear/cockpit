//! Process observation is native background work. UI reads do not authorize an
//! applied profile, start a provider, or turn an installation into live identity.
use super::*;
use crate::game_context::{ContextToken, GamePreparation, Observation, ProfileOwner};
#[derive(Clone, Debug)]
pub(crate) struct PreparedGame {
    pub proof: GamePreparation,
    pub connection: Value,
    pub catalogue: Value,
    pub aircraft_module: Option<String>,
}
impl GamesRuntime {
    pub(crate) fn prepare_profile_game_context(
        &self,
        id: &str,
    ) -> Result<Option<PreparedGame>, String> {
        self.inner.guard_installation_metadata()?;
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
        let (connection, revision, aircraft_module) =
            self.inner.setup.provider_connection_snapshot(id)?;
        let catalogue = self.inner.setup.resource_integration_catalogue(id)?;
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
            proof,
            connection,
            catalogue,
            aircraft_module,
        }))
    }
    pub(crate) fn validate_game_preparation(&self, proof: &GamePreparation) -> Result<(), String> {
        let result: Result<(), String> = (|| {
            self.inner.guard_installation_metadata()?;
            self.inner
                .contexts
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .validate_preparation(proof)?;
            let game = self
                .inner
                .settings
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .catalogue
                .iter()
                .find(|game| game["id"] == proof.game_id)
                .cloned()
                .ok_or("The selected game installation changed.")?;
            if self.inner.platform.find(&game)?.as_ref() != Some(&proof.process)
                || !self.inner.platform.alive(&proof.process)?
            {
                return Err(
                    "The prepared game process exited, was replaced or became ambiguous.".into(),
                );
            }
            if self.inner.setup.provider_setup_revision(&proof.game_id)
                != proof.provider_setup_revision
                || self
                    .inner
                    .setup
                    .current_catalogue_revision(&proof.game_id)
                    .as_ref()
                    != Some(&proof.catalogue_revision)
            {
                return Err("The prepared provider setup or catalogue changed.".into());
            }
            Ok(())
        })();
        if let Err(error) = &result {
            self.inner
                .contexts
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .invalidate_preparation(proof, error.clone());
        }
        result
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
            while !inner.closed.load(Ordering::Acquire) {
                inner.refresh_game_contexts(false);
                for _ in 0..5 {
                    if inner.closed.load(Ordering::Acquire) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }));
    }
    pub(super) fn decorate_game_contexts(&self, games: &mut [Value]) {
        let contexts = self
            .inner
            .contexts
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        for game in games {
            let id = game["id"].as_str().unwrap_or("");
            let mut context = contexts.view(id);
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
        self.inner.setup.resource_integration_catalogue(id)
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
        self.inner.refresh_game_contexts(true);
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
    pub(super) fn refresh_game_contexts(&self, force_setup: bool) {
        let installation_metadata = self.guard_installation_metadata();
        let mut games = self
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .catalogue
            .clone();
        let mut checked = self
            .context_setup_check
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if force_setup || checked.is_none_or(|last| last.elapsed() >= Duration::from_secs(2)) {
            self.setup.refresh(&games);
            *checked = Some(Instant::now());
        }
        drop(checked);
        self.setup.decorate(&mut games);
        for game in games {
            let Some(id) = game["id"].as_str() else {
                continue;
            };
            let catalogue = self.setup.resource_integration_catalogue(id);
            let revision = catalogue
                .as_ref()
                .ok()
                .and_then(|value| value["providerSetupRevision"].as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| self.setup.provider_setup_revision(id));
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
                let process = self.platform.find(&game)?;
                match process {
                    Some(process) if self.platform.alive(&process)? => Ok(Some(process)),
                    _ => Ok(None),
                }
            })(
            );
            let error = process.as_ref().err().cloned();
            let setup_matches = game["integrationSetup"]["providerSetupRevision"].as_str()
                == Some(revision.as_str());
            let observation = Observation {
                process: process.unwrap_or(None),
                supported,
                setup_ready: setup_matches
                    && game["integrationSetup"]["state"] == "ready"
                    && (id != "assetto-corsa-rally"
                        || game["integrationSetup"]["verified"] == true),
                provider_setup_revision: revision,
                catalogue_revision: catalogue
                    .ok()
                    .and_then(|value| value["catalogueRevision"].as_str().map(str::to_owned)),
                error,
            };
            let _ = self
                .contexts
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .observe(id, observation);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_discovery::ProcessIdentity;
    use std::collections::BTreeMap;
    struct Scratch {
        path: PathBuf,
        _directory: tempfile::TempDir,
    }
    impl Scratch {
        fn new() -> Self {
            let directory = crate::test_directory();
            Self {
                path: directory.path().to_owned(),
                _directory: directory,
            }
        }
    }
    struct FakePlatform {
        process: Mutex<Option<ProcessIdentity>>,
        failed: AtomicBool,
    }
    impl FakePlatform {
        fn new() -> Self {
            Self {
                process: Mutex::new(Some(ProcessIdentity {
                    pid: 42,
                    path: "/fixture/X-Plane.exe".into(),
                    epoch: 10,
                })),
                failed: AtomicBool::new(false),
            }
        }
    }
    impl Platform for FakePlatform {
        fn integration_setup_supported(&self) -> bool {
            true
        }
        fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
            vec![
                json!({"id":"x-plane-12","name":"X-Plane 12","provider":"x-plane","installed":true,"executable":"/fixture/X-Plane.exe","launchSupported":true,"processMonitoringSupported":true}),
            ]
        }
        fn validate(&self, _: &str, path: &str) -> Result<(), String> {
            if path == "/fixture/X-Plane.exe" {
                Ok(())
            } else {
                Err("Wrong executable".into())
            }
        }
        fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> {
            Err("Unexpected launch".into())
        }
        fn find(&self, _: &Value) -> Result<Option<ProcessIdentity>, String> {
            if self.failed.load(Ordering::Acquire) {
                Err("Process inspection failed".into())
            } else {
                Ok(self.process.lock().unwrap().clone())
            }
        }
        fn alive(&self, identity: &ProcessIdentity) -> Result<bool, String> {
            Ok(self.process.lock().unwrap().as_ref() == Some(identity))
        }
    }
    fn owner(game: Option<&str>) -> ProfileOwner {
        ProfileOwner {
            session: "9f4d5357-4592-4111-a8ec-44ff38160d87".into(),
            profile_id: "7b10d24e-7610-4083-a7d0-21ea12b24d23".into(),
            applied_revision: 1,
            run_generation: 1,
            game_id: game.map(str::to_owned),
        }
    }
    fn runtime(scratch: &Scratch) -> (GamesRuntime, Arc<FakePlatform>, Arc<Mutex<Vec<String>>>) {
        let platform = Arc::new(FakePlatform::new());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let unexpected = calls.clone();
        let panel: CleanupIo = Arc::new(move |request| {
            unexpected
                .lock()
                .unwrap()
                .push(request["command"].as_str().unwrap_or("").to_owned());
            panic!("Context metadata must never call cleanup or a retired provider: {request}");
        });
        let runtime = GamesRuntime::new(panel, Paths::in_directory(&scratch.path), platform.clone());
        let revision = runtime.inner.setup.provider_setup_revision("x-plane-12");
        runtime.request(&json!({"command":"game_integration_settings_save","gameId":"x-plane-12","expectedSetupRevision":revision,"connection":crate::xplane::defaults(12)})).unwrap();
        (runtime, platform, calls)
    }
    #[test]
    fn native_process_does_not_activate_without_matching_applied_owner() {
        let scratch = Scratch::new();
        let (runtime, _, _) = runtime(&scratch);
        assert!(runtime.profile_game_context("x-plane-12").is_err());
        runtime
            .set_profile_context_owner(Some(owner(Some("dcs"))))
            .unwrap();
        assert!(runtime.profile_game_context("x-plane-12").is_err());
        runtime
            .set_profile_context_owner(Some(owner(None)))
            .unwrap();
        assert!(runtime.profile_game_context("x-plane-12").is_err());
        runtime
            .set_profile_context_owner(Some(owner(Some("x-plane-12"))))
            .unwrap();
        let token = runtime.profile_game_context("x-plane-12").unwrap();
        assert_eq!(token.process.pid, 42);
        assert_eq!(token.process.epoch, 10);
        assert!(runtime.validate_profile_game_context(&token).is_ok());
        runtime.shutdown();
    }
    #[test]
    fn preparation_is_native_read_only_and_admission_requires_matching_applied_owner() {
        let scratch = Scratch::new();
        let (runtime, _, calls) = runtime(&scratch);
        let prepared = runtime
            .prepare_profile_game_context("x-plane-12")
            .unwrap()
            .unwrap();
        assert_eq!(prepared.proof.process.pid, 42);
        assert_eq!(prepared.connection["version"], 12);
        assert!(runtime.profile_game_context("x-plane-12").is_err());
        assert!(runtime
            .admit_prepared_game_context(&prepared.proof)
            .is_err());
        runtime
            .set_profile_context_owner(Some(owner(Some("dcs"))))
            .unwrap();
        assert!(runtime
            .admit_prepared_game_context(&prepared.proof)
            .is_err());
        runtime
            .set_profile_context_owner(Some(owner(Some("x-plane-12"))))
            .unwrap();
        let token = runtime
            .admit_prepared_game_context(&prepared.proof)
            .unwrap();
        assert!(runtime
            .validate_admitted_game_context(&token, &prepared.proof)
            .is_ok());
        assert!(calls.lock().unwrap().is_empty());
        runtime.shutdown();
    }
    #[test]
    fn preparation_loss_revokes_before_dispatch_without_waiting_for_observer() {
        let scratch = Scratch::new();
        let (runtime, platform, _) = runtime(&scratch);
        let prepared = runtime
            .prepare_profile_game_context("x-plane-12")
            .unwrap()
            .unwrap();
        runtime
            .set_profile_context_owner(Some(owner(Some("x-plane-12"))))
            .unwrap();
        let token = runtime
            .admit_prepared_game_context(&prepared.proof)
            .unwrap();
        platform.process.lock().unwrap().as_mut().unwrap().epoch += 1;
        assert!(runtime
            .validate_admitted_game_context(&token, &prepared.proof)
            .is_err());
        assert!(runtime
            .inner
            .contexts
            .lock()
            .unwrap()
            .token("x-plane-12")
            .is_err());
        let fresh = runtime
            .prepare_profile_game_context("x-plane-12")
            .unwrap()
            .unwrap();
        assert_ne!(fresh.proof.session, prepared.proof.session);
        let revision = runtime.inner.setup.provider_setup_revision("x-plane-12");
        let mut connection = crate::xplane::defaults(12);
        connection["freshnessMs"] = json!(2000);
        runtime.request(&json!({"command":"game_integration_settings_save","gameId":"x-plane-12","expectedSetupRevision":revision,"connection":connection})).unwrap();
        assert!(runtime.validate_game_preparation(&fresh.proof).is_err());
        *platform.process.lock().unwrap() = None;
        assert!(runtime
            .prepare_profile_game_context("x-plane-12")
            .unwrap()
            .is_none());
        runtime.shutdown();
    }
    #[test]
    fn late_old_stage_failure_preserves_the_admitted_replacement_process_and_owner() {
        let scratch = Scratch::new();
        let (runtime, platform, _) = runtime(&scratch);
        let old = runtime
            .prepare_profile_game_context("x-plane-12")
            .unwrap()
            .unwrap();
        runtime
            .set_profile_context_owner(Some(owner(Some("x-plane-12"))))
            .unwrap();
        platform.process.lock().unwrap().as_mut().unwrap().epoch += 1;
        let current = runtime
            .prepare_profile_game_context("x-plane-12")
            .unwrap()
            .unwrap();
        let mut next = owner(Some("x-plane-12"));
        next.run_generation += 1;
        runtime
            .set_profile_context_owner(Some(next.clone()))
            .unwrap();
        let token = runtime.admit_prepared_game_context(&current.proof).unwrap();
        assert_eq!(token.owner, next);
        assert!(runtime.validate_game_preparation(&old.proof).is_err());
        runtime
            .validate_admitted_game_context(&token, &current.proof)
            .unwrap();
        assert_eq!(runtime.profile_game_context("x-plane-12").unwrap(), token);
        *platform.process.lock().unwrap() = None;
        assert!(runtime.validate_game_preparation(&current.proof).is_err());
        assert!(runtime
            .inner
            .contexts
            .lock()
            .unwrap()
            .token("x-plane-12")
            .is_err());
        runtime.shutdown();
    }
    #[test]
    fn process_replacement_and_exit_withdraw_context_without_retired_provider_calls() {
        let scratch = Scratch::new();
        let (runtime, platform, calls) = runtime(&scratch);
        runtime
            .set_profile_context_owner(Some(owner(Some("x-plane-12"))))
            .unwrap();
        let token = runtime.profile_game_context("x-plane-12").unwrap();
        platform.process.lock().unwrap().as_mut().unwrap().epoch += 1;
        assert!(runtime.validate_profile_game_context(&token).is_err());
        let replacement = runtime.profile_game_context("x-plane-12").unwrap();
        assert_ne!(replacement.session, token.session);
        *platform.process.lock().unwrap() = None;
        assert!(runtime.validate_profile_game_context(&replacement).is_err());
        assert!(calls.lock().unwrap().is_empty());
        runtime.shutdown();
    }
    #[test]
    fn settings_save_synchronously_revokes_context_without_background_or_legacy_state() {
        let scratch = Scratch::new();
        let (runtime, _, calls) = runtime(&scratch);
        runtime
            .set_profile_context_owner(Some(owner(Some("x-plane-12"))))
            .unwrap();
        let token = runtime.profile_game_context("x-plane-12").unwrap();
        let mut connection = crate::xplane::defaults(12);
        connection["freshnessMs"] = json!(2000);
        runtime.request(&json!({"command":"game_integration_settings_save","gameId":"x-plane-12","expectedSetupRevision":token.provider_setup_revision,"connection":connection})).unwrap();
        assert!(runtime
            .inner
            .contexts
            .lock()
            .unwrap()
            .validate(&token)
            .is_err());
        assert_ne!(
            runtime
                .profile_game_context("x-plane-12")
                .unwrap()
                .provider_setup_revision,
            token.provider_setup_revision
        );
        assert!(calls.lock().unwrap().is_empty());
        runtime.shutdown();
    }
    #[test]
    fn inspection_failure_and_pause_revoke_authority_without_legacy_output_stop() {
        let scratch = Scratch::new();
        let (runtime, platform, calls) = runtime(&scratch);
        runtime
            .set_profile_context_owner(Some(owner(Some("x-plane-12"))))
            .unwrap();
        let token = runtime.profile_game_context("x-plane-12").unwrap();
        platform.failed.store(true, Ordering::Release);
        assert!(runtime.validate_profile_game_context(&token).is_err());
        platform.failed.store(false, Ordering::Release);
        let fresh = runtime.profile_game_context("x-plane-12").unwrap();
        runtime.set_profile_context_owner(None).unwrap();
        assert!(runtime.validate_profile_game_context(&fresh).is_err());
        assert!(calls.lock().unwrap().is_empty());
        runtime.shutdown();
    }
}
