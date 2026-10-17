use super::*;
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
    let directory = tempfile::tempdir().unwrap();
    let runtime = GamesRuntime::new(
        Arc::new(|_| panic!("no provider calls")),
        directory.path().join("games.json"),
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
