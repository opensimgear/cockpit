use super::*;

use crate::game_discovery::ProcessIdentity;
use std::collections::BTreeMap;

#[test]
fn context_inventory_reuses_only_successful_results_in_the_live_scope() {
    let platform = FakePlatform::new();
    let games = platform.catalogue(&BTreeMap::new());
    let selected = [&games[0]];
    let mut cache = process_cache::Cache::default();
    let scope = (5, 10);
    assert!(cache.read(&platform, &selected, Some(scope))[0].is_ok());
    let reads = platform.reads.load(Ordering::SeqCst);
    assert!(cache.is_current(scope));
    assert!(cache.read(&platform, &selected, Some(scope))[0].is_ok());
    assert_eq!(platform.reads.load(Ordering::SeqCst), reads);
    let mut replacement = games[0].clone();
    replacement["executable"] = json!("/replacement/X-Plane.exe");
    assert!(cache.read(&platform, &[&replacement], Some(scope))[0].is_ok());
    assert!(platform.reads.load(Ordering::SeqCst) > reads);

    platform.failed.store(true, Ordering::Release);
    let changed = (5, 11);
    assert!(cache.read(&platform, &selected, Some(changed))[0].is_err());
    assert!(!cache.is_current(changed));
    platform.failed.store(false, Ordering::Release);
    assert!(cache.read(&platform, &selected, Some(changed))[0].is_ok());
    assert!(cache.is_current(changed));
    assert!(cache.read(&platform, &selected, None)[0].is_ok());
    assert!(!cache.is_current(changed));

    let changes = crate::game_read_models::signals::Signal::default();
    let continuity = Arc::new(crate::game_activation::ProcessContinuity::default());
    assert!(continuity.begin());
    changes.set_context_process_scope(5, Some(continuity.clone()));
    assert!(changes.context_process_live(5));
    assert!(!changes.context_process_live(6));
    continuity.retire();
    assert!(!changes.context_process_live(5));
}

#[test]
fn capture_wakes_select_only_verified_streaming_providers() {
    let scratch = Scratch::new();
    let (runtime, _, _) = runtime(&scratch);
    let catalogue = [
        ("x-plane-11", true),
        ("x-plane-12", true),
        ("assetto-corsa", true),
        ("dcs", true),
        ("x-plane-12", false),
    ].into_iter().map(|(id, compatible)| json!({
        "id": id, "provider": crate::game_discovery::integration_provider(id),
        "integrationKey": id, "integrationCompatible": compatible,
    })).collect();
    runtime.inner.settings.lock().unwrap().catalogue = catalogue;
    let ids = |capture_only| runtime.inner.vehicle_telemetry_games(capture_only)
        .iter().map(|game| game["id"].as_str().unwrap().to_owned()).collect::<Vec<_>>();
    assert_eq!(ids(true), ["x-plane-11", "x-plane-12", "assetto-corsa", "dcs"]);
    assert_eq!(ids(false), ["x-plane-11", "x-plane-12", "assetto-corsa", "dcs"]);
    runtime.inner.settings.lock().unwrap().metadata_error = Some("Fixture metadata error".into());
    assert!(ids(true).is_empty());
    assert!(ids(false).is_empty());
    runtime.shutdown();
}

#[test]
fn capture_and_context_workers_recover_after_a_locked_worker_panics() {
    let scratch = Scratch::new();
    let (runtime, _, _) = runtime(&scratch);
    let inner = &runtime.inner;
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _settings = inner.settings.lock().unwrap();
        let _vehicles = inner.vehicles.lock().unwrap();
        let _contexts = inner.contexts.lock().unwrap();
        panic!("Fixture worker failed while holding the observation locks");
    }));
    assert!(failed.is_err());
    assert!(inner.settings.is_poisoned() && inner.vehicles.is_poisoned() && inner.contexts.is_poisoned());
    assert_eq!(inner.vehicle_telemetry_games(true)[0]["id"], "x-plane-12");
    inner.poll_vehicle_telemetry(true);
    inner.refresh_game_contexts(false);
    assert_eq!(inner.vehicle_telemetry_games(true)[0]["id"], "x-plane-12");
    runtime.shutdown();
}

#[test]
fn game_context_observer_parks_without_a_configured_integration_and_wakes_on_registration() {
    let scratch = Scratch::new();
    let (runtime, platform, _) = runtime(&scratch);
    let checked = &runtime.inner.context_setup_check;
    let wait_for_setup = || {
        let deadline = Instant::now() + Duration::from_secs(3);
        while checked.lock().unwrap().is_none() {
            assert!(Instant::now() < deadline, "Game context worker did not refresh setup");
            thread::sleep(Duration::from_millis(10));
        }
    };

    *checked.lock().unwrap() = None;
    runtime.seed_test_catalogue(Vec::new());
    wait_for_setup();
    *checked.lock().unwrap() = None;
    thread::sleep(Duration::from_millis(650));
    assert!(checked.lock().unwrap().is_none(), "Idle context worker ran a timed refresh");

    let library = runtime.inner.changes.context_wake.observed();
    runtime.inner.changes.context_process_wake.notify();
    wait_for_setup();
    assert_eq!(runtime.inner.changes.context_wake.observed(), library, "Process events do not invalidate the library scope");
    *checked.lock().unwrap() = None;

    *platform.process.lock().unwrap() = None;
    runtime.seed_test_catalogue(platform.catalogue(&BTreeMap::new()));
    wait_for_setup();
    assert_eq!(context_sdk_watch::maintenance_delay(&runtime.inner, false, Instant::now()), Some(Duration::from_millis(500)));
    assert_eq!(context_sdk_watch::maintenance_delay(&runtime.inner, true, Instant::now()), None);

    let mut catalogue = platform.catalogue(&BTreeMap::new());
    catalogue.push(json!({"id":"assetto-corsa","provider":"assetto-corsa",
        "integrationKey":"assetto-corsa","integrationCompatible":true,"installed":true}));
    *checked.lock().unwrap() = None;
    runtime.seed_test_catalogue(catalogue);
    wait_for_setup();
    let delay = || context_sdk_watch::maintenance_delay(&runtime.inner, true, Instant::now());
    assert!(delay().is_some(), "An unwatched Assetto installation needs maintenance");
    runtime.inner.setup.set_setup_watch("assetto-corsa", true);
    assert_eq!(delay(), None, "A watched Assetto installation waits for its source or setup owner");
    runtime.inner.setup.set_setup_watch("assetto-corsa", false);

    let mut catalogue = platform.catalogue(&BTreeMap::new());
    catalogue.push(json!({"id":"dcs","provider":"dcs-bios","integrationKey":"dcs",
        "integrationCompatible":true,"executable":"DCS.exe"}));
    *checked.lock().unwrap() = None;
    runtime.seed_test_catalogue(catalogue);
    wait_for_setup();
    assert!(context_sdk_watch::maintenance_delay(&runtime.inner, true, Instant::now())
        .is_some_and(|delay| delay > Duration::from_millis(500)));
    runtime.inner.setup.set_dcs_discovery_watch(true);
    assert!(delay().is_some(), "Discovery alone cannot cover DCS setup files");
    runtime.inner.setup.set_setup_watch("dcs", true);
    assert_eq!(delay(), None, "Fully watched DCS waits for its source or setup owner");
    runtime.inner.setup.set_dcs_discovery_watch(false);
    assert!(delay().is_some(), "Watching setup files alone cannot cover discovery");
    runtime.shutdown();
}

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
    reads: std::sync::atomic::AtomicUsize,
    observation_hook: Mutex<Option<(std::thread::ThreadId, Box<dyn FnOnce() + Send>)>>,
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
            reads: std::sync::atomic::AtomicUsize::new(0),
            observation_hook: Mutex::new(None),
        }
    }
}
impl Platform for FakePlatform {
    fn integration_setup_supported(&self) -> bool {
        self.reads.fetch_add(1, Ordering::SeqCst);
        true
    }
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        vec![
            json!({"id":"x-plane-12","name":"X-Plane 12","provider":"x-plane","integrationCompatible":true,"integrationKey":"x-plane-12","installed":true,"executable":"/fixture/X-Plane.exe","launchSupported":true,"processMonitoringSupported":true}),
        ]
    }
    fn validate(&self, _: &str, path: &str) -> Result<(), String> {
        self.reads.fetch_add(1, Ordering::SeqCst);
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
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.failed.load(Ordering::Acquire) {
            Err("Process inspection failed".into())
        } else {
            Ok(self.process.lock().unwrap().clone())
        }
    }
    fn alive(&self, identity: &ProcessIdentity) -> Result<bool, String> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(self.process.lock().unwrap().as_ref() == Some(identity))
    }
    fn integration_process(&self, game: &Value) -> Result<Option<ProcessIdentity>, String> {
        let hook = {
            let mut hook = self.observation_hook.lock().unwrap();
            if hook.as_ref().is_some_and(|(thread, _)| *thread == std::thread::current().id()) {
                hook.take()
            } else { None }
        };
        if let Some((_, hook)) = hook { hook(); }
        self.find(game)
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
    let runtime = fixture_runtime(panel, Paths::in_directory(&scratch.path), platform.clone());
    let revision = runtime.inner.setup.provider_setup_revision("x-plane-12");
    runtime.request(&json!({"command":"game_integration_settings_save","gameId":"x-plane-12","expectedSetupRevision":revision,"connection":crate::xplane::defaults(12)})).unwrap();
    (runtime, platform, calls)
}
#[test]
fn desktop_process_identity_is_available_without_provider_setup_and_pid_reuse_changes_scope_evidence(
) {
    let scratch = Scratch::new();
    let platform = Arc::new(FakePlatform::new());
    let runtime = fixture_runtime(
        Arc::new(|request| {
            panic!("Process observation must not set up a provider or submit output: {request}")
        }),
        Paths::in_directory(&scratch.path),
        platform.clone(),
    );
    assert!(runtime
        .prepare_profile_game_context("x-plane-12")
        .unwrap()
        .is_none());
    let captured = runtime.desktop_process("x-plane-12").unwrap().unwrap();
    assert_eq!(captured.pid, 42);
    assert_eq!(captured.epoch, 10);
    platform.process.lock().unwrap().as_mut().unwrap().epoch = 11;
    let current = runtime.desktop_process("x-plane-12").unwrap().unwrap();
    assert_eq!(current.pid, captured.pid);
    assert_ne!(current, captured);
    platform.failed.store(true, Ordering::Release);
    assert!(runtime.desktop_process("x-plane-12").is_err());
    platform.failed.store(false, Ordering::Release);
    *platform.process.lock().unwrap() = None;
    assert!(runtime.desktop_process("x-plane-12").unwrap().is_none());
    assert!(runtime.desktop_process("unknown").is_err());
    runtime.shutdown();
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
fn owner_publication_does_no_discovery_and_admission_still_rechecks_the_process() {
    let scratch = Scratch::new();
    let (runtime, platform, calls) = runtime(&scratch);
    // Isolate the publication call from the independently running native
    // observers; preparation and admission below still use real Runtime paths.
    runtime.inner.closed.store(true, Ordering::Release);
    for observer in [&runtime.observer, &runtime.activation_observer] {
        if let Some(observer) = observer.lock().unwrap().take() {
            observer.join().unwrap();
        }
    }
    runtime.inner.closed.store(false, Ordering::Release);
    let prepared = runtime
        .prepare_profile_game_context("x-plane-12")
        .unwrap()
        .unwrap();
    *runtime.inner.context_setup_check.lock().unwrap() = None;
    let before = platform.reads.load(Ordering::SeqCst);
    runtime
        .set_profile_context_owner(Some(owner(Some("x-plane-12"))))
        .unwrap();
    assert_eq!(
        platform.reads.load(Ordering::SeqCst),
        before,
        "Publishing the applied owner must not perform synchronous native discovery"
    );
    assert!(runtime.inner.context_setup_check.lock().unwrap().is_none());
    let token = runtime
        .admit_prepared_game_context(&prepared.proof)
        .unwrap();
    assert_eq!(token.process, prepared.proof.process);
    assert!(platform.reads.load(Ordering::SeqCst) > before);
    platform.process.lock().unwrap().as_mut().unwrap().epoch += 1;
    assert_eq!(
        runtime
            .admit_prepared_game_context(&prepared.proof)
            .unwrap_err(),
        "The prepared game process exited, was replaced or became ambiguous."
    );
    assert!(runtime
        .inner
        .contexts
        .lock()
        .unwrap()
        .token("x-plane-12")
        .is_err());
    assert!(calls.lock().unwrap().is_empty());
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
include!("game_context_runtime_tests_tail.rs");
#[path = "game_context/decoration_tests.rs"]
mod decoration;
#[path = "games/context/validation_tests.rs"]
mod validation;
