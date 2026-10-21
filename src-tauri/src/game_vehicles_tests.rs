use super::*;
use std::net::UdpSocket;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dcs-bios")
}
fn connection(port: u16) -> Value {
    json!({"cataloguePath":fixtures(),"commandHost":"127.0.0.1","commandPort":7778,
        "listenAddress":"127.0.0.1","listenPort":port,"multicastGroup":"","multicastInterface":"0.0.0.0","freshnessMs":1500})
}
fn process() -> ProcessIdentity {
    ProcessIdentity {
        pid: 42,
        path: "DCS.exe".into(),
        epoch: 10,
    }
}
fn block(bytes: &mut Vec<u8>, address: u16, data: &[u8]) {
    bytes.extend_from_slice(&address.to_le_bytes());
    bytes.extend_from_slice(&(data.len() as u16).to_le_bytes());
    bytes.extend_from_slice(data);
}
fn frame(name: &str, counter: u8) -> Vec<u8> {
    let mut bytes = vec![0x55; 4];
    let mut data = vec![0; 24];
    data[..name.len()].copy_from_slice(name.as_bytes());
    block(&mut bytes, 0, &data);
    block(&mut bytes, 65534, &[counter, 0]);
    bytes
}
fn scratch() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}
fn receive(source: &dcs_bios::Transport) -> Option<Vec<u8>> {
    for _ in 0..100 {
        if let Some(bytes) = source.receive().unwrap() {
            return Some(bytes);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    None
}
fn poll(reader: &mut Reader, now_ms: u64) -> Option<String> {
    for _ in 0..100 {
        if let Some(vehicle) = reader.poll(now_ms).unwrap() {
            return Some(vehicle);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    None
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
    let mut dcs = json!({"id":"dcs"});
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
    let mut xp = json!({"id":"x-plane-12","installPath":scratch.path()});
    tracker.observe(&xp, &json!({}), None, "setup", false, 2);
    tracker.decorate(&mut xp);
    assert_eq!(
        xp["vehicleSupport"]["vehicles"],
        json!([{"id":"Aircraft/Laminar/Cessna/C172.acf","name":"C172"}])
    );
}

#[test]
fn dcs_no_aircraft_aliases_are_valid_but_never_listed_or_promoted_to_fresh_aircraft() {
    let scratch = scratch();
    for name in ["A-10C.json", "AircraftAliases.json", "MetadataStart.json", "MetadataEnd.json"] {
        fs::copy(fixtures().join(name), scratch.path().join(name)).unwrap();
    }
    let aliases_path = scratch.path().join("AircraftAliases.json");
    let mut aliases: Value = serde_json::from_slice(&fs::read(&aliases_path).unwrap()).unwrap();
    for sentinel in ["", "NONE"] { aliases[sentinel] = json!(["CommonData", "FC3", "NS430"]); }
    fs::write(aliases_path, aliases.to_string()).unwrap();
    let reserve = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = reserve.local_addr().unwrap().port();
    drop(reserve);
    let mut connection = connection(port);
    connection["cataloguePath"] = json!(scratch.path());
    let vehicles = dcs_catalogue(&connection).unwrap();
    assert!(vehicles.iter().any(|vehicle|vehicle["id"] == "A-10C_2"));
    assert!(vehicles.iter().all(|vehicle|vehicle["id"] != "NONE" && vehicle["id"] != ""));
    let mut reader = Reader::open("dcs", &connection, &process()).unwrap();
    let Reader::Dcs {decoder, ..} = &mut reader else { panic!("Expected a passive DCS reader"); };
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
fn observed_vehicle_is_remembered_but_never_restored_as_current_or_accepted_from_old_scan() {
    let scratch = scratch();
    let path = scratch.path().join("vehicles.json");
    let mut tracker = Tracker::new(path.clone());
    let dcs = json!({"id":"dcs"});
    let mut network = dcs_bios::Network::parse(&connection(5010)).unwrap();
    network.listen_port = 0;
    let source = dcs_bios::Transport::open_network(&network).unwrap();
    let address = source.local_addr().unwrap();
    let mut query = connection(5010);
    query["aircraftModule"] = json!("A-10C");
    let ports = dcs_profile::resource_catalogue(&query).unwrap();
    let (_, guard, catalogue) = dcs_profile::guarded_telemetry_sources(
        &fixtures(),
        "A-10C",
        &[],
        ports["sourceRevision"].as_str().unwrap(),
    )
    .unwrap();
    let decoder = dcs_profile::ExportSession::from_sources(catalogue, 1500, vec![]).unwrap();
    tracker.observe(&dcs, &connection(5010), Some(process()), "setup", false, 2);
    tracker.entries.get_mut("dcs").unwrap().reader = Some(Reader::Dcs {
        source,
        decoder,
        guard,
    });
    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .send_to(&frame("A-10C", 1), address)
        .unwrap();
    for _ in 0..100 {
        tracker.poll();
        if tracker.current("dcs").is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(tracker.current("dcs").unwrap()["id"], "A-10C");
    tracker.observe(&dcs, &connection(5010), None, "setup", false, 1);
    assert!(tracker.current("dcs").is_some());
    tracker.observe(&dcs, &connection(5010), None, "setup", false, 3);
    assert_eq!(tracker.current("dcs"), None);
    let restored = Tracker::new(path);
    assert_eq!(restored.current("dcs"), None);
    let mut dcs = dcs;
    restored.decorate(&mut dcs);
    assert_eq!(dcs["vehicleSupport"]["vehicles"][0]["id"], "A-10C");
}
#[test]
fn independent_process_checks_survive_slow_catalogue_observation_but_never_extend_telemetry_or_changed_identity() {
    for changed in ["process", "setup", "installation"] {
        let scratch = scratch();
        let reserve = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = reserve.local_addr().unwrap().port();
        drop(reserve);
        let game = json!({"id":"dcs","installed":true,"executable":"DCS.exe"});
        let games = vec![game.clone()];
        let mut tracker = Tracker::new(scratch.path().join("vehicles.json"));
        tracker.observe(&game, &connection(port), Some(process()), "setup", true, 1);
        let producer = UdpSocket::bind("127.0.0.1:0").unwrap();
        producer.send_to(&frame("A-10C_2", 1), ("127.0.0.1", port)).unwrap();
        for _ in 0..100 {
            tracker.poll_verified(&games, |_|"setup".into(), |captured|captured == &process());
            if tracker.current("dcs").is_some() { break; }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(tracker.current("dcs").unwrap()["id"], "A-10C_2");
        // Simulate the expensive catalogue/setup observer being blocked beyond
        // its normal freshness lease. The independent exact process check and
        // advancing export retain the same aircraft without a Base transition.
        tracker.entries.get_mut("dcs").unwrap().observed -= Duration::from_secs(3);
        producer.send_to(&frame("A-10C_2", 2), ("127.0.0.1", port)).unwrap();
        for _ in 0..100 {
            tracker.poll_verified(&games, |_|"setup".into(), |captured|captured == &process());
            if matches!(tracker.entries["dcs"].reader.as_ref(), Some(Reader::Dcs {decoder,..}) if decoder.snapshot(0).counter == Some(2)) { break; }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(tracker.current("dcs").unwrap()["id"], "A-10C_2");
        tracker.origin -= Duration::from_millis(1501);
        tracker.poll_verified(&games, |_|"setup".into(), |_|true);
        assert_eq!(tracker.current("dcs"), None, "Process liveness cannot renew stale telemetry");
        producer.send_to(&frame("A-10C_2", 3), ("127.0.0.1", port)).unwrap();
        for _ in 0..100 {
            tracker.poll_verified(&games, |_|"setup".into(), |_|true);
            if tracker.current("dcs").is_some() { break; }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(tracker.current("dcs").is_some());
        let mut current_games = games;
        let mut current_process = process();
        let mut revision = "setup";
        match changed {
            "process" => current_process.epoch += 1,
            "setup" => revision = "new-setup",
            _ => current_games[0]["executable"] = json!("different/DCS.exe"),
        }
        tracker.poll_verified(&current_games, |_|revision.into(), |captured|captured == &current_process);
        assert_eq!(tracker.current("dcs"), None, "A changed {changed} must immediately withdraw the aircraft");
        assert!(tracker.entries["dcs"].reader.is_none());
    }
}

fn rally_sample(packet: u32, car: &str) -> game_telemetry::RawSample {
    let mut sample = game_telemetry::RawSample {
        source: game_telemetry::SourceIdentity {
            id: "fixture-rally".into(),
            epoch: 1,
        },
        physics: vec![0; game_telemetry::PHYSICS_SIZE],
        graphics: vec![0; game_telemetry::GRAPHICS_SIZE],
        statics: vec![0; game_telemetry::STATIC_SIZE],
    };
    sample.physics[0..4].copy_from_slice(&packet.to_le_bytes());
    sample.graphics[4..8].copy_from_slice(&2i32.to_le_bytes());
    for (index, unit) in car.encode_utf16().enumerate() {
        sample.statics[68 + index * 2..70 + index * 2].copy_from_slice(&unit.to_le_bytes());
    }
    sample
}

#[test]
fn rally_catalogue_remembers_only_qualified_native_car_models_and_stale_withdraws_selection() {
    let scratch = scratch();
    let path = scratch.path().join("vehicles.json");
    let mut tracker = Tracker::new(path.clone());
    let game = json!({"id":"assetto-corsa-rally"});
    tracker.observe(&game, &json!({}), Some(process()), "rally-setup", false, 1);
    let mut source = game_telemetry::FixtureSource::new();
    source.push(rally_sample(1, "Lancia Delta")).unwrap();
    source.push(rally_sample(2, "Lancia Delta")).unwrap();
    tracker
        .entries
        .get_mut("assetto-corsa-rally")
        .unwrap()
        .reader = Some(Reader::Rally {
        source: Box::new(source),
        decoder: game_telemetry::Session::new(1500).unwrap(),
    });
    tracker.poll();
    assert_eq!(tracker.current("assetto-corsa-rally"), None);
    tracker.poll();
    assert_eq!(
        tracker.current("assetto-corsa-rally").unwrap()["id"],
        "Lancia Delta"
    );
    tracker.origin -= Duration::from_millis(1501);
    tracker.poll();
    assert_eq!(tracker.current("assetto-corsa-rally"), None);
    let restored = Tracker::new(path);
    let mut game = game;
    restored.decorate(&mut game);
    assert_eq!(game["currentVehicle"], Value::Null);
    assert_eq!(
        game["vehicleSupport"]["vehicles"][0]["name"],
        "Lancia Delta"
    );
}

#[test]
fn shared_export_subscriber_overflow_is_an_explicit_error_and_new_subscribers_start_empty() {
    let reserve = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = reserve.local_addr().unwrap().port();
    drop(reserve);
    let network = dcs_bios::Network::parse(&connection(port)).unwrap();
    let fast = dcs_bios::Transport::open_network(&network).unwrap();
    let slow = dcs_bios::Transport::open_network(&network).unwrap();
    let producer = UdpSocket::bind("127.0.0.1:0").unwrap();
    for _ in 0..257 {
        producer
            .send_to(b"frame", fast.local_addr().unwrap())
            .unwrap();
        assert_eq!(receive(&fast), Some(b"frame".to_vec()));
    }
    assert!(slow.receive().unwrap_err().contains("overflow"));
    drop(slow);
    let replacement = dcs_bios::Transport::open_network(&network).unwrap();
    assert_eq!(replacement.receive().unwrap(), None);
}
