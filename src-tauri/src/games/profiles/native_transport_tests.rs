// Included in the authored fixture module to reuse its exact native UDP setup.
#[test]
fn native_bios_subscriber_overflow_preserves_ownship_and_rebases_only_bios() {
    use crate::native_capture::{CapturedValue, Contribution, InputId, Stamp, Validity};
    let fixture = Dcs::new();
    let mut source = fixture.sources();
    source.authored_fixture = false;
    let contract = source.sources["game"].ports[0].contract.clone();
    source
        .sources
        .get_mut("game")
        .unwrap()
        .ports
        .push(SourcePort {
            logical: "ownship".into(),
            native: "ownship".into(),
            contract,
            family: "dcs-ownship".into(),
            policy: NativePolicy::Sampled,
        });
    let reserve = UdpSocket::bind("127.0.0.1:0").unwrap();
    let listen = reserve.local_addr().unwrap().port();
    drop(reserve);
    let Some(Reader::Dcs {
        network,
        transport,
        decoder,
        ..
    }) = &mut source.reader
    else {
        panic!()
    };
    network.listen_port = listen;
    *transport = Some(
        crate::dcs_bios::decoded::Source::open(network, decoder.decode_spec().unwrap()).unwrap(),
    );
    let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
    for counter in 1..=2 {
        sender
            .send_to(&bytes(counter, true), ("127.0.0.1", listen))
            .unwrap();
        receive_counter(&mut source, counter.into(), 0);
    }
    let captured_at = Instant::now();
    let ownship = CapturedValue::native(
        json!(1),
        vec![Arc::new(Contribution {
            input: InputId::Channel("ownship".into()),
            capture: Stamp {
                producer_id: "ownship-native".into(),
                generation: 1,
                sequence: 1,
                byte_offset: None,
                captured_at,
            },
            validity: Validity::Sampled {
                valid_until: captured_at + Duration::from_secs(2),
            },
        })],
        captured_at,
    )
    .unwrap();
    source.physics = Some(PhysicsMerge {
        witness: PhysicsWitness {
            producer_id: Some("ownship-native".into()),
            session: Some("mission".into()),
            epoch: 1,
            sequence: Some(1),
            fresh: true,
        },
        keys: BTreeSet::from(["ownship".into()]),
        stale: BTreeSet::new(),
    });
    let mut snapshot = source.read(source.clock.now_us(), false).unwrap().unwrap();
    snapshot.values.insert("ownship".into(), ownship.clone());
    let baseline = source.frames(snapshot, source.clock.now_us());
    let Admission::Native(first) = &baseline.frames[0] else {
        panic!()
    };
    assert!(first
        .changes
        .iter()
        .any(|change| change.port_id == "ownship" && change.kind == NativeChangeKind::Baseline));
    // Each committed publication is observed through a separate subscription;
    // the tested client's bounded six-frame queue deliberately overflows.
    for counter in 3..=12 {
        sender
            .send_to(&bytes(counter, true), ("127.0.0.1", listen))
            .unwrap();
        wait_committed_counter(&source, counter.into());
    }
    let mut snapshot = source.read(source.clock.now_us(), false).unwrap().unwrap();
    assert!(!snapshot.fresh);
    snapshot.values.insert("ownship".into(), ownship.clone());
    let withdrawn = source.frames(snapshot, source.clock.now_us());
    assert!(!withdrawn.context_lost && withdrawn.fatal.is_none());
    let Admission::Native(loss) = &withdrawn.frames[0] else {
        panic!()
    };
    assert!(loss
        .changes
        .iter()
        .all(|change| change.port_id != "ownship" && change.kind == NativeChangeKind::Withdraw));
    assert!(source.reader.is_some() && !source.lost);
    let mut snapshot = source.read(source.clock.now_us(), false).unwrap().unwrap();
    assert!(
        snapshot.fresh,
        "A fresh subscription receives actual current native proof"
    );
    snapshot.values.insert("ownship".into(), ownship);
    let recovered = source.frames(snapshot, source.clock.now_us());
    let Admission::Native(recovery) = &recovered.frames[0] else {
        panic!()
    };
    assert!(recovery
        .changes
        .iter()
        .all(|change| change.port_id != "ownship" && change.kind == NativeChangeKind::Baseline));
    assert_eq!(first.generation, recovery.generation);
    assert!(
        recovery.changes[0].continuity
            > first
                .changes
                .iter()
                .find(|change| change.port_id != "ownship")
                .unwrap()
                .continuity
    );
}
