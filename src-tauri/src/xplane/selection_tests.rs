fn packet_for(source: &Driver, values: &[(usize, f64)]) -> Vec<u8> {
    let mut bytes = b"RREF,".to_vec();
    for (logical, value) in values {
        let wire = source.plan.index_for(&source.paths[*logical]).unwrap();
        bytes.extend(wire.to_le_bytes());
        bytes.extend((*value as f32).to_le_bytes());
    }
    bytes
}

#[test]
fn passive_selection_preserves_profile_context_values_and_wire_indices() {
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket2::SockRef::from(&server).set_recv_buffer_size(1024 * 1024).unwrap();
    server.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut connection = defaults(12);
    connection["port"] = json!(server.local_addr().unwrap().port());
    let profile = legacy_channels()[0].clone();
    let mut extra = profile.clone();
    extra.id = "sim/flightmodel/forces/g_nrml".into();
    extra.dataref = extra.id.clone(); extra.minimum = None; extra.maximum = None;
    let catalogue = [profile.clone(), extra.clone()].into_iter().map(|c| (c.id.clone(), c)).collect();
    let mut source = Driver::connect_selected(&connection,
        vec![json!({"id":"speed","channel":profile.id})],
        vec![true; GUARDS.len()+PATH_BYTES+1], catalogue, vec![profile.clone()], Box::new(|| Ok(())), false).unwrap();
    flush_requested(&mut source);
    let mut bytes = [0; 512];
    let mut peer = None;
    for _ in 0..GUARDS.len()+PATH_BYTES+1 {
        let (_, address) = server.recv_from(&mut bytes).unwrap(); peer = Some(address);
        assert!(!bytes[13..].starts_with(extra.dataref.as_bytes()));
    }
    let now = Instant::now();
    for clock in [1., 2.] {
        let values: Vec<_> = frame(12, clock, "Aircraft/A.acf").into_iter()
            .filter(|(i,_)| *i <= GUARDS.len()+PATH_BYTES).collect();
        server.send_to(&packet_for(&source, &values), peer.unwrap()).unwrap();
        poll_after_delivery(&mut source, now);
    }
    let epoch = source.session.epoch;
    let before = source.session.values.clone();
    let profile_index = source.plan.index_for(&profile.dataref).unwrap();
    source.set_inspection_channels(&[extra.id.clone()]).unwrap();
    flush_requested(&mut source);
    server.recv_from(&mut bytes).unwrap();
    assert_eq!(i32::from_le_bytes(bytes[5..9].try_into().unwrap()), 10);
    assert!(bytes[13..].starts_with(extra.dataref.as_bytes()));
    assert_eq!(source.session.epoch, epoch);
    assert_eq!(source.plan.index_for(&profile.dataref), Some(profile_index));
    assert_eq!(source.session.values, before);
    let (snapshot, status) = source.poll(now);
    assert!(snapshot.fresh);
    assert_eq!(snapshot.values["speed"].value().clone(), 115.5);
    assert_eq!(status["channels"][&extra.id]["value"], Value::Null);
    let retired = source.plan.index_for(&extra.dataref).unwrap();
    let mut mixed = b"RREF,".to_vec();
    for (index, value) in [(retired, -1.25f32), (profile_index, 123.5)] {
        mixed.extend(index.to_le_bytes()); mixed.extend(value.to_le_bytes());
    }
    server.send_to(&mixed, peer.unwrap()).unwrap();
    let (snapshot, status) = poll_after_delivery(&mut source, now);
    assert_eq!(snapshot.values["speed"].value().clone(), 123.5);
    assert_eq!(status["channels"][&extra.id]["value"], -1.25);
    source.set_inspection_channels(&[]).unwrap();
    flush_requested(&mut source);
    server.recv_from(&mut bytes).unwrap();
    assert_eq!(i32::from_le_bytes(bytes[5..9].try_into().unwrap()), 0);
    assert_eq!(i32::from_le_bytes(bytes[9..13].try_into().unwrap()), retired);
    server.send_to(&mixed, peer.unwrap()).unwrap();
    assert_eq!(poll_after_delivery(&mut source, now).0.values["speed"].value().clone(), 123.5);
    assert_eq!(source.session.epoch, epoch);
    source.set_inspection_channels(&[extra.id.clone()]).unwrap();
    flush_requested(&mut source);
    server.recv_from(&mut bytes).unwrap();
    assert_ne!(source.plan.index_for(&extra.dataref), Some(retired));
    server.send_to(&mixed, peer.unwrap()).unwrap();
    let (_, status) = poll_after_delivery(&mut source, now);
    assert_eq!(status["channels"][&extra.id]["value"], Value::Null);
    assert!(source.set_inspection_channels(&["missing".into()]).is_err());
    assert_eq!(source.plan.index_for(&profile.dataref), Some(profile_index));
}

#[test]
fn selection_queue_overflow_is_transactional_and_pending_retired_requests_cannot_resurrect() {
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut connection = defaults(12);
    connection["port"] = json!(server.local_addr().unwrap().port());
    let mut source = Driver::connect(&connection, outputs(), vec![true; subscriptions().len()], Box::new(|| Ok(()))).unwrap();
    flush_requested(&mut source);
    // Inspection aliases an already-required profile path: no duplicate wire request.
    source.set_inspection_channels(&["airspeed_kts".into()]).unwrap();
    assert!(source.pending_subscriptions.is_empty());
    source.set_inspection_channels(&[]).unwrap();
    assert!(source.pending_subscriptions.is_empty());
    let entry = subscriptions::WireSubscription { index: 100, path: "sim/new".into() };
    source.pending_subscriptions.push_back(request(entry.index, 10, &entry.path));
    let delta = subscriptions::LeaseDelta { added: vec![], removed: vec![entry] };
    let next = source.pending_delta(&delta).unwrap();
    assert_eq!(next.len(), 1);
    assert_eq!(&next[0][5..9], &0_i32.to_le_bytes());
    source.pending_subscriptions = (0..8192).map(|_| request(101, 0, "sim/retired")).collect();
    assert!(source.pending_delta(&delta).is_err());
    assert_eq!(source.pending_subscriptions.len(), 8192);
    source.pending_subscriptions.clear();
}

#[test]
fn malformed_mixed_reports_do_not_partially_publish_and_aliases_share_values() {
    let indices = [(123, vec![0, 1])].into_iter().collect();
    assert_eq!(decode_selected(&packet(123, &[(0, -1.25), (1, 7.)]), &indices).unwrap(), vec![(0, -1.25), (1, -1.25)]);
    assert!(decode_selected(&packet(123, &[(0, 3.), (1, f64::NAN)]), &indices).is_err());
    assert!(decode_selected(&packet(123, &[(0, 3.), (0, 4.)]), &indices).is_err());
}

#[test]
fn failed_context_fence_is_explicitly_terminal_until_reactivation() {
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut connection = defaults(12);
    connection["port"] = json!(server.local_addr().unwrap().port());
    let mut source = Driver::connect(&connection, outputs(), vec![true; subscriptions().len()], Box::new(|| Ok(()))).unwrap();
    flush_requested(&mut source);
    let now = Instant::now();
    source.last_subscription_batch = now;
    source.pending_subscriptions = (0..8192).map(|_| request(101, 0, "sim/retired")).collect();
    server.send_to(b"RREF,bad", source.socket.local_addr().unwrap()).unwrap();
    assert!(!poll_after_delivery(&mut source, now).0.fresh);
    assert!(source.terminal.as_deref().unwrap().contains("bounded request queue"));
    assert!(source.wire_indices.is_empty());
    let (snapshot, status) = source.poll(now + Duration::from_secs(3));
    assert!(!snapshot.fresh);
    assert!(snapshot.values.is_empty());
    assert!(status["status"].as_str().unwrap().contains("reactivate"));
    assert!(source.set_inspection_channels(&["airspeed_kts".into()]).is_err());
    source.pending_subscriptions.clear();
}

#[test]
fn deferred_creation_sends_no_subscriptions_before_capture_worker_readiness() {
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    server.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
    let mut connection = defaults(12);
    connection["port"] = json!(server.local_addr().unwrap().port());
    let mut driver = Driver::connect_selected(&connection, vec![], vec![true; GUARDS.len()+PATH_BYTES],
        BTreeMap::new(), vec![], Box::new(|| Ok(())), true).unwrap();
    assert_eq!(driver.pending_subscriptions.len(), GUARDS.len()+PATH_BYTES);
    let mut bytes = [0; 512];
    assert!(server.recv_from(&mut bytes).is_err());
    driver.flush_subscriptions(Instant::now()).unwrap();
    assert_eq!(server.recv_from(&mut bytes).unwrap().0, 413);
}
