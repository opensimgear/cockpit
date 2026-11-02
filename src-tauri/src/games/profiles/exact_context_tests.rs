#[test]
fn shared_variant_change_cannot_rearm_the_captured_aircraft_before_tracker_refresh() {
    let fixture = Dcs::new();
    let mut source = fixture.sources();
    source.game.as_mut().unwrap().proof.vehicle_id = Some("A-10C".into());
    let reserve = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = reserve.local_addr().unwrap().port();
    drop(reserve);
    let Some(Reader::Dcs { network, transport, decoder, .. }) = &mut source.reader else { panic!() };
    let aliases = crate::dcs_bios::read_json(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dcs-bios/AircraftAliases.json"),
        crate::dcs_bios::MAX_DOCUMENT).unwrap();
    assert!(aliases["A-10C_2"].as_array().unwrap().iter().any(|module| module == "A-10C"),
        "Use a valid same-catalogue variant.");
    network.listen_port = port;
    *transport = Some(crate::dcs_bios::decoded::Source::open(network, decoder.decode_spec().unwrap()).unwrap());
    let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
    for counter in 1..=2 {
        sender.send_to(&bytes(counter, true), ("127.0.0.1", port)).unwrap();
        let snapshot = receive_counter(&mut source, counter.into(), counter as u64 * 10_000);
        assert_eq!(source.frames(snapshot, counter as u64 * 10_000).fresh, counter == 2);
    }
    authorize(&mut source);
    for counter in 3..=4 {
        let mut packet = bytes(counter, true);
        packet[8..32].fill(b' ');
        packet[8..15].copy_from_slice(b"A-10C_2");
        sender.send_to(&packet, ("127.0.0.1", port)).unwrap();
        wait_committed_counter(&source, counter.into());
    }
    // The tracker/context proof deliberately still identifies the prior variant.
    assert_eq!(source.game.as_ref().unwrap().proof.vehicle_id.as_deref(), Some("A-10C"));
    let error = source.read(50_000, false).unwrap_err();
    assert!(error.contains("aircraft changed"));
    let Some(Reader::Dcs { transport: Some(transport), .. }) = &source.reader else { panic!() };
    assert!(transport.send_control_for_aircraft("UFC_MASTER_CAUTION", "1", Some("A-10C"))
        .unwrap_err().contains("aircraft changed"));
    let lost = source.fail(50_000, error);
    assert!(lost.context_lost && !lost.fresh);
    assert!(source.delivery_batch().is_none());
}
