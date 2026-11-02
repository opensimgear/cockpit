use super::*;
use crate::game_discovery::ProcessIdentity;
use std::collections::BTreeMap;
struct Passive;
impl Platform for Passive {
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
        vec![
            json!({"id":"x-plane-12","name":"X-Plane 12","provider":"x-plane",
            "installed":true,"executable":"C:/X-Plane.exe","installPath":"C:/X-Plane",
            "launchSupported":false,"processMonitoringSupported":false}),
        ]
    }
    fn validate(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> {
        panic!("Permission requests must never launch a game")
    }
    fn find(&self, _: &Value) -> Result<Option<ProcessIdentity>, String> {
        Ok(None)
    }
    fn alive(&self, _: &ProcessIdentity) -> Result<bool, String> {
        Ok(false)
    }
}
#[test]
fn requests_cannot_supply_a_pid_path_or_enable_an_unobserved_process() {
    let directory = crate::test_directory();
    let runtime = GamesRuntime::test_runtime(
        Arc::new(|_| json!({})),
        Paths::in_directory(directory.path()),
        Arc::new(Passive),
    );
    for command in ["game_process_monitor_enable", "game_process_monitor_cancel"] {
        let forged = json!({"command":command,"gameId":"x-plane-12","pid":123});
        assert!(runtime
            .request(&forged)
            .unwrap_err()
            .contains("Unknown process monitoring field"));
    }
    let before = crate::xplane::permissions::status("x-plane-12");
    let error = runtime
        .request(&json!({"command":"game_process_monitor_enable","gameId":"x-plane-12"}))
        .unwrap_err();
    assert!(error.contains("Start the selected game"), "{error}");
    assert_eq!(crate::xplane::permissions::status("x-plane-12"), before);
    assert!(runtime
        .request(&json!({"command":"game_process_monitor_enable","gameId":"missing"}))
        .is_err());
}
#[test]
fn permission_actions_are_write_scoped_mcp_tools_with_bounded_game_identity() {
    for name in ["game_process_monitor_enable", "game_process_monitor_cancel"] {
        assert!(crate::mcp_tools::is_mutation(name));
        let catalogue = crate::mcp_tools::catalogue();
        let tool = catalogue.iter().find(|tool| tool["name"] == name).unwrap();
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
        assert_eq!(tool["inputSchema"]["properties"]["gameId"]["maxLength"], 96);
        assert!(tool["inputSchema"]["properties"].get("pid").is_none());
        assert!(tool["inputSchema"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("operationId")));
    }
}
