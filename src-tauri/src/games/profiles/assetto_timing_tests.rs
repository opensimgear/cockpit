// Included with the profile game fixtures to share authored decoder setup.
fn assetto_fixture_sample(game: crate::game_telemetry::Game) -> crate::game_telemetry::RawSample {
    use crate::game_telemetry::{Game, RawSample, SourceIdentity};
    let mut sample = RawSample {
        source: SourceIdentity {
            id: game.source_id(42),
            epoch: 1,
        },
        physics: include_bytes!("../../../../tests/fixtures/acr/live.physics.bin").to_vec(),
        graphics: include_bytes!("../../../../tests/fixtures/acr/live.graphics.bin").to_vec(),
        statics: include_bytes!("../../../../tests/fixtures/acr/live.static.bin").to_vec(),
        capture: None,
    };
    if game == Game::Evo {
        sample.physics.resize(592, 0);
        sample.physics[588..592].copy_from_slice(&7500i32.to_le_bytes());
        sample.graphics.resize(3119, 0);
        sample.graphics[3086..3094].copy_from_slice(b"evo-car\0");
        sample.statics.fill(0);
        sample.statics[0..4].copy_from_slice(b"1.0\0");
        sample.statics[15..21].copy_from_slice(b"0.9.1\0");
        sample.statics[136..142].copy_from_slice(b"imola\0");
    }
    sample
}

#[test]
fn each_assetto_physics_packet_retains_its_exact_host_capture_in_the_reducer() {
    use crate::game_telemetry::{CaptureOwner, CaptureSet, FixtureSource, Game};
    use osg_mapping::{
        engine::typed::Phase,
        program::{Endpoint as MappingEndpoint, Program, SourcePort as MappingSource},
    };

    for game in [Game::Corsa, Game::Competizione, Game::Evo, Game::Rally] {
        let id = game.id();
        let prepared = prepared(
            id,
            json!({"source":"shared-memory","freshnessMs":500}),
            crate::game_telemetry::catalogue_for_game(id),
        );
        let port = crate::profile_resource_ports::integration(&prepared.catalogue)
            .unwrap()
            .into_iter()
            .find(|port| port.id == "channel:physics.packetId")
            .expect("Every supported Assetto dialect exposes the physics packet counter");
        let mut plan = empty_plan();
        source(&mut plan, id, &port);
        plan.sources[0].endpoint.port_id = "physics.packetId".into();
        let clock = osg_controller_input::ControllerService::new(None, false).capture_clock();
        let mut acquired = GameSources::prepare(&plan, Some(prepared), clock).unwrap();
        acquired.acquired = true;

        let endpoint = MappingEndpoint {
            node_id: "game".into(),
            port_id: "physics.packetId".into(),
        };
        let mut reducer = Phase::new(
            Program {
                sources: vec![MappingSource {
                    endpoint: endpoint.clone(),
                    contract: port.contract,
                }],
                nodes: vec![],
                sinks: vec![],
                controllers: vec![],
            },
            Counter(1),
            uuid::Uuid::new_v4().to_string(),
        )
        .unwrap();
        reducer.pin_native_clock(&clock.id().to_string()).unwrap();
        for registration in acquired.native_registrations() {
            reducer.register_native_source(registration).unwrap();
        }

        let mut capture_owner = CaptureOwner::new();
        capture_owner.begin_generation().unwrap();
        let mut sample = assetto_fixture_sample(game);
        if game != Game::Evo {
            let version = if game == Game::Corsa { "1.7" } else { "1.9" };
            sample.statics[..30].fill(0);
            for (bytes, word) in sample.statics[..30]
                .chunks_exact_mut(2)
                .zip(version.encode_utf16())
            {
                bytes.copy_from_slice(&word.to_le_bytes());
            }
        }
        for packet in [10u32, 11, 12, 14, 15] {
            sample.physics[..4].copy_from_slice(&packet.to_le_bytes());
            let at = Instant::now();
            let physics = capture_owner.capture(at).unwrap();
            let graphics = capture_owner.capture(at).unwrap();
            let statics = capture_owner.capture(at).unwrap();
            sample.capture =
                Some(CaptureSet::new(&capture_owner, physics, graphics, statics).unwrap());
            let mut queue = FixtureSource::new_for_game(game);
            queue.push(sample.clone()).unwrap();
            let Some(Reader::Acr { source, .. }) = &mut acquired.reader else {
                panic!()
            };
            *source = Some(Box::new(queue));
            let now = clock.now_us();
            let snapshot = acquired.read(now, false).unwrap().unwrap();
            let poll = acquired.frames(snapshot, now);
            assert!(poll.fatal.is_none(), "{id}: {:?}", poll.fatal);
            assert!(reducer
                .accept_admissions(poll.frames, Counter(clock.now_us()))
                .unwrap()
                .is_empty());

            if packet >= 12 {
                assert!(poll.fresh, "{id}: packet {packet}");
                let observation = reducer.inspected_observation(&endpoint).unwrap();
                assert_eq!(
                    observation.sample,
                    &SignalSample::scalar(PortValue::Number(f64::from(packet))),
                    "{id}"
                );
                assert_eq!(
                    observation.captured_sequence,
                    Counter(physics.sequence),
                    "{id}"
                );
                assert_eq!(
                    observation.captured_at,
                    Some(Counter(clock.captured_us(at).unwrap())),
                    "{id}"
                );
            }
        }
    }
}
