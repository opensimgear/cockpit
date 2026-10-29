use super::*;

fn active_variant() -> Fixture {
    let mut fixture = Fixture::new();
    fixture.bind();
    let candidate = fixture.stage();
    let prepared = fixture.host.install_launch_context(&fixture.run, &candidate).unwrap();
    fixture.host.set_authority(Some(&fixture.run.owner)).unwrap();
    fixture.host.enter(ApplyPhase::Start, &prepared, &fixture.run).unwrap();
    let state = fixture.host.run(&fixture.run.owner).unwrap();
    state.source_document.vehicle_tabs = [("alpha", "vehicle-a"), ("beta", "vehicle-b")]
        .into_iter().map(|(tab, vehicle)| osg_profiles::VehicleTab {
            id: tab.into(), game_id: "x-plane-12".into(), vehicle_id: vehicle.into(),
            vehicle_name: vehicle.into(), node_ids: vec![],
        }).collect();
    state.vehicle_selection = Some("alpha".into());
    fixture
}

#[test]
fn unknown_vehicle_observation_retains_active_variant_reducer_and_owner() {
    let mut fixture = active_variant();
    fixture.host.games.set_test_vehicle("x-plane-12", None);
    assert_eq!(fixture.host.vehicle_change(&fixture.run.owner), None);
    let health = fixture.host.health(&fixture.run).unwrap();
    assert!(health.stop.is_none());
    assert_eq!(fixture.host.authority.as_ref(), Some(&fixture.run.owner));
    let state = fixture.host.run(&fixture.run.owner).unwrap();
    assert_eq!(state.vehicle_selection.as_deref(), Some("alpha"));
    assert!(!state.vehicle_paused);
    assert!(state.reducer.as_ref().unwrap().active());
    assert_eq!(state.owner, fixture.run.owner);
}

#[test]
fn only_confirmed_changed_vehicle_selects_another_tab_or_base() {
    let mut fixture = active_variant();
    fixture.host.games.set_test_vehicle("x-plane-12", Some("vehicle-a"));
    assert_eq!(fixture.host.vehicle_change(&fixture.run.owner), None);
    fixture.host.games.set_test_vehicle("x-plane-12", Some("vehicle-b"));
    assert_eq!(fixture.host.vehicle_change(&fixture.run.owner).as_deref(), Some("beta"));
    fixture.host.games.set_test_vehicle("x-plane-12", Some("unconfigured"));
    assert_eq!(fixture.host.vehicle_change(&fixture.run.owner).as_deref(), Some("base"));
    fixture.host.run(&fixture.run.owner).unwrap().vehicle_selection = None;
    assert_eq!(fixture.host.vehicle_change(&fixture.run.owner), None);
}

#[test]
fn paused_variant_waits_for_confirmed_target_instead_of_guessing_base() {
    let mut fixture = active_variant();
    fixture.host.run(&fixture.run.owner).unwrap().vehicle_paused = true;
    fixture.host.games.set_test_vehicle("x-plane-12", None);
    assert_eq!(fixture.host.vehicle_change(&fixture.run.owner), None);
    assert!(fixture.host.health(&fixture.run).unwrap().degraded.iter()
        .any(|message| message.contains("preparing its profile settings")));
    assert!(fixture.host.run(&fixture.run.owner).unwrap().vehicle_paused);
    fixture.host.games.set_test_vehicle("x-plane-12", Some("vehicle-a"));
    assert_eq!(fixture.host.vehicle_change(&fixture.run.owner).as_deref(), Some("alpha"));
    fixture.host.games.set_test_vehicle("x-plane-12", Some("unconfigured"));
    assert_eq!(fixture.host.vehicle_change(&fixture.run.owner).as_deref(), Some("base"));
}
