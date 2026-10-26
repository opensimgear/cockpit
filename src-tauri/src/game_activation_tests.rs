use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
struct Fake {
    current: Mutex<Result<Option<ProcessIdentity>, String>>,
    scans: AtomicUsize,
    catalogue: Mutex<Vec<Value>>,
}
impl Platform for Fake {
    // Deliberately leaves integration_setup_supported false.
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
        self.catalogue.lock().unwrap().clone()
    }
    fn validate(&self, _: &str, path: &str) -> Result<(), String> {
        if path == "C:\\Games\\DCS\\bin\\DCS.exe" {
            Ok(())
        } else {
            Err("Wrong executable".into())
        }
    }
    fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> {
        panic!("watching must never launch")
    }
    fn find(&self, _: &Value) -> Result<Option<ProcessIdentity>, String> {
        self.scans.fetch_add(1, Ordering::Relaxed);
        self.current.lock().unwrap().clone()
    }
    fn alive(&self, process: &ProcessIdentity) -> Result<bool, String> {
        Ok(self
            .current
            .lock()
            .unwrap()
            .as_ref()
            .ok()
            .and_then(|p| p.as_ref())
            == Some(process))
    }
}
fn game() -> Value {
    json!({"id":"dcs","installed":true,"processMonitoringSupported":true,"executable":"C:\\Games\\DCS\\bin\\DCS.exe"})
}
fn running(epoch: u64) -> ProcessIdentity {
    ProcessIdentity {
        pid: 42,
        path: "C:\\Games\\DCS\\bin\\DCS.exe".into(),
        epoch,
    }
}
fn fake() -> Fake {
    Fake {
        current: Mutex::new(Ok(None)),
        scans: AtomicUsize::new(0),
        catalogue: Mutex::new(vec![game()]),
    }
}
#[test]
fn uninstalled_supported_games_are_not_process_detection_failures() {
    let platform = fake();
    let observer = Observer::default();
    let missing = json!({"id":"x-plane-12","installed":false,"processMonitoringSupported":true});
    observer.observe(&platform, &[game(), missing.clone()]);
    assert_eq!(observer.view("dcs")["status"], "not-running");
    assert_eq!(observer.view("x-plane-12")["status"], "not-installed");
    assert!(observer.snapshot().unwrap().is_empty());
    assert_eq!(platform.scans.load(Ordering::Relaxed), 1);
    assert!(identity(&platform, &missing).is_err());
    *platform.current.lock().unwrap() = Ok(Some(running(1)));
    observer.observe(&platform, &[game()]);
    observer.observe(&platform, &[game()]);
    assert_eq!(observer.snapshot().unwrap().len(), 1);
    let mut removed = game();
    removed["installed"] = json!(false);
    observer.observe(&platform, &[removed]);
    assert_eq!(observer.view("dcs")["status"], "not-installed");
    assert!(observer.snapshot().unwrap().is_empty());
}

#[test]
fn native_games_status_separates_missing_installations_from_real_scan_errors() {
    let platform = std::sync::Arc::new(fake());
    let mut catalogue = vec![game()];
    for id in ["assetto-corsa-rally", "x-plane-11", "x-plane-12", "assetto-corsa", "assetto-corsa-competizione", "assetto-corsa-evo"] {
        catalogue.push(json!({"id":id,"installed":false,"processMonitoringSupported":true}));
    }
    *platform.catalogue.lock().unwrap() = catalogue;
    let directory = crate::test_directory();
    let runtime = crate::games::GamesRuntime::test_runtime(
        std::sync::Arc::new(|_| panic!("Detection cannot call device/provider output")),
        crate::games::Paths::in_directory(directory.path()),
        platform.clone(),
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    while runtime.profile_activation_observations().is_err() {
        assert!(Instant::now() < deadline, "Native observer did not publish");
        std::thread::sleep(Duration::from_millis(5));
    }
    let status = runtime.status();
    assert_eq!(status["detectedGames"], json!([]));
    assert_eq!(status["detectionErrors"], json!([]));
    assert!(status["catalogue"]
        .as_array()
        .unwrap()
        .iter()
        .skip(1)
        .all(|game| game["profileContext"]["status"] == "not-installed"));
    *platform.current.lock().unwrap() = Err("Cannot inspect game process: access denied".into());
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let status = runtime.status();
        if status["detectionErrors"] == json!(["Cannot inspect game process: access denied"]) {
            assert_eq!(status["detectedGames"], json!([]));
            break;
        }
        assert!(Instant::now() < deadline, "Actual scan failure was hidden");
        std::thread::sleep(Duration::from_millis(5));
    }
    runtime.shutdown();
}
#[test]
fn background_capture_needs_no_integration_setup_and_status_reads_never_scan() {
    let platform = fake();
    let observer = Observer::default();
    assert!(!platform.integration_setup_supported());
    assert!(observer.snapshot().is_err());
    *platform.current.lock().unwrap() = Ok(Some(running(1)));
    observer.observe(&platform, &[game()]);
    assert!(observer.snapshot().unwrap().is_empty());
    assert_eq!(observer.view("dcs")["status"], "settling");
    observer.observe(&platform, &[game()]);
    let context = observer.snapshot().unwrap().pop().unwrap();
    for _ in 0..3 {
        assert_eq!(observer.snapshot().unwrap(), [context.clone()]);
        assert_eq!(observer.view("dcs")["status"], "verified");
    }
    assert_eq!(platform.scans.load(Ordering::Relaxed), 2);
    observer.validate(&platform, &game(), &context).unwrap();
    observer.observe(&platform, &[game()]);
    assert_eq!(observer.snapshot().unwrap(), [context]);
}
#[test]
fn launch_confirmation_requires_exact_debounced_process_without_scanning() {
    let platform = fake();
    let observer = Observer::default();
    let identity = identity(&platform, &game()).unwrap();
    assert!(observer.confirmed_process(&identity, &running(1)).is_none());
    *platform.current.lock().unwrap() = Ok(Some(running(1)));
    observer.observe(&platform, &[game()]);
    assert!(observer.confirmed_process(&identity, &running(1)).is_none());
    observer.observe(&platform, &[game()]);
    let confirmed = observer.confirmed_process(&identity, &running(1)).unwrap();
    assert!(observer.confirmed_process(&identity, &running(2)).is_none());
    let mut wrong = running(1);
    wrong.path = "C:\\Other\\DCS.exe".into();
    assert!(observer.confirmed_process(&identity, &wrong).is_none());
    assert_eq!(platform.scans.load(Ordering::Relaxed), 2);
    *platform.current.lock().unwrap() = Ok(Some(running(2)));
    observer.observe(&platform, &[game()]);
    assert!(observer.confirmed_process(&identity, &running(1)).is_none());
    observer.observe(&platform, &[game()]);
    assert_ne!(
        observer.confirmed_process(&identity, &running(2)).unwrap(),
        confirmed
    );
    observer.0.lock().unwrap().at = Some(Instant::now() - MAX_AGE - Duration::from_millis(1));
    assert!(observer.confirmed_process(&identity, &running(2)).is_none());
}
#[test]
fn replacement_exit_ambiguity_and_executable_change_revoke_without_replay() {
    let platform = fake();
    let observer = Observer::default();
    *platform.current.lock().unwrap() = Ok(Some(running(1)));
    observer.observe(&platform, &[game()]);
    observer.observe(&platform, &[game()]);
    let original = observer.snapshot().unwrap().pop().unwrap();
    *platform.current.lock().unwrap() = Ok(Some(running(2)));
    assert!(observer.validate(&platform, &game(), &original).is_err());
    observer.observe(&platform, &[game()]);
    assert!(observer.snapshot().unwrap().is_empty());
    observer.observe(&platform, &[game()]);
    let replacement = observer.snapshot().unwrap().pop().unwrap();
    assert_ne!(replacement.process_session, original.process_session);
    *platform.current.lock().unwrap() = Err("Two matching instances".into());
    observer.observe(&platform, &[game()]);
    assert!(observer.snapshot().unwrap().is_empty());
    assert_eq!(observer.view("dcs")["status"], "unavailable");
    *platform.current.lock().unwrap() = Ok(None);
    observer.observe(&platform, &[game()]);
    assert_eq!(observer.view("dcs")["status"], "not-running");
    let mut wrong = game();
    wrong["executable"] = json!("C:\\other.exe");
    assert!(identity(&platform, &wrong).is_err());
}
#[test]
fn stale_unsupported_malformed_and_duplicate_catalogues_never_grant_authority() {
    let platform = fake();
    let observer = Observer::default();
    *platform.current.lock().unwrap() = Ok(Some(running(1)));
    observer.observe(&platform, &[game()]);
    observer.observe(&platform, &[game()]);
    let proof = observer.snapshot().unwrap().pop().unwrap();
    observer.0.lock().unwrap().at = Some(Instant::now() - MAX_AGE - Duration::from_millis(1));
    assert!(observer.snapshot().is_err());
    assert!(observer.validate(&platform, &game(), &proof).is_err());
    assert_eq!(observer.view("dcs")["status"], "waiting");
    let mut unsupported = game();
    unsupported["processMonitoringSupported"] = json!(false);
    observer.observe(&platform, &[unsupported]);
    assert!(observer.snapshot().unwrap().is_empty());
    *platform.current.lock().unwrap() = Ok(Some(ProcessIdentity {
        epoch: 0,
        ..running(1)
    }));
    observer.observe(&platform, &[game()]);
    assert!(observer.snapshot().unwrap().is_empty());
    *platform.current.lock().unwrap() = Ok(Some(running(1)));
    observer.observe(&platform, &[game(), game()]);
    assert!(observer.snapshot().unwrap().is_empty());
    observer.observe(&platform, &vec![game(); 5]);
    assert!(observer.snapshot().unwrap().is_empty());
    observer.observe(&platform, &[]);
    assert!(observer.snapshot().unwrap().is_empty());
}

#[test]
fn actual_games_runtime_detects_external_start_and_exit_without_ui_or_provider_setup() {
    let platform = std::sync::Arc::new(fake());
    let directory = crate::test_directory();
    let runtime = crate::games::GamesRuntime::test_runtime(
        std::sync::Arc::new(|_| {
            panic!("Process observation must not call provider or device output")
        }),
        {
            let fixture_metadata_path = directory.path().join("games.json");
            crate::games::Paths {
                installations: fixture_metadata_path.with_extension("installations-v1.json"),
                provider_setup: fixture_metadata_path.with_extension("integration-setup.json"),
                recovery: fixture_metadata_path.with_extension("recovery.json"),
            }
        },
        platform.clone(),
    );
    assert_eq!(
        runtime.profile_activation_identity("dcs").unwrap(),
        identity(platform.as_ref(), &game()).unwrap()
    );
    assert!(runtime.profile_activation_identity("unknown").is_err());
    *platform.current.lock().unwrap() = Ok(Some(running(1)));
    let deadline = Instant::now() + Duration::from_secs(4);
    let context = loop {
        if let Ok(games) = runtime.profile_activation_observations() {
            if let Some(context) = games.into_iter().next() {
                break context;
            }
        }
        assert!(
            Instant::now() < deadline,
            "Background watcher did not confirm external game"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    runtime.validate_profile_activation(&context).unwrap();
    *platform.current.lock().unwrap() = Ok(None);
    assert!(runtime.validate_profile_activation(&context).is_err());
    let deadline = Instant::now() + Duration::from_secs(2);
    while !runtime
        .profile_activation_observations()
        .unwrap()
        .is_empty()
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    runtime.shutdown();
}

#[test]
fn large_custom_catalogue_uses_one_batch_and_relocation_removal_revoke_old_process_authority() {
    struct Batched {
        processes: Mutex<BTreeMap<String, ProcessIdentity>>,
        batches: AtomicUsize,
    }
    impl Platform for Batched {
        fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> { vec![] }
        fn validate(&self, id: &str, path: &str) -> Result<(), String> {
            GameIdentity { game_id: id.into(), executable: path.into() }.validate()
        }
        fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> { panic!("observation cannot launch") }
        fn find(&self, game: &Value) -> Result<Option<ProcessIdentity>, String> {
            Ok(self.processes.lock().unwrap().get(game["id"].as_str().unwrap())
                .filter(|process| game["executable"].as_str().is_some_and(|path|path.eq_ignore_ascii_case(&process.path))).cloned())
        }
        fn find_many(&self, games: &[Value]) -> Vec<Result<Option<ProcessIdentity>, String>> {
            self.batches.fetch_add(1, Ordering::Relaxed);
            games.iter().map(|game|self.find(game)).collect()
        }
        fn alive(&self, process: &ProcessIdentity) -> Result<bool, String> {
            Ok(self.processes.lock().unwrap().values().any(|candidate|candidate == process))
        }
    }
    let platform = Batched { processes: Mutex::new(BTreeMap::new()), batches: AtomicUsize::new(0) };
    let mut games = (1..=osg_profiles::activation::MAX_GAMES).map(|index| {
        let id = format!("custom-{}", uuid::Uuid::from_u128(index as u128));
        let process = ProcessIdentity { pid: index as u32, epoch: 10, path: format!("C:\\Games\\{index}\\Game.exe") };
        platform.processes.lock().unwrap().insert(id.clone(), process.clone());
        json!({"id":id,"installed":true,"custom":true,"provider":null,"processMonitoringSupported":true,"executable":process.path})
    }).collect::<Vec<_>>();
    let observer = Observer::default();
    observer.observe(&platform, &games);
    assert!(observer.snapshot().unwrap().is_empty());
    observer.observe(&platform, &games);
    assert_eq!(platform.batches.load(Ordering::Relaxed), 2);
    assert_eq!(observer.snapshot().unwrap().len(), osg_profiles::activation::MAX_GAMES);
    let first = games[0]["id"].as_str().unwrap().to_owned();
    let original = observer.snapshot().unwrap().into_iter().find(|game|game.identity.game_id == first).unwrap();
    games[0]["executable"] = json!("C:\\Moved\\Game.exe");
    assert!(observer.validate(&platform, &games[0], &original).is_err());
    observer.observe(&platform, &games);
    assert_eq!(observer.snapshot().unwrap().len(), osg_profiles::activation::MAX_GAMES - 1);
    let replacement = ProcessIdentity { pid: 700, epoch: 20, path: "C:\\Moved\\Game.exe".into() };
    platform.processes.lock().unwrap().insert(first.clone(), replacement);
    observer.observe(&platform, &games);
    assert_eq!(observer.view(&first)["status"], "settling");
    observer.observe(&platform, &games);
    let current = observer.snapshot().unwrap().into_iter().find(|game|game.identity.game_id == first).unwrap();
    assert_ne!(current.process_session, original.process_session);
    games.remove(0);
    observer.observe(&platform, &games);
    assert_eq!(observer.view(&first)["status"], "waiting");
    assert!(observer.snapshot().unwrap().iter().all(|game|game.identity.game_id != first));
    assert_eq!(platform.batches.load(Ordering::Relaxed), 6);
}
