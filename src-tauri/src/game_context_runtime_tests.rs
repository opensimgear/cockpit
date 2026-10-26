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
    reads: std::sync::atomic::AtomicUsize,
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
