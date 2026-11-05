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
    let automatic = Arc::new(osg_streams::NativeWake::default());
    let routing = Arc::new(osg_streams::NativeWake::default());
    signal.watch_automatic(&automatic);
    signal.watch_vehicle_routing(&routing);
    tracker.watch_changes(signal.clone());
    let observed = || (automatic.observed(), routing.observed());
    let initial = observed();
    tracker.set_test_vehicle("assetto-corsa", Some("car"));
    let selected_context = observed();
    assert_ne!(selected_context.0, initial.0);
    assert_ne!(selected_context.1, initial.1);
    let selected = signal.wake.observed();
    tracker.set_test_vehicle("assetto-corsa", Some("car"));
    assert_eq!(signal.wake.observed(), selected);
    tracker.entries.get_mut("assetto-corsa").unwrap().current.as_mut().unwrap()["name"] = json!("Renamed car");
    tracker.publish_visible("assetto-corsa");
    assert_eq!(observed(), selected_context);
    tracker.set_test_vehicle("assetto-corsa", None);
    assert_ne!(signal.wake.observed(), selected);
    let cleared_context = observed();
    assert_ne!(cleared_context.0, selected_context.0);
    assert_ne!(cleared_context.1, selected_context.1);
    let cleared = signal.wake.observed();
    tracker.set_test_vehicle("assetto-corsa", None);
    assert_eq!(signal.wake.observed(), cleared);
    assert_eq!(observed(), cleared_context);
    tracker.observe(&game(), &json!({}), None, "setup", false, 1);
    let observed = signal.wake.observed();
    tracker.observe(&game(), &json!({}), None, "setup", false, 2);
    assert_eq!(signal.wake.observed(), observed);
}
