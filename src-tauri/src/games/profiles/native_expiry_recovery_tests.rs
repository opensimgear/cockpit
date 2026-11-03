// Included with the authored transport helpers; this test disables their
// authored admission path and follows real UDP evidence through the reducer.
#[test]
fn native_bios_actual_lease_expiry_and_udp_recovery_preserve_live_ownship() {
    use crate::native_capture::{CapturedValue, Contribution, InputId, Stamp, Validity};
    let fixture = Dcs::new();
    let mut source = fixture.sources();
    source.authored_fixture = false;
    let mut contract = source.sources["game"].ports[0].contract.clone();
    if let PortContract::State { freshness_ms, .. } = &mut contract {
        *freshness_ms = 5_000;
    }
    source.sources.get_mut("game").unwrap().ports.push(SourcePort {
        logical: "ownship".into(),
        native: "ownship".into(),
        contract,
        family: "dcs-ownship".into(),
        policy: NativePolicy::Sampled,
    });
    let Some(Reader::Dcs { network, transport, decoder, .. }) = &mut source.reader else {
        panic!()
    };
    network.listen_port = 0;
    *transport = Some(crate::dcs_bios::decoded::Source::open(
        network, decoder.decode_spec().unwrap(),
    ).unwrap());
    let address = transport.as_ref().unwrap().local_addr().unwrap();
    network.listen_port = address.port();
    let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
    for counter in 1..=2 {
        sender.send_to(&bytes(counter, true), address).unwrap();
        let now = source.clock.now_us();
        receive_counter(&mut source, counter.into(), now);
    }

    let captured_at = Instant::now();
    let ownship = CapturedValue::native(json!(1), vec![Arc::new(Contribution {
        input: InputId::Channel("ownship".into()),
        capture: Stamp {
            producer_id: "expiry-test-ownship".into(), generation: 1,
            sequence: 1, byte_offset: None, captured_at,
        },
        validity: Validity::Sampled {
            valid_until: captured_at + Duration::from_secs(5),
        },
    })], captured_at).unwrap();
    source.physics = Some(PhysicsMerge {
        witness: PhysicsWitness {
            producer_id: Some("expiry-test-ownship".into()),
            session: Some("mission".into()), epoch: 1, sequence: Some(1), fresh: true,
        },
        keys: BTreeSet::from(["ownship".into()]), stale: BTreeSet::new(),
    });
    let program = osg_mapping::program::Program {
        sources: source.sources["game"].ports.iter().map(|port| osg_mapping::program::SourcePort {
            endpoint: osg_mapping::program::Endpoint {
                node_id: "game".into(), port_id: port.logical.clone(),
            },
            contract: port.contract.clone(),
        }).collect(),
        nodes: vec![], sinks: vec![], controllers: vec![],
    };
    let mut phase = osg_mapping::engine::typed::Phase::new(
        program, Counter(1), uuid::Uuid::new_v4().to_string(),
    ).unwrap();
    phase.pin_native_clock(&source.clock.id().to_string()).unwrap();
    for registration in source.native_registrations() {
        phase.register_native_source(registration).unwrap();
    }
    let endpoint = |port: &str| osg_mapping::program::Endpoint {
        node_id: "game".into(), port_id: port.into(),
    };
    let bios_port = "channel:MASTER_CAUTION:0";
    let mut snapshot = source.read(source.clock.now_us(), false).unwrap().unwrap();
    assert!(snapshot.fresh);
    snapshot.values.insert("ownship".into(), ownship.clone());
    let now = source.clock.now_us();
    let baseline = source.frames(snapshot, now);
    assert!(baseline.fatal.is_none());
    let Admission::Native(first) = &baseline.frames[0] else { panic!() };
    let generation = first.generation;
    let continuity = first.changes.iter().find(|change| change.port_id == bios_port).unwrap().continuity;
    assert!(first.changes.iter().all(|change| change.kind == NativeChangeKind::Baseline));
    phase.accept_admissions(baseline.frames, Counter(now)).unwrap();
    assert!(matches!(phase.sample(&endpoint(bios_port)), Some(SignalSample::Known(_))));
    assert!(matches!(phase.sample(&endpoint("ownship")), Some(SignalSample::Known(_))));

    // Wait for the canonical worker's real deadline publication. Consumer time
    // cannot manufacture expiry or count as a replacement capture.
    let native_deadline = match &source.reader {
        Some(Reader::Dcs { transport: Some(transport), .. }) => transport.snapshot().unwrap().deadline().unwrap(),
        _ => panic!(),
    };
    let wait_until = Instant::now() + Duration::from_secs(2);
    let mut snapshot = loop {
        let snapshot = source.read(source.clock.now_us(), false).unwrap().unwrap();
        let worker_expired = match &source.reader {
            Some(Reader::Dcs { transport: Some(transport), .. }) => transport.snapshot().unwrap().status == "stale",
            _ => panic!(),
        };
        if !snapshot.fresh && worker_expired && Instant::now() >= native_deadline { break snapshot; }
        assert!(Instant::now() < wait_until, "Actual BIOS lease did not expire");
        std::thread::sleep(Duration::from_millis(1));
    };
    snapshot.values.insert("ownship".into(), ownship.clone());
    let now = source.clock.now_us();
    let withdrawn = source.frames(snapshot, now);
    assert!(!withdrawn.context_lost && withdrawn.fatal.is_none());
    let Admission::Native(loss) = &withdrawn.frames[0] else { panic!() };
    assert_eq!(loss.generation, generation);
    assert!(loss.changes.iter().all(|change| change.port_id == bios_port
        && change.kind == NativeChangeKind::Withdraw && change.sample == SignalSample::Stale));
    phase.accept_admissions(withdrawn.frames, Counter(now)).unwrap();
    assert_eq!(phase.sample(&endpoint(bios_port)), Some(&SignalSample::Stale));
    assert!(matches!(phase.sample(&endpoint("ownship")), Some(SignalSample::Known(_))));

    // Expiry clears qualified BIOS state. Two new full consecutive exports
    // restore the real lease and a strictly newer BIOS-only baseline.
    let mut recovered = false;
    for counter in 3..=4 {
        sender.send_to(&bytes(counter, true), address).unwrap();
        let now = source.clock.now_us();
        let mut snapshot = receive_counter(&mut source, counter.into(), now);
        assert_eq!(snapshot.fresh, counter == 4);
        snapshot.values.insert("ownship".into(), ownship.clone());
        let now = source.clock.now_us();
        let poll = source.frames(snapshot, now);
        assert!(!poll.context_lost && poll.fatal.is_none());
        if counter == 4 {
            let Admission::Native(frame) = &poll.frames[0] else { panic!() };
            assert_eq!(frame.generation, generation);
            assert!(frame.changes.iter().all(|change| change.port_id == bios_port
                && change.kind == NativeChangeKind::Baseline && change.continuity > continuity
                && matches!(change.sample, SignalSample::Known(_))));
            recovered = true;
        }
        phase.accept_admissions(poll.frames, Counter(now)).unwrap();
        assert!(matches!(phase.sample(&endpoint("ownship")), Some(SignalSample::Known(_))));
    }
    assert!(recovered);
    assert!(matches!(phase.sample(&endpoint(bios_port)), Some(SignalSample::Known(_))));
    assert!(source.reader.is_some() && !source.lost);
}
