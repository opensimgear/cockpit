#[test]
fn process_monitor_cancel_bypasses_takeover_and_never_claims_outputs() {
    let (manager, provider) = setup();
    manager.games.seed_test_catalogue(vec![json!({"id":"x-plane-12","name":"X-Plane 12",
        "provider":"x-plane","integrationKey":"x-plane-12","integrationCompatible":true,
        "installed":true,"launchSupported":false,"processMonitoringSupported":false})]);
    let profile = manager.profile_admission.write().unwrap();
    let game = manager.game_admission.write().unwrap();
    let coordinator = manager.profile_session.lock().unwrap();
    let epoch = manager.profile_admission_epoch.load(Ordering::Acquire);
    let response = request(&manager, "game_process_monitor_cancel", None, json!({"gameId":"x-plane-12"}));
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(manager.profile_admission_epoch.load(Ordering::Acquire), epoch);
    assert!(provider.adapters.lock().unwrap().is_empty());
    assert_eq!(profile_guard_operation("game_process_monitor_enable"), None);
    assert_eq!(profile_guard_operation("game_process_monitor_cancel"), None);
    assert!(!learning_interrupt(&json!({"command":"game_process_monitor_enable"})));
    assert!(!learning_interrupt(&json!({"command":"game_process_monitor_cancel"})));
    drop(coordinator); drop(game); drop(profile); manager.shutdown();
}
