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
    let runtime = GamesRuntime::new(
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
                json!({"id":"dcs","installed":true,"launchSupported":true,"processMonitoringSupported":true,"executable":"C:\\Discovered\\DCS.exe"}),
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
    let platform = Arc::new(Fallback::default());
    let runtime = GamesRuntime::new(
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
    let mut candidate = crate::profile_session::Candidate {
        document: osg_profiles::default_document(),
        assignments: vec![],
        read_only: false,
        game_id: None,
        admission_epoch: Some(1),
        stage: None,
    };
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
    runtime
        .request(&json!({"command":"game_integration_cancel","gameId":"dcs"}))
        .unwrap();
    runtime
        .request(&json!({"command":"game_get_state"}))
        .unwrap();
    assert!(
        runtime.resource_integration_catalogue("x-plane-11").is_ok(),
        "Static metadata does not require an executable"
    );
    assert!(
        runtime.guard_profile_takeover().is_ok(),
        "Ordinary hardware ownership remains usable"
    );
    runtime.shutdown();
    assert_eq!(platform.0.load(Ordering::Acquire), 0);
    assert_eq!(fs::read(file).unwrap(), corrupt);
    assert_eq!(
        fs::read_dir(directory.path()).unwrap().count(),
        1,
        "No setup, probe or cleanup evidence was written"
    );
}
