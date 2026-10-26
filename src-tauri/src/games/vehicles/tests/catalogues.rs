use super::*;
#[test]
fn dcs_physics_errors_and_staleness_preserve_live_bios_and_missing_components_stay_unknown() {
    let bios = dcs_profile::ExportSnapshot {
        epoch: 1,
        aircraft: Some("A-10C".into()),
        mission_active: true,
        fresh: true,
        age_ms: Some(0),
        counter: Some(2),
        values: BTreeMap::from([("bios-zero".into(), json!(0))]),
        malformed_frames: 0,
    };
    let mut catalogue = json!({"channels":crate::dcs_physics::catalogue()});
    crate::game_channels::catalogue("dcs", &mut catalogue);
    let all = catalogue["channels"].as_array().unwrap();
    let mut capture = Capture::export("dcs", &bios);
    merge_ownship_capture(
        &mut capture,
        &bios,
        None,
        Some(&format!("Cannot bind port 7781\n{}", "x".repeat(600))),
    );
    assert_eq!(capture.status, "live");
    assert_eq!(capture.values["bios-zero"], 0);
    assert_eq!(capture.context["physicsStatus"], "error");
    let error = capture.context["physicsError"].as_str().unwrap();
    assert_eq!(error.chars().count(), 512);
    assert!(!error.chars().any(char::is_control));
    assert!(capture
        .message
        .as_ref()
        .unwrap()
        .contains("Cannot bind port 7781"));
    let rows = crate::game_telemetry_inspection::page(all, Some(&capture), 0, 256, "", "").0;
    assert_eq!(
        rows.iter()
            .find(|row| row["id"] == "flight.airspeed.true")
            .unwrap()["status"],
        "stale"
    );

    let physics = crate::dcs_physics::Snapshot {
        epoch: 1,
        session: Some("mission-1".into()),
        aircraft: Some("A-10C".into()),
        plane_id: Some(1),
        model_time: Some(10.0),
        sequence: Some(2),
        fresh: true,
        status: "live".into(),
        age_ms: Some(0),
        values: BTreeMap::from([("flight.airspeed.true".into(), json!(0))]),
        malformed_frames: 0,
        error: None,
    };
    let mut capture = Capture::export("dcs", &bios);
    merge_ownship_capture(&mut capture, &bios, Some(&physics), None);
    assert_eq!(capture.context["physicsStatus"], "live");
    assert_eq!(capture.source, "dcs-bios + ownship");
    assert!(capture.stale_ids.is_empty());
    let rows = crate::game_telemetry_inspection::page(all, Some(&capture), 0, 256, "", "").0;
    let speed = rows
        .iter()
        .find(|row| row["id"] == "flight.airspeed.true")
        .unwrap();
    assert_eq!(speed["status"], "known");
    assert_eq!(speed["value"], 0);
    let missing = rows.iter().find(|row| row["id"] == "flight.shake").unwrap();
    assert_eq!(missing["status"], "unknown");
    assert_eq!(missing["ageMs"], Value::Null);
}

#[test]
fn identity_uses_exact_game_ids_and_normalizes_xplane_paths() {
    for invalid in ["", " ", "NONE", "car\0name", "car\nname", &"a".repeat(257)] {
        assert_eq!(vehicle_id("dcs", invalid), None);
    }
    assert_eq!(vehicle_id("dcs", "A-10C_2"), Some("A-10C_2".into()));
    assert_eq!(
        vehicle_id("assetto-corsa-rally", "Lancia Delta"),
        Some("Lancia Delta".into())
    );
    assert_eq!(
        vehicle_id("x-plane-12", "Aircraft\\Laminar Research\\Cessna_172.acf"),
        Some("Aircraft/Laminar Research/Cessna_172.acf".into())
    );
}

#[test]
fn closed_games_list_dcs_aliases_and_installed_xplane_aircraft() {
    let scratch = scratch();
    let mut tracker = Tracker::new(scratch.path().join("vehicles.json"));
    let mut dcs = json!({"id":"dcs","integrationCompatible":true,"integrationKey":"dcs","provider":"dcs-bios"});
    tracker.observe(&dcs, &connection(5010), None, "setup", false, 1);
    tracker.decorate(&mut dcs);
    assert_eq!(dcs["currentVehicle"], Value::Null);
    assert_eq!(dcs["vehicleSupport"]["supported"], true);
    assert!(dcs["vehicleSupport"]["vehicles"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["id"] == "A-10C_2"));
    fs::create_dir_all(scratch.path().join("Aircraft/Laminar/Cessna")).unwrap();
    fs::write(
        scratch.path().join("Aircraft/Laminar/Cessna/C172.acf"),
        "I\n1200 Version\nACF",
    )
    .unwrap();
    fs::write(
        scratch.path().join("Aircraft/Laminar/Cessna/readme.txt"),
        "",
    )
    .unwrap();
    let mut xp = json!({"id":"x-plane-12","integrationCompatible":true,"integrationKey":"x-plane-12","provider":"x-plane","installPath":scratch.path()});
    tracker.observe(&xp, &json!({}), None, "setup", false, 2);
    tracker.decorate(&mut xp);
    assert_eq!(
        xp["vehicleSupport"]["vehicles"],
        json!([{"id":"Aircraft/Laminar/Cessna/C172.acf","name":"C172"}])
    );
}

#[test]
fn changing_selected_installation_hides_the_previous_live_car_before_observer_refresh() {
    let scratch = scratch();
    let mut tracker = Tracker::new(scratch.path().join("vehicles.json"));
    let mut game = json!({"id":"assetto-corsa","integrationCompatible":true,"integrationKey":"assetto-corsa","provider":"assetto-corsa","installed":true,"installPath":scratch.path().join("AC"),"executable":scratch.path().join("AC/acs.exe")});
    tracker.observe(&game, &json!({}), None, "setup", false, 1);
    tracker.entries.get_mut("assetto-corsa").unwrap().current =
        Some(json!({"id":"lotus_elise_sc","name":"lotus_elise_sc"}));
    tracker.decorate(&mut game);
    assert_eq!(game["currentVehicle"]["id"], "lotus_elise_sc");
    game["installPath"] = json!(scratch.path().join("Other AC"));
    game["executable"] = json!(scratch.path().join("Other AC/acs.exe"));
    tracker.decorate(&mut game);
    assert_eq!(game["currentVehicle"], Value::Null);
}

#[test]
fn dcs_no_aircraft_aliases_are_valid_but_never_listed_or_promoted_to_fresh_aircraft() {
    let scratch = scratch();
    for name in [
        "A-10C.json",
        "AircraftAliases.json",
        "MetadataStart.json",
        "MetadataEnd.json",
    ] {
        fs::copy(fixtures().join(name), scratch.path().join(name)).unwrap();
    }
    let aliases_path = scratch.path().join("AircraftAliases.json");
    let mut aliases: Value = serde_json::from_slice(&fs::read(&aliases_path).unwrap()).unwrap();
    for sentinel in ["", "NONE"] {
        aliases[sentinel] = json!(["CommonData", "FC3", "NS430"]);
    }
    fs::write(aliases_path, aliases.to_string()).unwrap();
    let reserve = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = reserve.local_addr().unwrap().port();
    drop(reserve);
    let mut connection = connection(port);
    connection["cataloguePath"] = json!(scratch.path());
    let vehicles = dcs_catalogue(&connection).unwrap();
    assert!(vehicles.iter().any(|vehicle| vehicle["id"] == "A-10C_2"));
    assert!(vehicles
        .iter()
        .all(|vehicle| vehicle["id"] != "NONE" && vehicle["id"] != ""));
    let mut reader = Reader::open("dcs", &connection, &process()).unwrap();
    let Reader::Dcs { decoder, .. } = &mut reader else {
        panic!("Expected a passive DCS reader");
    };
    decoder.feed(&frame("A-10C_2", 1), 0);
    assert!(decoder.snapshot(0).fresh);
    decoder.feed(&frame("NONE", 2), 100);
    assert!(!decoder.snapshot(100).fresh);
    decoder.feed(&frame("NONE", 3), 200);
    assert!(!decoder.snapshot(200).fresh);
    assert_eq!(reader.poll(200).unwrap(), None);
}

#[test]
fn introspection_and_profile_share_unicast_exports_without_stealing_frames() {
    assert!(dcs_bios::Network::parse(&connection(0)).is_err());
    let reserve = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = reserve.local_addr().unwrap().port();
    drop(reserve);
    let connection = connection(port);
    let network = dcs_bios::Network::parse(&connection).unwrap();
    let profile = dcs_bios::Transport::open_network(&network).unwrap();
    let mut observer = Reader::open("dcs", &connection, &process()).unwrap();
    let producer = UdpSocket::bind("127.0.0.1:0").unwrap();
    let first = frame("A-10C", 1);
    producer.send_to(&first, ("127.0.0.1", port)).unwrap();
    assert_eq!(receive(&profile), Some(first));
    assert_eq!(poll(&mut observer, 0), Some("A-10C".into()));
    let second = frame("A-10C_2", 2);
    producer.send_to(&second, ("127.0.0.1", port)).unwrap();
    for _ in 0..100 {
        if observer.poll(100).unwrap() == Some("A-10C_2".into()) {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(observer.poll(100).unwrap(), Some("A-10C_2".into()));
    assert_eq!(receive(&profile), Some(second));
    assert_eq!(observer.poll(1601).unwrap(), None);
}

#[test]
fn generic_title_aliases_use_bound_vehicle_dialects_and_keep_independent_title_maps() {
    let scratch = scratch();
    let mut tracker = Tracker::new(scratch.path().join("vehicles.json"));
    let mut dcs = json!({"id":"my-flight-sim","integrationCompatible":true,"integrationKey":"dcs","provider":"dcs-bios"});
    tracker.observe(&dcs, &connection(5010), None, "setup", false, 1);
    tracker.decorate(&mut dcs);
    assert_eq!(dcs["vehicleSupport"]["supported"], true);
    assert!(dcs["vehicleSupport"]["vehicles"]
        .as_array()
        .unwrap()
        .iter()
        .any(|vehicle| vehicle["id"] == "A-10C_2"));
    assert!(!tracker.entries.contains_key("dcs"));
    fs::create_dir_all(scratch.path().join("Aircraft/Custom Plane")).unwrap();
    fs::write(
        scratch.path().join("Aircraft/Custom Plane/My_Plane.acf"),
        "I\n1200 Version\nACF",
    )
    .unwrap();
    let mut xp = json!({"id":"another-flight-sim","integrationCompatible":true,"integrationKey":"x-plane-11","provider":"x-plane","installPath":scratch.path()});
    tracker.observe(&xp, &json!({}), None, "setup", false, 2);
    tracker.decorate(&mut xp);
    assert_eq!(
        xp["vehicleSupport"]["vehicles"],
        json!([{"id":"Aircraft/Custom Plane/My_Plane.acf","name":"My Plane"}])
    );
    let mut ac = json!({"id":"racing-title","integrationCompatible":true,"integrationKey":"assetto-corsa","provider":"assetto-corsa"});
    tracker.observe(&ac, &json!({}), None, "setup", false, 3);
    tracker.decorate(&mut ac);
    assert_eq!(ac["vehicleSupport"]["kind"], "car");
    assert_eq!(tracker.entries["racing-title"].integration, "assetto-corsa");
    ac["integrationKey"] = Value::Null;
    tracker.decorate(&mut ac);
    assert_eq!(ac["vehicleSupport"]["supported"], false);
}
#[test]
fn remembered_vehicle_titles_use_generic_ids_and_remain_bounded() {
    let mut games = serde_json::Map::new();
    for index in 0..512 {
        games.insert(format!("title-{index}"), json!([]));
    }
    assert!(validate_saved(&json!({"schemaVersion":1,"games":games})).is_ok());
    games.insert("one-more".into(), json!([]));
    assert!(validate_saved(&json!({"schemaVersion":1,"games":games})).is_err());
}
