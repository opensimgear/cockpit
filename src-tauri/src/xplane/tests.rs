use super::*;

// send_to only queues a datagram for delivery. In particular, macOS can return
// WouldBlock when the nonblocking receiver is polled immediately afterwards.
// Synchronize at the socket boundary so a baseline is consumed before sending
// its advancing frame, without changing the production polling behavior.
fn poll_after_delivery(source: &mut Source, now: Instant) -> (ExportSnapshot, Value) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut byte = [0u8; 1];
    loop {
        match source.socket.peek(&mut byte) {
            Ok(_) => return source.poll(now),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "Timed out waiting for UDP fixture delivery"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("Cannot observe UDP fixture delivery: {error}"),
        }
    }
}
#[test]
fn authored_aircraft_fixtures_decode_through_production_wire_and_channel_mapping() {
    for text in [
        include_str!("../../tests/fixtures/x-plane-11-rref.json"),
        include_str!("../../tests/fixtures/x-plane-12-rref.json"),
    ] {
        let fixture: Value = serde_json::from_str(text).unwrap();
        let version = fixture["version"].as_u64().unwrap();
        let mut s = Session::new(&defaults(version));
        let now = Instant::now();
        for clock in [1., 2.] {
            let mut values = frame(version, clock, fixture["aircraft"].as_str().unwrap());
            values[4].1 = fixture["engines"].as_f64().unwrap();
            values.retain(|(i, _)| *i < GUARDS.len() + PATH_BYTES);
            for (i, c) in CHANNELS.iter().enumerate() {
                if let Some(value) = fixture["channels"].get(c.id) {
                    values.push((
                        GUARDS.len() + PATH_BYTES + i,
                        value
                            .as_f64()
                            .unwrap_or_else(|| if value == true { 1. } else { 0. }),
                    ));
                }
            }
            ingest(&mut s, &values, now);
            s.snapshot(now, &outputs());
        }
        let (snapshot, status, _) = s.snapshot(now, &outputs());
        assert!(snapshot.fresh);
        assert_eq!(snapshot.aircraft.as_deref(), fixture["aircraft"].as_str());
        for (id, value) in fixture["channels"].as_object().unwrap() {
            if value.is_number() {
                assert_eq!(status["channels"][id]["value"].as_f64(), value.as_f64());
            } else {
                assert_eq!(status["channels"][id]["value"], *value);
            }
        }
    }
}
#[test]
fn simulated_udp_session_renews_unsubscribes_and_disarms_on_errors_and_exit() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket2::SockRef::from(&server)
        .set_recv_buffer_size(1024 * 1024)
        .unwrap();
    server
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut connection = defaults(11);
    connection["port"] = json!(server.local_addr().unwrap().port());
    let alive = Arc::new(AtomicBool::new(true));
    let monitored = alive.clone();
    let mut source = Source::connect(
        &connection,
        outputs(),
        vec![true; subscriptions().len()],
        Box::new(move || {
            if monitored.load(Ordering::Acquire) {
                Ok(())
            } else {
                Err("Process exited".into())
            }
        }),
    )
    .unwrap();
    let receive = || {
        let mut indexes = BTreeMap::new();
        let mut peer = None;
        let mut bytes = [0; 512];
        for _ in subscriptions() {
            let (n, p) = server.recv_from(&mut bytes).unwrap();
            assert_eq!(n, 413);
            peer = Some(p);
            let name = std::str::from_utf8(&bytes[13..])
                .unwrap()
                .trim_end_matches('\0')
                .to_string();
            indexes.insert(
                name,
                (
                    i32::from_le_bytes(bytes[9..13].try_into().unwrap()),
                    i32::from_le_bytes(bytes[5..9].try_into().unwrap()),
                ),
            );
        }
        (indexes, peer.unwrap())
    };
    let (index, peer) = receive();
    let base = index["sim/version/xplane_internal_version"].0;
    let now = Instant::now();
    // Deliberately delay the first datagram to exercise asynchronous delivery.
    let sender = server.try_clone().unwrap();
    let baseline = packet(base, &frame(11, 1., "Aircraft/A.acf"));
    let delivery = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(10));
        sender.send_to(&baseline, peer).unwrap();
    });
    let (baseline, _) = poll_after_delivery(&mut source, now);
    delivery.join().unwrap();
    assert!(!baseline.fresh);
    assert_eq!(baseline.aircraft.as_deref(), Some("Aircraft/A.acf"));
    server
        .send_to(&packet(base, &frame(11, 2., "Aircraft/A.acf")), peer)
        .unwrap();
    assert_eq!(
        poll_after_delivery(&mut source, now).0.values["speed"],
        115.5
    );
    server
        .send_to(&packet(base, &frame(11, 3., "Aircraft/A.acf")), peer)
        .unwrap();
    let later = now + Duration::from_secs(3);
    poll_after_delivery(&mut source, later);
    let (renewed, _) = receive();
    assert_eq!(renewed["sim/time/paused"].1, 10);
    server.send_to(b"RREF,bad", peer).unwrap();
    let (snapshot, status) = poll_after_delivery(&mut source, later);
    assert!(!snapshot.fresh);
    assert!(snapshot.values.is_empty());
    assert!(status["status"].as_str().unwrap().contains("Malformed"));
    let (cancelled, _) = receive();
    assert_eq!(cancelled["sim/time/paused"].1, 0);
    let (new, _) = receive();
    assert_eq!(new["sim/time/paused"].1, 10);
    let next = new["sim/version/xplane_internal_version"].0;
    server
        .send_to(&packet(base, &frame(11, 500., "Aircraft/A.acf")), peer)
        .unwrap();
    assert!(!poll_after_delivery(&mut source, later).0.fresh);
    server
        .send_to(&packet(next, &frame(11, 10., "Aircraft/B.acf")), peer)
        .unwrap();
    assert!(!poll_after_delivery(&mut source, later).0.fresh);
    server
        .send_to(&packet(next, &frame(11, 11., "Aircraft/B.acf")), peer)
        .unwrap();
    assert!(poll_after_delivery(&mut source, later).0.fresh);
    alive.store(false, Ordering::Release);
    assert!(!source.poll(later).0.fresh);
    assert!(source.poll(later).0.values.is_empty());
}
fn packet(base: i32, values: &[(usize, f64)]) -> Vec<u8> {
    let mut out = b"RREF,".to_vec();
    for (index, value) in values {
        out.extend((base + *index as i32).to_le_bytes());
        out.extend((*value as f32).to_le_bytes());
    }
    out
}
fn frame(version: u64, clock: f64, aircraft: &str) -> Vec<(usize, f64)> {
    let mut values = vec![
        (0, (version * 10000 + 550) as f64),
        (1, 0.),
        (2, 0.),
        (3, clock),
        (4, 1.),
    ];
    values.extend(
        aircraft
            .bytes()
            .chain(Some(0))
            .enumerate()
            .map(|(i, n)| (GUARDS.len() + i, n as f64)),
    );
    let offset = GUARDS.len() + PATH_BYTES;
    values.extend([
        (offset, 115.5),
        (offset + 1, 4200.),
        (offset + 6, 2400.),
        (offset + 7, 2500.),
        (offset + 10, 1.),
    ]);
    values
}
fn outputs() -> Vec<Value> {
    vec![
        json!({"id":"speed","channel":"airspeed_kts"}),
        json!({"id":"unsafe","channel":"gear_unsafe"}),
        json!({"id":"missing","channel":"low_fuel"}),
    ]
}
fn ingest(s: &mut Session, values: &[(usize, f64)], now: Instant) {
    s.ingest(
        decode(&packet(150, values), 150, subscriptions().len()).unwrap(),
        now,
    );
}
#[test]
fn rref_wire_contract_and_malformed_packets() {
    let req = request(123, 10, "sim/time/paused");
    assert_eq!(req.len(), 413);
    assert_eq!(&req[..5], b"RREF\0");
    assert_eq!(i32::from_le_bytes(req[5..9].try_into().unwrap()), 10);
    assert_eq!(i32::from_le_bytes(req[9..13].try_into().unwrap()), 123);
    assert_eq!(&req[13..29], b"sim/time/paused\0");
    assert_eq!(&request(123, 0, "a")[5..9], &0_i32.to_le_bytes());
    assert_eq!(
        decode(&packet(100, &[(1, 123.5), (3, -25.)]), 100, 4).unwrap(),
        vec![(1, 123.5), (3, -25.)]
    );
    for bytes in [
        vec![],
        b"RREF,".to_vec(),
        b"DATA,12345678".to_vec(),
        packet(100, &[(1, f64::NAN)]),
        packet(100, &[(1, f64::INFINITY)]),
        packet(100, &[(2, 1.), (2, 2.)]),
        packet(99, &[(0, 1.)]),
        packet(100, &[(4, 1.)]),
        vec![0; 4097],
    ] {
        assert!(decode(&bytes, 100, 4).is_err());
    }
    let mut truncated = packet(100, &[(1, 1.)]);
    truncated.pop();
    assert!(decode(&truncated, 100, 4).is_err());
    assert!(decode(&packet(i32::MIN, &[(0, 1.)]), 1, 4).is_err());
}
#[test]
fn both_versions_need_baselines_and_missing_channels_stay_unavailable() {
    for version in [11, 12] {
        let now = Instant::now();
        let mut s = Session::new(&defaults(version));
        ingest(&mut s, &frame(version, 1., "Aircraft/Cessna.acf"), now);
        assert!(!s.snapshot(now, &outputs()).0.fresh);
        ingest(
            &mut s,
            &frame(version, 2., "Aircraft/Cessna.acf"),
            now + Duration::from_millis(100),
        );
        let (snapshot, status, rotate) = s.snapshot(now + Duration::from_millis(100), &outputs());
        assert!(snapshot.fresh);
        assert!(!rotate);
        assert_eq!(snapshot.values["speed"], json!(115.5));
        assert_eq!(snapshot.values["unsafe"], json!(true));
        assert!(!snapshot.values.contains_key("missing"));
        assert_eq!(status["channels"]["low_fuel"]["status"], "unavailable");
        assert_eq!(status["channels"]["engine_2_rpm"]["status"], "unavailable");
        assert_eq!(status["verified"], false);
    }
}
#[test]
fn pause_replay_version_aircraft_and_clock_changes_clear_outputs() {
    for mutation in ["pause", "replay", "version", "aircraft", "clock"] {
        let now = Instant::now();
        let mut s = Session::new(&defaults(11));
        ingest(&mut s, &frame(11, 10., "Aircraft/A.acf"), now);
        s.snapshot(now, &outputs());
        ingest(&mut s, &frame(11, 11., "Aircraft/A.acf"), now);
        assert!(s.snapshot(now, &outputs()).0.fresh);
        let epoch = s.epoch;
        let mut next = frame(
            11,
            12.,
            if mutation == "aircraft" {
                "Aircraft/B.acf"
            } else {
                "Aircraft/A.acf"
            },
        );
        match mutation {
            "pause" => next[1].1 = 1.,
            "replay" => next[2].1 = 1.,
            "version" => next[0].1 = 120000.,
            "clock" => next[3].1 = 1.,
            _ => (),
        }
        ingest(&mut s, &next, now);
        let (snapshot, _, rotate) = s.snapshot(now, &outputs());
        assert!(!snapshot.fresh, "{mutation}");
        assert!(snapshot.values.is_empty());
        assert!(rotate);
        assert!(s.epoch > epoch);
        ingest(&mut s, &frame(11, 20., "Aircraft/B.acf"), now);
        assert!(!s.snapshot(now, &outputs()).0.fresh);
        ingest(&mut s, &frame(11, 21., "Aircraft/B.acf"), now);
        assert!(s.snapshot(now, &outputs()).0.fresh);
    }
}
#[test]
fn individual_channels_and_frozen_clock_expire_while_datagrams_arrive() {
    let now = Instant::now();
    let mut s = Session::new(&defaults(12));
    ingest(&mut s, &frame(12, 1., "A.acf"), now);
    s.snapshot(now, &outputs());
    ingest(&mut s, &frame(12, 2., "A.acf"), now);
    assert!(s.snapshot(now, &outputs()).0.fresh);
    let later = now + Duration::from_millis(1600);
    let values: Vec<_> = frame(12, 3., "A.acf")
        .into_iter()
        .filter(|(i, _)| *i < GUARDS.len() + PATH_BYTES)
        .collect();
    ingest(&mut s, &values, later);
    let (snapshot, status, _) = s.snapshot(later, &outputs());
    assert!(snapshot.fresh);
    assert!(snapshot.values.is_empty());
    assert_eq!(status["channels"]["airspeed_kts"]["status"], "unavailable");
    let frozen = later + Duration::from_millis(1600);
    ingest(&mut s, &frame(12, 3., "A.acf"), frozen);
    assert!(!s.snapshot(frozen, &outputs()).0.fresh);
    ingest(&mut s, &frame(12, 4., "A.acf"), frozen);
    assert!(!s.snapshot(frozen, &outputs()).0.fresh);
    ingest(&mut s, &frame(12, 5., "A.acf"), frozen);
    assert!(s.snapshot(frozen, &outputs()).0.fresh);
    assert!(
        !s.snapshot(frozen + Duration::from_secs(2), &outputs())
            .0
            .fresh
    );
}
#[test]
fn missing_guards_invalid_ranges_and_unterminated_aircraft_paths_fail_closed() {
    let now = Instant::now();
    let mut s = Session::new(&defaults(11));
    let mut values = frame(11, 1., "A.acf");
    values.retain(|(i, _)| *i != 2);
    ingest(&mut s, &values, now);
    assert!(!s.snapshot(now, &outputs()).0.fresh);
    s.reset("test");
    let mut values = frame(11, 1., "A.acf");
    values.retain(|(i, _)| *i < GUARDS.len() || *i >= GUARDS.len() + PATH_BYTES);
    values.extend((0..PATH_BYTES).map(|i| (GUARDS.len() + i, 65.)));
    ingest(&mut s, &values, now);
    assert!(!s.snapshot(now, &outputs()).0.fresh);
    s.reset("test");
    ingest(&mut s, &frame(11, 1., "A.acf"), now);
    s.snapshot(now, &outputs());
    let mut values = frame(11, 2., "A.acf");
    for (i, n) in &mut values {
        if *i == GUARDS.len() + PATH_BYTES {
            *n = -1.;
        }
        if *i == GUARDS.len() + PATH_BYTES + 10 {
            *n = 0.5;
        }
    }
    ingest(&mut s, &values, now);
    let snapshot = s.snapshot(now, &outputs()).0;
    assert!(snapshot.fresh);
    assert!(snapshot.values.is_empty());
}
#[test]
fn connection_limits_and_native_platform_fail_closed() {
    assert!(validate_connection(&defaults(11)).is_ok());
    assert!(validate_connection(&defaults(12)).is_ok());
    for (key, value) in [
        ("host", json!("192.168.0.2")),
        ("host", json!("localhost")),
        ("port", json!(0)),
        ("listenPort", json!(65536)),
        ("updateHz", json!(21)),
        ("updateHz", json!(0)),
        ("version", json!(10)),
        ("source", json!("synthetic")),
        ("freshnessMs", json!(10001)),
    ] {
        let mut v = defaults(11);
        v[key] = value;
        assert!(validate_connection(&v).is_err(), "{key}");
    }
    let mut v = defaults(11);
    v["updateHz"] = json!(1);
    assert!(validate_connection(&v).is_err());
    v["updateHz"] = json!(3);
    v["freshnessMs"] = json!(666);
    assert!(validate_connection(&v).is_err());
    v["freshnessMs"] = json!(667);
    assert!(validate_connection(&v).is_ok());
    if !cfg!(windows) {
        assert!(Source::new(&defaults(11), outputs()).is_err());
    }
}
#[test]
fn udp_reply_port_filters_wrong_sources_and_reports_conflicts() {
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    server
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut v = defaults(11);
    v["port"] = json!(server.local_addr().unwrap().port());
    let socket = Source::socket(&v).unwrap();
    socket.send(&request(123, 10, "sim/time/paused")).unwrap();
    let mut bytes = [0; 512];
    let (n, peer) = server.recv_from(&mut bytes).unwrap();
    assert_eq!(n, 413);
    assert_eq!(peer, socket.local_addr().unwrap());
    let stranger = UdpSocket::bind("127.0.0.1:0").unwrap();
    stranger.send_to(&packet(123, &[(0, 1.)]), peer).unwrap();
    assert!(socket.recv(&mut bytes).is_err());
    server.send_to(&packet(123, &[(0, 1.)]), peer).unwrap();
    socket.set_nonblocking(false).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let n = socket.recv(&mut bytes).unwrap();
    assert_eq!(decode(&bytes[..n], 123, 1).unwrap(), vec![(0, 1.)]);
    v["listenPort"] = json!(socket.local_addr().unwrap().port());
    assert!(Source::socket(&v).is_err());
}
