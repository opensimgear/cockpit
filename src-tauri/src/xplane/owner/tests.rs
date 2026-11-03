use super::*;

fn fixture() -> (UdpSocket, Source) {
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    server
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let mut connection = defaults(12);
    connection["port"] = json!(server.local_addr().unwrap().port());
    connection["freshnessMs"] = json!(500);
    let catalogue = legacy_channels()
        .into_iter()
        .map(|c| (c.id.clone(), c))
        .collect();
    let source = Source::open_test(
        &connection,
        catalogue,
        vec![json!({"id":"speed","channel":"airspeed_kts"})],
    )
    .unwrap();
    (server, source)
}
fn requests(server: &UdpSocket, expected: usize) -> BTreeMap<String, i32> {
    let mut paths = BTreeMap::new();
    let until = Instant::now() + Duration::from_secs(3);
    let mut bytes = [0; 512];
    while paths.len() < expected {
        assert!(
            Instant::now() < until,
            "subscription requests stalled: {}",
            paths.len()
        );
        match server.recv_from(&mut bytes) {
            Ok((413, _)) if i32::from_le_bytes(bytes[5..9].try_into().unwrap()) > 0 => {
                let path = std::str::from_utf8(&bytes[13..])
                    .unwrap()
                    .trim_end_matches('\0')
                    .to_owned();
                paths.insert(path, i32::from_le_bytes(bytes[9..13].try_into().unwrap()));
            }
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(error) => panic!("{error}"),
        }
    }
    paths
}
fn send(
    server: &UdpSocket,
    source: &Source,
    indices: &BTreeMap<String, i32>,
    clock: f32,
    speed: f32,
) {
    let mut values = vec![
        (GUARDS[0].to_owned(), 120000.),
        (GUARDS[1].into(), 0.),
        (GUARDS[2].into(), 0.),
        (GUARDS[3].into(), clock),
        (GUARDS[4].into(), 1.),
        ("sim/flightmodel/position/indicated_airspeed".into(), speed),
    ];
    values.extend(b"Aircraft/Test.acf\0".iter().enumerate().map(|(i, value)| {
        (
            format!("sim/aircraft/view/acf_relative_path[{i}]"),
            *value as f32,
        )
    }));
    let mut bytes = b"RREF,".to_vec();
    for (path, value) in values {
        bytes.extend(indices[&path].to_le_bytes());
        bytes.extend(value.to_le_bytes());
    }
    server.send_to(&bytes, source.address_test()).unwrap();
}
fn observed(source: &mut Source, predicate: impl Fn(&NativeSnapshot) -> bool) -> NativeSnapshot {
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        let snapshot = source.native_snapshot().unwrap();
        if predicate(&snapshot) {
            return snapshot;
        }
        assert!(
            Instant::now() < until,
            "native commit did not arrive: {snapshot:?}"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn shared_clients_observe_autonomous_capture_and_selection_preserves_profile() {
    let (server, mut profile) = fixture();
    let mut inspector = profile.share_test(vec![]).unwrap();
    inspector
        .set_inspection_channels(&["airspeed_kts".into()])
        .unwrap();
    assert_eq!(profile.address_test(), inspector.address_test());
    let indices = requests(&server, GUARDS.len() + PATH_BYTES + 1);
    send(&server, &profile, &indices, 1., 0.);
    observed(&mut profile, |s| s.capture_sequence == 1);
    send(&server, &profile, &indices, 2., 115.5);
    let native = observed(&mut profile, |s| s.capture_sequence == 2 && s.fresh);
    let passive = observed(&mut inspector, |s| s.capture_sequence == 2);
    assert!(Arc::ptr_eq(&native.channels, &passive.channels));
    assert_eq!(
        native.channels["airspeed_kts"].value,
        Some(NativeValue::Number(115.5))
    );
    let epoch = native.epoch;
    let captured = native.channels["airspeed_kts"].captured_at;
    inspector.set_inspection_channels(&[]).unwrap();
    let next = profile.native_snapshot().unwrap();
    assert_eq!(next.epoch, epoch);
    assert_eq!(next.channels["airspeed_kts"].captured_at, captured);
    assert!(next.fresh_at(Instant::now()));
    assert_eq!(profile.snapshot().unwrap().0.values["speed"].value().clone(), 115.5);
    drop(inspector);
    send(&server, &profile, &indices, 3., 116.);
    observed(&mut profile, |s| s.capture_sequence == 3);
}

#[test]
fn producer_expiry_publishes_without_consumer_pumping_or_capture_renewal() {
    let (server, mut source) = fixture();
    let indices = requests(&server, GUARDS.len() + PATH_BYTES + 1);
    send(&server, &source, &indices, 1., 1.);
    observed(&mut source, |s| s.capture_sequence == 1);
    send(&server, &source, &indices, 2., 2.);
    let live = observed(&mut source, |s| s.capture_sequence == 2 && s.fresh);
    thread::sleep(Duration::from_millis(550));
    let expired = source.native_snapshot().unwrap();
    assert!(!expired.fresh);
    assert!(expired.epoch > live.epoch);
    assert_eq!(expired.captured_at, live.captured_at);
    assert_eq!(expired.capture_sequence, live.capture_sequence);
    assert!(source.snapshot().unwrap().0.values.is_empty());
}

#[test]
fn a_slow_subscriber_faults_without_blocking_a_healthy_profile() {
    let (server, mut healthy) = fixture();
    let mut slow = healthy.share_test(vec![]).unwrap();
    let indices = requests(&server, GUARDS.len() + PATH_BYTES + 1);
    for sequence in 1..=40 {
        send(
            &server,
            &healthy,
            &indices,
            sequence as f32,
            sequence as f32,
        );
        observed(&mut healthy, |s| s.capture_sequence == sequence);
    }
    assert!(slow.native_snapshot().unwrap_err().contains("continuity"));
    send(&server, &healthy, &indices, 41., 41.);
    assert!(observed(&mut healthy, |s| s.capture_sequence == 41).fresh);
}

#[test]
fn all_64_readers_fit_and_observer_churn_does_not_publish_phantom_frames() {
    let (server, mut profile) = fixture();
    requests(&server, GUARDS.len() + PATH_BYTES + 1);
    let baseline = observed(&mut profile, |s| !s.status.starts_with("Subscribing"));
    let readers: Vec<_> = (0..63)
        .map(|_| profile.share_test(vec![]).unwrap())
        .collect();
    assert!(profile.share_test(vec![]).is_err());
    let same = profile.native_snapshot().unwrap();
    assert!(Arc::ptr_eq(&same.channels, &baseline.channels));
    assert_eq!(same.capture_sequence, 0);
    drop(readers);
    // A lease-release notification never constructs a new source report when
    // every selected dataref is still required by the healthy profile.
    thread::sleep(Duration::from_millis(20));
    let same = profile.native_snapshot().unwrap();
    assert!(Arc::ptr_eq(&same.channels, &baseline.channels));
}

#[test]
fn a_retired_last_reader_cannot_retain_the_socket_through_its_cached_handle() {
    let (_server, source) = fixture();
    let address = source.address_test();
    source.retire_test();
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        match UdpSocket::bind(address) {
            Ok(reclaimed) => {
                assert_eq!(reclaimed.local_addr().unwrap(), address);
                break;
            }
            Err(_) => {
                assert!(
                    Instant::now() < until,
                    "retired reader retained UDP ownership"
                );
                thread::sleep(Duration::from_millis(2));
            }
        }
    }
    // Keep the Source itself alive until after reclaiming its endpoint.
    assert_eq!(source.address_test(), address);
}

#[test]
fn retained_faulted_readers_release_journal_capacity_for_replacements() {
    let (server, mut healthy) = fixture();
    let mut slow: Vec<_> = (0..63)
        .map(|_| healthy.share_test(vec![]).unwrap())
        .collect();
    let indices = requests(&server, GUARDS.len() + PATH_BYTES + 1);
    for sequence in 1..=40 {
        send(
            &server,
            &healthy,
            &indices,
            sequence as f32,
            sequence as f32,
        );
        observed(&mut healthy, |s| s.capture_sequence == sequence);
    }
    for reader in &mut slow {
        assert!(reader.native_snapshot().is_err());
    }
    let mut replacement = healthy.share_test(vec![]).unwrap();
    assert!(replacement.native_snapshot().unwrap().fresh);
    send(&server, &healthy, &indices, 41., 41.);
    assert!(observed(&mut healthy, |s| s.capture_sequence == 41).fresh);
    assert_eq!(slow.len(), 63); // All faulted handles remain cached throughout.
}

#[path = "watch_tests.rs"]
mod watch_tests;
