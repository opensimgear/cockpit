use super::*;

fn capture() -> Capture {
    Capture {
        status: "live".into(),
        source: "fixture".into(),
        context: json!({"aircraft":"FA-18C_hornet"}),
        packet: json!(7),
        age_ms: Some(0),
        values: BTreeMap::new(),
        ages: BTreeMap::new(),
        stale_ids: Default::default(),
        message: None,
        at: Instant::now(),
    }
}

fn rally_reader() -> Reader {
    Reader::Rally {
        source: Box::new(game_telemetry::FixtureSource::new()),
        decoder: game_telemetry::Session::new(1500).unwrap(),
    }
}

#[test]
fn telemetry_requires_a_fresh_enabled_reader_for_the_same_installation() {
    let scratch = scratch();
    let mut tracker = Tracker::new(scratch.path().join("vehicles.json"));
    let game = json!({"id":"assetto-corsa-rally","integrationCompatible":true,
        "integrationKey":"assetto-corsa-rally","provider":"assetto-corsa-rally",
        "installPath":"/sim/rally"});
    tracker.observe(&game, &json!({}), None, "setup", true, 1);
    let entry = tracker.entries.get_mut("assetto-corsa-rally").unwrap();
    entry.reader = Some(rally_reader());
    entry.telemetry = Some(capture());

    assert_eq!(tracker.telemetry(&game).unwrap().packet, json!(7));
    let mut other_install = game.clone();
    other_install["installPath"] = json!("/sim/other");
    assert!(tracker.telemetry(&other_install).is_none());

    tracker.entries.get_mut("assetto-corsa-rally").unwrap().enabled = false;
    assert!(tracker.telemetry(&game).is_none());
    let entry = tracker.entries.get_mut("assetto-corsa-rally").unwrap();
    entry.enabled = true;
    entry.observed -= Duration::from_secs(3);
    assert!(tracker.telemetry(&game).is_none());

    let entry = tracker.entries.get_mut("assetto-corsa-rally").unwrap();
    entry.observed = Instant::now();
    entry.reader = None;
    assert!(tracker.telemetry(&game).is_none());
    assert!(tracker.telemetry(&json!({})).is_none());
}

#[test]
fn view_inspection_is_idempotent_and_missing_titles_are_harmless() {
    let scratch = scratch();
    let mut tracker = Tracker::new(scratch.path().join("vehicles.json"));
    let game = json!({"id":"assetto-corsa-rally","integrationCompatible":true,
        "integrationKey":"assetto-corsa-rally","provider":"assetto-corsa-rally"});
    tracker.observe(&game, &json!({}), None, "setup", true, 1);
    tracker.entries.get_mut("assetto-corsa-rally").unwrap().reader = Some(rally_reader());
    let ids = vec!["car.speedKmh".to_owned()];

    tracker.inspect("missing-title", ids.clone()).unwrap();
    tracker.inspect("assetto-corsa-rally", ids.clone()).unwrap();
    let inspected = tracker.entries["assetto-corsa-rally"].inspected;
    tracker.inspect("assetto-corsa-rally", ids.clone()).unwrap();
    assert_eq!(tracker.entries["assetto-corsa-rally"].inspection, ids);
    assert!(tracker.entries["assetto-corsa-rally"].inspected >= inspected);
}

#[test]
fn physics_view_requires_a_fresh_matching_dcs_reader() {
    let scratch = scratch();
    let mut tracker = Tracker::new(scratch.path().join("vehicles.json"));
    let process = process();
    let game = json!({"id":"dcs","integrationCompatible":true,"integrationKey":"dcs",
        "provider":"dcs-bios"});
    tracker.observe(&game, &json!({}), Some(process.clone()), "setup", true, 1);

    // A reader for another integration cannot supply DCS ownship state even
    // when the process identity and lease match.
    tracker.entries.get_mut("dcs").unwrap().reader = Some(rally_reader());
    assert!(tracker.dcs_physics(&process).is_none());
    assert!(tracker.dcs_physics(&ProcessIdentity { epoch: 11, ..process.clone() }).is_none());

    tracker.entries.get_mut("dcs").unwrap().integration = "assetto-corsa-rally".into();
    assert!(tracker.dcs_physics(&process).is_none());
    let entry = tracker.entries.get_mut("dcs").unwrap();
    entry.integration = "dcs".into();
    entry.enabled = false;
    assert!(tracker.dcs_physics(&process).is_none());
    let entry = tracker.entries.get_mut("dcs").unwrap();
    entry.enabled = true;
    entry.observed -= Duration::from_secs(3);
    assert!(tracker.dcs_physics(&process).is_none());
}

#[test]
fn decoration_reports_binding_errors_and_merges_catalogue_with_retained_vehicles() {
    let scratch = scratch();
    let mut tracker = Tracker::new(scratch.path().join("vehicles.json"));
    let mut unsupported = json!({"id":"unknown-flight-title","integrationCompatible":true,
        "integrationKey":"dcs","provider":"wrong-provider"});
    tracker.decorate(&mut unsupported);
    assert_eq!(unsupported["vehicleSupport"]["supported"], false);
    assert_eq!(unsupported["vehicleSupport"]["kind"], "aircraft");
    assert!(unsupported["vehicleSupport"]["message"].as_str().unwrap().contains("Aircraft"));
    assert_eq!(unsupported["vehicleStatus"]["status"], "waiting");

    let game = json!({"id":"dcs","integrationCompatible":true,"integrationKey":"dcs",
        "provider":"dcs-bios"});
    tracker.observe(&game, &json!({}), None, "setup", true, 1);
    tracker.seen.insert("dcs".into(), BTreeMap::from([
        ("FA-18C_hornet".into(), json!({"id":"FA-18C_hornet","name":"Saved Hornet"})),
    ]));
    let entry = tracker.entries.get_mut("dcs").unwrap();
    entry.vehicles = vec![json!({"id":"FA-18C_hornet","name":"Installed Hornet"})];
    entry.error = Some("DCS-BIOS catalogue unavailable".into());
    let mut game = game;
    tracker.decorate(&mut game);
    assert_eq!(game["vehicleSupport"]["message"], "Aircraft come from the installed DCS-BIOS catalogue. Current aircraft requires fresh DCS-BIOS exports.");
    assert_eq!(game["vehicleSupport"]["vehicles"], json!([
        {"id":"FA-18C_hornet","name":"Installed Hornet"}
    ]));
    assert_eq!(game["vehicleStatus"]["status"], "unavailable");
    assert_eq!(game["vehicleStatus"]["message"], "DCS-BIOS catalogue unavailable");
    assert_eq!(game["currentVehicle"], Value::Null);
}

#[test]
fn xplane_decoration_exposes_current_aircraft_only_for_the_selected_installation() {
    let scratch = scratch();
    let mut tracker = Tracker::new(scratch.path().join("vehicles.json"));
    let mut game = json!({"id":"x-plane-12","integrationCompatible":true,"integrationKey":"x-plane-12",
        "provider":"x-plane","installPath":"/sim/xplane"});
    tracker.observe(&game, &json!({}), None, "setup", false, 1);
    tracker.set_test_vehicle("x-plane-12", Some("Aircraft\\Laminar\\C172.acf"));
    tracker.decorate(&mut game);
    assert_eq!(game["vehicleSupport"]["supported"], true);
    assert_eq!(game["vehicleSupport"]["kind"], "aircraft");
    assert_eq!(game["vehicleSupport"]["message"], "Aircraft come from this installation's Aircraft folder. Current aircraft requires fresh RREF telemetry.");
    assert_eq!(game["currentVehicle"]["id"], "Aircraft/Laminar/C172.acf");
    assert_eq!(game["vehicleStatus"]["status"], "ready");

    game["installPath"] = json!("/sim/second-xplane");
    tracker.decorate(&mut game);
    assert_eq!(game["currentVehicle"], Value::Null);
    assert_eq!(game["vehicleStatus"]["status"], "waiting");
}
