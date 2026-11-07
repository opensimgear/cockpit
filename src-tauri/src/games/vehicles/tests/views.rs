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
    entry.telemetry = Some(Arc::new(capture()));

    let committed = tracker.telemetry(&game).unwrap();
    assert_eq!(committed.packet, json!(7));
    assert!(Arc::ptr_eq(&committed, &tracker.telemetry(&game).unwrap()));
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
    let game = json!({"id":"x-plane-12","integrationCompatible":true,
        "integrationKey":"x-plane-12","provider":"x-plane"});
    tracker.observe(&game, &json!({}), None, "setup", true, 1);

    tracker.inspect("missing-title", "first", vec!["one".into()], None).unwrap();
    tracker.inspect("x-plane-12", "first", vec!["one".into()], None).unwrap();
    tracker.inspect("x-plane-12", "second", vec!["two".into()], None).unwrap();
    let selected = |tracker: &mut Tracker| tracker.entries.get_mut("x-plane-12")
        .unwrap().inspection.selection(Instant::now()).to_vec();
    assert_eq!(selected(&mut tracker), ["one", "two"]);
    tracker.inspect("x-plane-12", "first", vec![], None).unwrap();
    assert_eq!(selected(&mut tracker), ["two"]);
    tracker.observe(&game, &json!({}), None, "replacement", true, 2);
    assert!(selected(&mut tracker).is_empty());
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
fn physics_watches_reuse_the_exact_attached_or_detached_owner_without_retaining_its_socket() {
    let scratch = scratch();
    let mut tracker = Tracker::new(scratch.path().join("vehicles.json"));
    #[cfg(windows)]
    let process = crate::xplane::process_events::ExitWatch::current_process_fixture();
    #[cfg(not(windows))]
    let process = process();
    let game = json!({"id":"dcs","integrationCompatible":true,"integrationKey":"dcs","provider":"dcs-bios"});
    tracker.observe(&game, &connection(5010), Some(process.clone()), "setup", true, 1);
    let physics = crate::dcs_physics::Source::bind_loopback_ephemeral().unwrap();
    let address = physics.local_addr();
    let physics_view = physics.view();
    let mut network = dcs_bios::Network::parse(&connection(5010)).unwrap();
    network.listen_port = 0;
    let (decoder, guard, catalogue) = dcs_profile::passive_telemetry_sources(&fixtures(), "A-10C", 1500).unwrap();
    let bios = dcs_bios::decoded::Source::open(&network, decoder.decode_spec().unwrap()).unwrap();
    tracker.entries.get_mut("dcs").unwrap().reader = Some(Reader::Dcs {
        process_exit: crate::xplane::process_events::ExitWatch::open(&process).ok(),
        source: bios, decoder, guard, connection: connection(5010), module: "A-10C".into(),
        catalogue, physics: Some(physics), physics_error: None, physics_retry: Instant::now(),
        resolved_aircraft: None, capture_dirty: false,
    });
    let peer = Arc::new(osg_streams::NativeWake::default());
    let mut saturated = Vec::new();
    for _ in 0..64 {
        saturated.push(physics_view.watch(peer.clone()).unwrap());
    }
    assert!(matches!(physics_view.watch(peer), Err(osg_streams::StreamError::SubscriberLimit)));
    let capture_wake = Arc::new(osg_streams::NativeWake::default());
    capture_wake.bind_thread(std::thread::current()).unwrap();
    tracker.watch_captures(capture_wake.clone());
    #[cfg(windows)]
    assert!(!tracker.needs_process_maintenance(), "The exact exit watch stays authoritative");
    let before = capture_wake.observed();
    let packet = json!({"schemaVersion":1,"source":"osg-dcs-ownship","session":"tracker-fixture",
        "sequence":1,"modelTime":1.0,"planeId":5,"aircraft":"A-10C","allowed":true,
        "paused":false,"activePause":false,"stopped":false,"values":{"flight.airspeed.true":20}});
    UdpSocket::bind("127.0.0.1:0").unwrap().send_to(packet.to_string().as_bytes(), address).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while capture_wake.observed() == before {
        assert!(Instant::now() < deadline, "Ownship capture must wake the vehicle tracker");
        capture_wake.wait(before, deadline.saturating_duration_since(Instant::now()));
    }
    assert_eq!(tracker.dcs_physics(&process).unwrap().sequence, Some(1));
    drop(saturated);
    let wake = std::sync::Arc::new(osg_streams::NativeWake::default());
    assert!(tracker.dcs_physics_watch(&ProcessIdentity { epoch: process.epoch ^ 1, ..process.clone() }, wake.clone()).unwrap().is_none());
    assert_eq!(wake.observed(), 0);
    let attached = tracker.dcs_physics_watch(&process, wake.clone()).unwrap().unwrap();
    assert_eq!(wake.observed(), 1);
    let entry = tracker.entries.get_mut("dcs").unwrap();
    let owner = entry.reader.take();
    entry.polling = true;
    entry.detached_physics = Some(physics_view);
    let detached = tracker.dcs_physics_watch(&process, wake.clone()).unwrap().unwrap();
    assert_eq!(wake.observed(), 2);
    let entry = tracker.entries.get_mut("dcs").unwrap();
    entry.enabled = false;
    assert!(tracker.dcs_physics_watch(&process, wake.clone()).unwrap().is_none());
    let entry = tracker.entries.get_mut("dcs").unwrap();
    entry.enabled = true;
    entry.capture_watch = None;
    entry.observed -= Duration::from_secs(3);
    assert!(tracker.dcs_physics_watch(&process, wake).unwrap().is_none());
    drop(owner);
    assert!(attached.is_closed() && detached.is_closed());
    UdpSocket::bind(address).unwrap();
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
