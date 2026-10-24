use super::*;
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
        connection: query,
        module: "A-10C".into(),
        catalogue: ports,
        physics: None,
        physics_error: None,
        physics_retry: Instant::now() + Duration::from_secs(60),
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
    tracker.writer.as_ref().unwrap().flush().unwrap();
    let restored = Tracker::new(path);
    assert_eq!(restored.current("dcs"), None);
    let mut dcs = dcs;
    restored.decorate(&mut dcs);
    assert_eq!(dcs["vehicleSupport"]["vehicles"][0]["id"], "A-10C");
}
#[test]
fn independent_process_checks_survive_slow_catalogue_observation_but_never_extend_telemetry_or_changed_identity(
) {
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
        producer
            .send_to(&frame("A-10C_2", 1), ("127.0.0.1", port))
            .unwrap();
        for _ in 0..100 {
            tracker.poll_verified(
                &games,
                |_| "setup".into(),
                |captured| captured == &process(),
            );
            if tracker.current("dcs").is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(tracker.current("dcs").unwrap()["id"], "A-10C_2");
        // Simulate the expensive catalogue/setup observer being blocked beyond
        // its normal freshness lease. The independent exact process check and
        // advancing export retain the same aircraft without a Base transition.
        tracker.entries.get_mut("dcs").unwrap().observed -= Duration::from_secs(3);
        producer
            .send_to(&frame("A-10C_2", 2), ("127.0.0.1", port))
            .unwrap();
        for _ in 0..100 {
            tracker.poll_verified(
                &games,
                |_| "setup".into(),
                |captured| captured == &process(),
            );
            if matches!(tracker.entries["dcs"].reader.as_ref(), Some(Reader::Dcs {decoder,..}) if decoder.snapshot(0).counter == Some(2))
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(tracker.current("dcs").unwrap()["id"], "A-10C_2");
        tracker.origin -= Duration::from_millis(1501);
        tracker.poll_verified(&games, |_| "setup".into(), |_| true);
        assert_eq!(
            tracker.current("dcs"),
            None,
            "Process liveness cannot renew stale telemetry"
        );
        producer
            .send_to(&frame("A-10C_2", 3), ("127.0.0.1", port))
            .unwrap();
        for _ in 0..100 {
            tracker.poll_verified(&games, |_| "setup".into(), |_| true);
            if tracker.current("dcs").is_some() {
                break;
            }
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
        tracker.poll_verified(
            &current_games,
            |_| revision.into(),
            |captured| captured == &current_process,
        );
        assert_eq!(
            tracker.current("dcs"),
            None,
            "A changed {changed} must immediately withdraw the aircraft"
        );
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
    tracker.writer.as_ref().unwrap().flush().unwrap();
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

#[test]
fn invalid_retention_is_visible_without_blocking_live_observations_or_rewriting_evidence() {
    let directory = crate::test_directory();
    let path = directory.path().join("vehicles.json");
    fs::write(&path, b"{invalid").unwrap();
    crate::persistence::test_support::record(&path,"games","vehicles",&std::fs::read(&path).unwrap());
    let mut tracker = Tracker::new(path.clone());
    let mut game = json!({"id":"assetto-corsa-rally"});
    tracker.set_test_vehicle("assetto-corsa-rally", Some("Lancia Delta"));
    tracker.decorate(&mut game);
    assert_eq!(game["currentVehicle"]["id"], "Lancia Delta");
    assert!(game["vehicleSupport"]["retentionError"].is_string());
    assert!(tracker.writer.is_none());
    assert_eq!(fs::read(path).unwrap(), b"{invalid");
}
#[test]
fn remembered_vehicle_import_is_bounded_validated_and_never_restores_live_authority() {
    let directory = crate::test_directory();
    let path = directory.path().join("vehicles.json");
    let saved = json!({"schemaVersion":1,"games":{"dcs":[{"id":"A-10C","name":"A-10C"}],"assetto-corsa":[{"id":"lotus","name":"lotus"}]}});
    fs::write(&path, saved.to_string()).unwrap();
    crate::persistence::test_support::record(&path,"games","vehicles",&std::fs::read(&path).unwrap());
    let mut tracker = Tracker::new(path.clone());
    assert!(tracker.retention_error.is_none());
    assert_eq!(tracker.current("dcs"), None);
    fs::write(&path, b"retired").unwrap();
    let restored = Tracker::new(path.clone());
    assert_eq!(restored.seen, tracker.seen);
    for invalid in [
        json!({"schemaVersion":2,"games":{}}),
        json!({"schemaVersion":1,"games":{"unknown":[]}}),
        json!({"schemaVersion":1,"games":{"dcs":[{"id":"NONE"}]}}),
        json!({"schemaVersion":1,"games":{"dcs":[{"id":"A"},{"id":"A"}]}}),
        json!({"schemaVersion":1,"games":{"dcs":vec![json!({"id":"A"});MAX_VEHICLES+1]}}),
    ] {
        assert!(validate_saved(&invalid).is_err());
    }
    let db = osg_persistence::Database::for_namespace(&path).unwrap();
    let lock = osg_persistence::test_support::hold_writer(&db).unwrap();
    let start = Instant::now();
    tracker
        .writer
        .as_ref()
        .unwrap()
        .queue(saved.clone())
        .unwrap();
    assert!(start.elapsed() < Duration::from_millis(100));
    drop(lock);
    tracker.writer.as_ref().unwrap().flush().unwrap();
    tracker.seen.clear();
}

#[test]
fn injected_vehicle_remains_until_fixture_changes_it() {
    let directory = crate::test_directory();
    let mut tracker = Tracker::new(directory.path().join("vehicles.json"));
    tracker.set_test_vehicle("dcs", Some("FA-18C_hornet"));
    tracker.entries.get_mut("dcs").unwrap().observed =
        Instant::now() - Duration::from_secs(3);
    tracker.poll_verified(&[], |_| String::new(), |_| false);
    assert_eq!(tracker.current("dcs").unwrap()["id"], "FA-18C_hornet");
    tracker.set_test_vehicle("dcs", None);
    assert_eq!(tracker.current("dcs"), None);
}
