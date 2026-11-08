use super::*;
use std::collections::BTreeMap;
struct Unsupported;
impl Platform for Unsupported {
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
        vec![json!({"id":"dcs","installed":false,"processMonitoringSupported":false})]
    }
    fn validate(&self, _: &str, _: &str) -> Result<(), String> {
        panic!("unsupported monitoring cannot validate")
    }
    fn launch(&self, _: &Value) -> Result<crate::game_discovery::ProcessIdentity, String> {
        panic!("unsupported monitoring cannot launch")
    }
    fn find(&self, _: &Value) -> Result<Option<crate::game_discovery::ProcessIdentity>, String> {
        panic!("unsupported monitoring cannot scan")
    }
    fn alive(&self, _: &crate::game_discovery::ProcessIdentity) -> Result<bool, String> {
        panic!("unsupported monitoring cannot check liveness")
    }
}
#[test]
fn unsupported_monitoring_stays_explicit_and_cannot_invent_a_context() {
    let directory = crate::test_directory();
    let runtime = GamesRuntime::test_runtime(
        Arc::new(|_| panic!("no provider calls")),
        {
            let fixture_metadata_path = directory.path().join("games.json");
            crate::games::Paths {
                installations: fixture_metadata_path.with_extension("installations-v1.json"),
                provider_setup: fixture_metadata_path.with_extension("integration-setup.json"),
                recovery: fixture_metadata_path.with_extension("recovery.json"),
            }
        },
        Arc::new(Unsupported),
    );
    assert!(runtime.activation_observer.lock().unwrap().is_none());
    assert!(runtime.profile_activation_observations().is_err());
    assert!(runtime.profile_activation_identity("dcs").is_err());
    assert!(runtime
        .validate_profile_activation(&VerifiedGame {
            identity: GameIdentity {
                game_id: "unknown".into(),
                executable: "C:\\bad.exe".into()
            },
            process_session: "forged".into()
        })
        .is_err());
    runtime.shutdown();
}

#[test]
fn corrupt_installation_store_never_verifies_or_launches_the_discovered_fallback() {
    use crate::game_discovery::ProcessIdentity;
    use std::sync::atomic::AtomicUsize;
    #[derive(Default)]
    struct Fallback(AtomicUsize);
    impl Platform for Fallback {
        fn catalogue(&self, manual: &BTreeMap<String, String>) -> Vec<Value> {
            assert!(manual.is_empty());
            vec![
                json!({"id":"dcs","provider":"dcs-bios","installed":true,"launchSupported":true,"processMonitoringSupported":true,"executable":"C:\\Discovered\\DCS.exe"}),
            ]
        }
        fn validate(&self, _: &str, _: &str) -> Result<(), String> {
            self.0.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }
        fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> {
            panic!("Retired launcher cannot run")
        }
        fn spawn_process(&self, _: &Value) -> Result<(), String> {
            self.0.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }
        fn find(&self, _: &Value) -> Result<Option<ProcessIdentity>, String> {
            self.0.fetch_add(1, Ordering::AcqRel);
            Ok(Some(ProcessIdentity {
                pid: 42,
                epoch: 1,
                path: "C:\\Discovered\\DCS.exe".into(),
            }))
        }
        fn alive(&self, _: &ProcessIdentity) -> Result<bool, String> {
            self.0.fetch_add(1, Ordering::AcqRel);
            Ok(true)
        }
    }
    let directory = crate::test_directory();
    let paths = Paths::in_directory(directory.path());
    let file = paths.installations.clone();
    let corrupt=br#"{"schemaVersion":1,"manualInstallations":{"dcs":"Z:\\Pinned\\DCS.exe"},"interrupted":true}"#;
    fs::write(&file, corrupt).unwrap();
    crate::persistence::test_support::record(&file, "games", "library", corrupt);
    let platform = Arc::new(Fallback::default());
    let runtime = GamesRuntime::test_runtime(
        Arc::new(|_| panic!("No provider/hardware calls")),
        paths,
        platform.clone(),
    );
    assert!(runtime.activation_observer.lock().unwrap().is_none());
    for result in [
        runtime.profile_activation_identity("dcs").map(|_| ()),
        runtime.profile_activation_observations().map(|_| ()),
        runtime.validate_profile_activation(&VerifiedGame {
            identity: GameIdentity {
                game_id: "dcs".into(),
                executable: "C:\\Discovered\\DCS.exe".into(),
            },
            process_session: uuid::Uuid::new_v4().to_string(),
        }),
        runtime.prepare_profile_game_context("dcs").map(|_| ()),
    ] {
        assert!(result
            .unwrap_err()
            .contains("Repair game installation metadata"));
    }
    let mut candidate = crate::profile_session::Candidate { virtual_slots: Default::default(),
        launch_preparation: false,
        document: osg_profiles::default_document(),
        assignments: vec![],
        read_only: false,
        game_id: None,
        admission_epoch: Some(1),
        stage: None,
    };
    candidate.launch_preparation = true;
    candidate.stage = Some(crate::profile_session::Stage::new(&candidate, ()).unwrap());
    assert!(runtime
        .prepare_profile_launch("dcs", &candidate)
        .err()
        .unwrap()
        .contains("Repair game installation metadata"));
    for command in [
        "game_integration_settings_save",
        "game_integration_setup",
        "game_integration_check",
        "game_integration_recover",
    ] {
        assert!(runtime
            .request(&json!({"command":command,"gameId":"dcs"}))
            .unwrap_err()
            .contains("Repair game installation metadata"));
    }
    assert!(runtime
        .request(&json!({"command":"game_integration_cancel","gameId":"dcs"}))
        .unwrap_err().contains("setup job changed or finished"));
    runtime
        .request(&json!({"command":"game_get_state"}))
        .unwrap();
    assert!(
        runtime
            .resource_integration_catalogue("x-plane-11")
            .is_err(),
        "Corrupt game metadata must not fall back to an unregistered integration"
    );
    assert!(
        runtime.guard_profile_takeover().is_ok(),
        "Ordinary hardware ownership remains usable"
    );
    runtime.shutdown();
    assert_eq!(platform.0.load(Ordering::Acquire), 0);
    assert_eq!(fs::read(file).unwrap(), corrupt);
    assert_eq!(
        fs::read_dir(directory.path())
            .unwrap()
            .filter(|entry| entry.as_ref().unwrap().file_name() != "cockpit.sqlite3")
            .count(),
        1,
        "No setup, probe or cleanup evidence was written"
    );
}

#[test]
fn native_observer_starts_before_first_discovery_and_monitors_an_integration_free_title() {
    #[derive(Default)]
    struct EmptyMonitoring {
        scans: std::sync::atomic::AtomicUsize,
        #[cfg(windows)]
        watches: std::sync::atomic::AtomicUsize,
    }
    impl Platform for EmptyMonitoring {
        fn process_monitoring_supported(&self) -> bool {
            true
        }
        fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
            vec![]
        }
        #[cfg(windows)]
        fn watch_activation_processes(&self, games: &[Value], _: [Arc<osg_streams::NativeWake>; 2])
            -> Option<crate::xplane::process_events::InventoryEvents> {
            games.iter().any(crate::game_activation::process_candidate).then(|| {
                self.watches.fetch_add(1, Ordering::AcqRel);
                crate::xplane::process_events::InventoryEvents::test_failed_after_live()
            })
        }
        fn validate(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn launch(&self, _: &Value) -> Result<crate::game_discovery::ProcessIdentity, String> {
            Err("Unexpected launch".into())
        }
        fn find(
            &self,
            game: &Value,
        ) -> Result<Option<crate::game_discovery::ProcessIdentity>, String> {
            Ok(Some(crate::game_discovery::ProcessIdentity {
                pid: 42,
                epoch: 1,
                path: game["executable"].as_str().unwrap().into(),
            }))
        }
        fn find_many(&self, games: &[Value]) -> Vec<Result<Option<crate::game_discovery::ProcessIdentity>, String>> {
            self.scans.fetch_add(1, Ordering::AcqRel);
            games.iter().map(|game| self.find(game)).collect()
        }
        fn alive(&self, _: &crate::game_discovery::ProcessIdentity) -> Result<bool, String> {
            Ok(true)
        }
    }
    let directory = crate::test_directory();
    let platform = Arc::new(EmptyMonitoring::default());
    let preferences = crate::persistence::ui::Preferences::new(&directory.path().join("fixture.json"));
    let runtime = GamesRuntime::new(
        Arc::new(|_| panic!("No integration calls")),
        Paths::in_directory(directory.path()),
        platform.clone(),
        preferences.clone(),
    );
    assert!(runtime.inner.settings.lock().unwrap().catalogue.is_empty());
    assert!(runtime.activation_observer.lock().unwrap().is_some());
    let initial_deadline = Instant::now() + Duration::from_secs(3);
    while runtime.profile_activation_observations().is_err() {
        assert!(Instant::now() < initial_deadline, "Empty catalogue was never observed");
        thread::sleep(Duration::from_millis(10));
    }
    thread::sleep(Duration::from_millis(2100));
    assert_eq!(platform.scans.load(Ordering::Acquire), 1, "Empty catalogue must leave the worker parked");
    runtime.inner.settings.lock().unwrap().catalogue.extend([
        json!({"id":"missing-game","installed":false,"processMonitoringSupported":true}),
        json!({"id":"unsupported-game","installed":true,"processMonitoringSupported":false}),
    ]);
    runtime.inner.changes.mark_library_commit();
    let quiet_deadline = Instant::now() + Duration::from_secs(3);
    while platform.scans.load(Ordering::Acquire) < 2 {
        assert!(Instant::now() < quiet_deadline, "Library commit did not wake the worker");
        thread::sleep(Duration::from_millis(10));
    }
    thread::sleep(Duration::from_millis(2100));
    assert_eq!(platform.scans.load(Ordering::Acquire), 2, "Catalogue without monitorable games must leave the worker parked");
    assert!(runtime.profile_activation_observations().unwrap().is_empty());
    runtime.inner.settings.lock().unwrap().catalogue.push(json!({"id":"first-discovered-game","name":"First discovered game","installed":true,"processMonitoringSupported":true,"executable":"C:/Games/First.exe","integrationKey":null,"provider":null}));
    runtime.inner.changes.mark_library_commit();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        if runtime
            .profile_activation_observations()
            .ok()
            .is_some_and(|games| {
                games
                    .iter()
                    .any(|game| game.identity.game_id == "first-discovered-game")
            })
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "First discovered game was never observed"
        );
        thread::sleep(Duration::from_millis(20));
    }
    #[cfg(windows)]
    {
        assert_eq!(platform.watches.load(Ordering::Acquire), 0, "Default detection must not subscribe to Windows events");
        preferences.write("game-process-events", 0, json!(true)).unwrap();
        runtime.process_event_preference_changed();
        let deadline = Instant::now() + Duration::from_secs(3);
        while platform.watches.load(Ordering::Acquire) < 2 {
            assert!(Instant::now() < deadline, "Opt-in did not start and recover the watcher");
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(platform.watches.load(Ordering::Acquire), 2,
            "One initial watcher and one recovery watcher per library revision");
        runtime.inner.changes.mark_library_commit();
        let deadline = Instant::now() + Duration::from_secs(3);
        while platform.watches.load(Ordering::Acquire) < 4 {
            assert!(Instant::now() < deadline, "Library commit did not reset the watcher recovery budget");
            thread::sleep(Duration::from_millis(10));
        }
        preferences.write("game-process-events", 1, json!(false)).unwrap();
        runtime.process_event_preference_changed();
        runtime.inner.changes.mark_library_commit();
        let deadline = Instant::now() + Duration::from_secs(3);
        let scans = platform.scans.load(Ordering::Acquire);
        while platform.scans.load(Ordering::Acquire) < scans + 2 {
            assert!(Instant::now() < deadline, "Fallback scans stopped after opt-out");
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(platform.watches.load(Ordering::Acquire), 4);
    }
    runtime.shutdown();
}
