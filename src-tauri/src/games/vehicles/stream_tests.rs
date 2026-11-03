use super::*;

fn game() -> Value {
    json!({"id":"assetto-corsa","integrationCompatible":true,"integrationKey":"assetto-corsa","provider":"assetto"})
}

#[test]
fn summary_counts_unique_ids_without_exporting_inventory_and_keeps_current_status() {
    let root = crate::test_directory();
    let mut tracker = Tracker::new(root.path().join("vehicles.json"));
    tracker.set_test_vehicle("assetto-corsa", Some("car"));
    tracker.seen.insert("assetto-corsa".into(), BTreeMap::from([("car".into(), json!({"id":"car","name":"saved"}))]));
    tracker.entries.get_mut("assetto-corsa").unwrap().vehicles = vec![json!({"id":"car","name":"installed"}), json!({"id":"second","name":"second"})];
    // Use the same captured installation as the fixture injector.
    let mut summary = json!({"id":"assetto-corsa","integrationCompatible":true,"integrationKey":"assetto-corsa","provider":crate::game_discovery::integration_provider("assetto-corsa")});
    let mut detail = summary.clone();
    tracker.decorate_summary(&mut summary); tracker.decorate(&mut detail);
    assert_eq!(summary["vehicleSupport"]["vehicleCount"], 2);
    assert!(summary["vehicleSupport"].get("vehicles").is_none());
    assert_eq!(summary["currentVehicle"], detail["currentVehicle"]);
    assert_eq!(summary["vehicleStatus"], detail["vehicleStatus"]);
    assert_eq!(detail["vehicleSupport"]["vehicles"][0]["name"], "installed");
}

#[test]
fn identical_observations_and_test_vehicles_do_not_emit_runtime_changes() {
    let root = crate::test_directory();
    let mut tracker = Tracker::new(root.path().join("vehicles.json"));
    let signal = Arc::new(crate::game_read_models::signals::Signal::default());
    tracker.watch_changes(signal.clone());
    tracker.set_test_vehicle("assetto-corsa", Some("car"));
    let selected = signal.wake.observed();
    tracker.set_test_vehicle("assetto-corsa", Some("car"));
    assert_eq!(signal.wake.observed(), selected);
    tracker.set_test_vehicle("assetto-corsa", None);
    assert_ne!(signal.wake.observed(), selected);
    let cleared = signal.wake.observed();
    tracker.set_test_vehicle("assetto-corsa", None);
    assert_eq!(signal.wake.observed(), cleared);
    tracker.observe(&game(), &json!({}), None, "setup", false, 1);
    let observed = signal.wake.observed();
    tracker.observe(&game(), &json!({}), None, "setup", false, 2);
    assert_eq!(signal.wake.observed(), observed);
}
