//! Process observation is native background work. UI reads do not authorize an
//! applied profile, start a provider, or turn an installation into live identity.
use super::*;
use crate::game_context::{ContextToken, Observation, ProfileOwner};
impl GamesRuntime {
    pub(crate) fn guard_unified_integration_request(&self, request: &Value) -> Result<(), String> {
        if request["command"] == "game_profile_save"
            && !request["gameProfile"]["integration"].is_null()
        {
            return Err("Integration bindings now belong to the unified profile graph. Save this game hardware profile without an integration capture.".into());
        }
        if matches!(
            request["command"].as_str(),
            Some("game_activate" | "game_launch")
        ) {
            let settings = self
                .inner
                .settings
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let id = request["profileId"].as_str().or_else(|| {
                request["gameId"]
                    .as_str()
                    .and_then(|game| settings.saved.defaults.get(game).map(String::as_str))
            });
            if id
                .and_then(|id| settings.saved.profiles.get(id))
                .is_some_and(|profile| !profile["integration"].is_null())
            {
                return Err("This captured game integration is retired. Keep provider settings in game details and author its bindings in a unified profile before activation.".into());
            }
        }
        Ok(())
    }
    pub(super) fn start_context_observer(&self) {
        if !self.inner.platform.integration_setup_supported() {
            return;
        }
        let inner = self.inner.clone();
        *self.observer.lock().unwrap_or_else(|p| p.into_inner()) = Some(thread::spawn(move || {
            while !inner.closed.load(Ordering::Acquire) {
                inner.refresh_game_contexts(false);
                inner.enforce_integration_context();
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
        self.inner.enforce_integration_context();
        Ok(())
    }
    pub(crate) fn profile_game_context(&self, id: &str) -> Result<ContextToken, String> {
        self.inner.refresh_game_contexts(true);
        self.inner.enforce_integration_context();
        self.inner
            .contexts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .token(id)
    }
    pub(crate) fn admit_profile_integration(&self, token: &ContextToken) -> Result<(), String> {
        self.inner.refresh_game_contexts(true);
        self.inner.enforce_integration_context();
        let response =
            (self.inner.panel)(json!({"command":"integration_get_state","clientId":CLIENT}));
        let existing = self
            .inner
            .admitted_context
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if response["state"]["integration"]["running"] == true && existing.as_ref() != Some(token) {
            return Err(
                "Stop the existing profile-owned integration before admitting a replacement."
                    .into(),
            );
        }
        let contexts = self
            .inner
            .contexts
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        contexts.validate(token)?;
        *self
            .inner
            .admitted_context
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(token.clone());
        Ok(())
    }
    /// Source attachment, telemetry consumption and commands must use this native
    /// check rather than a browser's cached contextSession label.
    pub(crate) fn validate_profile_game_context(&self, token: &ContextToken) -> Result<(), String> {
        self.inner.refresh_game_contexts(true);
        self.inner.enforce_integration_context();
        self.inner
            .contexts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .validate(token)
    }
}

impl Inner {
    pub(super) fn refresh_game_contexts(&self, force_setup: bool) {
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
        self.setup.decorate(&mut games, &Value::Null, &Value::Null);
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
    pub(super) fn enforce_integration_context(&self) {
        let response = (self.panel)(json!({"command":"integration_get_state","clientId":CLIENT}));
        let integration = &response["state"]["integration"];
        if integration["running"] == true {
            let game = match integration["config"]["document"]["provider"].as_str() {
                Some("dcs-bios") => Some("dcs"),
                Some("assetto-corsa-rally") => Some("assetto-corsa-rally"),
                Some("x-plane") => match integration["config"]["connection"]["version"].as_u64() {
                    Some(11) => Some("x-plane-11"),
                    Some(12) => Some("x-plane-12"),
                    _ => None,
                },
                _ => None,
            };
            let admitted = self
                .admitted_context
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone();
            let current = admitted.as_ref().is_some_and(|token| {
                game == Some(token.game_id.as_str())
                    && self
                        .contexts
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .validate(token)
                        .is_ok()
            });
            if !current {
                let _ = (self.panel)(json!({"command":"integration_stop","clientId":CLIENT}));
                *self
                    .admitted_context
                    .lock()
                    .unwrap_or_else(|p| p.into_inner()) = None;
            }
        }
        if response["state"]["dcsBios"]["running"] == true {
            let _ = (self.panel)(json!({"command":"dcs_stop","clientId":CLIENT}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_discovery::ProcessIdentity;
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("osg-game-context-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
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
    fn runtime(
        scratch: &Scratch,
    ) -> (
        GamesRuntime,
        Arc<FakePlatform>,
        Arc<Mutex<Value>>,
        Arc<Mutex<Vec<String>>>,
    ) {
        let platform = Arc::new(FakePlatform::new());
        let state = Arc::new(Mutex::new(
            json!({"running":false,"config":{"document":{"provider":"x-plane"},"connection":{"version":12}}}),
        ));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let worker_state = state.clone();
        let worker_calls = calls.clone();
        let panel: Panel = Arc::new(move |request| {
            let command = request["command"].as_str().unwrap_or("").to_owned();
            worker_calls.lock().unwrap().push(command.clone());
            if command == "integration_stop" {
                worker_state.lock().unwrap()["running"] = json!(false);
            }
            json!({"ok":true,"state":{"integration":worker_state.lock().unwrap().clone(),"dcsBios":{"running":false}}})
        });
        let runtime = GamesRuntime::new(panel, scratch.0.join("games.json"), platform.clone());
        let revision = runtime.inner.setup.provider_setup_revision("x-plane-12");
        runtime.request(&json!({"command":"game_integration_settings_save","gameId":"x-plane-12","expectedSetupRevision":revision,"connection":crate::xplane::defaults(12)})).unwrap();
        (runtime, platform, state, calls)
    }
    #[test]
    fn native_process_does_not_activate_without_matching_applied_owner() {
        let scratch = Scratch::new();
        let (runtime, _, _, _) = runtime(&scratch);
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
    fn process_replacement_and_exit_stop_only_the_old_integration_branch() {
        let scratch = Scratch::new();
        let (runtime, platform, state, calls) = runtime(&scratch);
        runtime
            .set_profile_context_owner(Some(owner(Some("x-plane-12"))))
            .unwrap();
        let token = runtime.profile_game_context("x-plane-12").unwrap();
        runtime.admit_profile_integration(&token).unwrap();
        state.lock().unwrap()["running"] = json!(true);
        platform.process.lock().unwrap().as_mut().unwrap().epoch += 1;
        assert!(runtime.validate_profile_game_context(&token).is_err());
        assert_eq!(state.lock().unwrap()["running"], false);
        let replacement = runtime.profile_game_context("x-plane-12").unwrap();
        assert_ne!(replacement.session, token.session);
        runtime.admit_profile_integration(&replacement).unwrap();
        state.lock().unwrap()["running"] = json!(true);
        *platform.process.lock().unwrap() = None;
        assert!(runtime.validate_profile_game_context(&replacement).is_err());
        assert_eq!(state.lock().unwrap()["running"], false);
        assert!(
            calls
                .lock()
                .unwrap()
                .iter()
                .filter(|call| call.as_str() == "integration_stop")
                .count()
                >= 2
        );
        assert!(!calls
            .lock()
            .unwrap()
            .iter()
            .any(|call| call.starts_with("_game_mapping_")));
        runtime.shutdown();
    }
    #[test]
    fn settings_save_synchronously_revokes_captured_context_without_waiting_for_background_scan() {
        let scratch = Scratch::new();
        let (runtime, _, state, _) = runtime(&scratch);
        runtime
            .set_profile_context_owner(Some(owner(Some("x-plane-12"))))
            .unwrap();
        let token = runtime.profile_game_context("x-plane-12").unwrap();
        runtime.admit_profile_integration(&token).unwrap();
        state.lock().unwrap()["running"] = json!(true);
        let mut connection = crate::xplane::defaults(12);
        connection["freshnessMs"] = json!(2000);
        runtime.request(&json!({"command":"game_integration_settings_save","gameId":"x-plane-12","expectedSetupRevision":token.provider_setup_revision,"connection":connection})).unwrap();
        assert_eq!(state.lock().unwrap()["running"], false);
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
        runtime.shutdown();
    }
    #[test]
    fn inspection_errors_and_pause_revoke_live_telemetry_authority() {
        let scratch = Scratch::new();
        let (runtime, platform, state, _) = runtime(&scratch);
        runtime
            .set_profile_context_owner(Some(owner(Some("x-plane-12"))))
            .unwrap();
        let token = runtime.profile_game_context("x-plane-12").unwrap();
        runtime.admit_profile_integration(&token).unwrap();
        state.lock().unwrap()["running"] = json!(true);
        platform.failed.store(true, Ordering::Release);
        assert!(runtime.validate_profile_game_context(&token).is_err());
        assert_eq!(state.lock().unwrap()["running"], false);
        platform.failed.store(false, Ordering::Release);
        let fresh = runtime.profile_game_context("x-plane-12").unwrap();
        runtime.admit_profile_integration(&fresh).unwrap();
        state.lock().unwrap()["running"] = json!(true);
        runtime.set_profile_context_owner(None).unwrap();
        assert_eq!(state.lock().unwrap()["running"], false);
        assert!(runtime.validate_profile_game_context(&fresh).is_err());
        runtime.shutdown();
    }
}
