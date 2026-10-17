use super::*;
use crate::game_context::{GamePreparation, ProfileOwner};
use osg_mapping::{
    engine::RunToken,
    program::{ActionPack, NamedArgument},
};
use osg_profiles::{
    compiler_models::{BackendCapabilities, CompileSnapshot, PlanReadiness, SinkPlan, SourcePlan},
    Endpoint,
};
use std::net::UdpSocket;
use std::sync::{Arc, Mutex};

fn empty_plan() -> ExecutionPlan {
    osg_profiles::compiler::compile(
        &osg_profiles::default_document(),
        &CompileSnapshot {
            inventory_session: "fixture".into(),
            inventory_revision: "1".into(),
            assignments: vec![],
            resources: vec![],
            game_context: None,
            backend: BackendCapabilities::default(),
            inventory_diagnostics: vec![],
            read_only: false,
        },
    )
    .plan
    .unwrap()
}
fn prepared(id: &str, connection: Value, catalogue: Value) -> PreparedGame {
    PreparedGame {
        proof: GamePreparation {
            session: uuid::Uuid::new_v4().to_string(),
            game_id: id.into(),
            process: crate::game_discovery::ProcessIdentity {
                pid: 42,
                path: "/fixture/game.exe".into(),
                epoch: 10,
            },
            provider_setup_revision: format!("sha256:{}", "a".repeat(64)),
            catalogue_revision: format!("sha256:{}", "b".repeat(64)),
        },
        connection,
        catalogue,
        aircraft_module: (id == "dcs").then(|| "A-10C".into()),
    }
}
fn route(id: &str) -> Option<Route> {
    Some(Route {
        id: format!("game:{id}"),
        transport: Transport::Integration,
        provider_generation: "fixture".into(),
        attributes: BTreeMap::new(),
        baseline_ready: false,
        writable: false,
        priority: 0,
    })
}
fn source(plan: &mut ExecutionPlan, id: &str, port: &PortDescriptor) {
    plan.sources.push(SourcePlan {
        endpoint: Endpoint {
            node_id: "game".into(),
            port_id: port.id.clone(),
        },
        inventory_resource_id: Some("opaque-game".into()),
        observed_port_id: Some(port.id.clone()),
        contract: port.contract.clone(),
        token: None,
        preparation: None,
        route: route(id),
        readiness: PlanReadiness::Ready,
    });
}
struct Dcs {
    folder: tempfile::TempDir,
    game: PreparedGame,
    plan: ExecutionPlan,
    command: PortDescriptor,
}
impl Dcs {
    fn new() -> Self {
        let folder = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dcs-bios");
        for name in [
            "A-10C.json",
            "MetadataStart.json",
            "MetadataEnd.json",
            "AircraftAliases.json",
        ] {
            std::fs::copy(root.join(name), folder.path().join(name)).unwrap();
        }
        let catalogue = crate::dcs_profile::resource_catalogue(
            &json!({"cataloguePath":folder.path(),"aircraftModule":"A-10C"}),
        )
        .unwrap();
        let descriptors = crate::profile_resource_ports::integration(&catalogue).unwrap();
        let mut plan = empty_plan();
        source(
            &mut plan,
            "dcs",
            descriptors
                .iter()
                .find(|port| port.id == "channel:MASTER_CAUTION:0")
                .unwrap(),
        );
        let command = descriptors
            .iter()
            .find(|port| port.id == "command:UFC_MASTER_CAUTION:set_state")
            .unwrap()
            .clone();
        plan.sinks.push(SinkPlan {
            endpoint: Endpoint {
                node_id: "game".into(),
                port_id: command.id.clone(),
            },
            adapter: SinkAdapter::SimulatorCommand,
            inventory_resource_id: Some("opaque-game".into()),
            observed_port_id: Some(command.id.clone()),
            contract: command.contract.clone(),
            token: None,
            preparation: None,
            route: route("dcs"),
            readiness: PlanReadiness::Ready,
        });
        let game = prepared(
            "dcs",
            json!({"cataloguePath":folder.path(),"commandHost":"127.0.0.1","commandPort":7778,"listenAddress":"127.0.0.1","listenPort":5010,"multicastGroup":"","multicastInterface":"127.0.0.1","freshnessMs":100}),
            catalogue,
        );
        Self {
            folder,
            game,
            plan,
            command,
        }
    }
    fn sources(&self) -> GameSources {
        let mut source = GameSources::prepare(&self.plan, Some(self.game.clone())).unwrap();
        // These tests inject the real decoder/UDP acquisition below while the
        // separate native-process test verifies production acquisition gates.
        source.acquired = true;
        source
    }
}
fn bytes(counter: u8, on: bool) -> Vec<u8> {
    fn block(bytes: &mut Vec<u8>, address: u16, value: &[u8]) {
        bytes.extend(address.to_le_bytes());
        bytes.extend((value.len() as u16).to_le_bytes());
        bytes.extend(value);
    }
    let mut bytes = vec![0x55; 4];
    let mut name = [b' '; 24];
    name[..5].copy_from_slice(b"A-10C");
    block(&mut bytes, 0, &name);
    block(&mut bytes, 4660, &[if on { 64 } else { 0 }, 0]);
    block(&mut bytes, 65534, &[counter, 0]);
    bytes
}
fn feed(source: &mut GameSources, counter: u8, on: bool, now_us: u64) -> GamePoll {
    let Some(Reader::Dcs { decoder, .. }) = source.reader.as_mut() else {
        panic!()
    };
    decoder.feed(&bytes(counter, on), now_us / 1000);
    let snapshot = decoder.snapshot(now_us / 1000);
    source.frames(snapshot, now_us)
}
fn authorize(source: &mut GameSources) {
    let proof = &source.game.as_ref().unwrap().proof;
    source.context = Some(ContextToken {
        session: uuid::Uuid::new_v4().to_string(),
        game_id: proof.game_id.clone(),
        owner: ProfileOwner {
            session: "2b78de32-f0bc-40a3-9fd6-22ec3e0bd1cb".into(),
            profile_id: osg_profiles::DEFAULT_PROFILE_ID.into(),
            applied_revision: 1,
            run_generation: 3,
            game_id: Some("dcs".into()),
        },
        process: proof.process.clone(),
        provider_setup_revision: proof.provider_setup_revision.clone(),
        catalogue_revision: proof.catalogue_revision.clone(),
    });
}
fn effect(port: &PortDescriptor, sequence: u64) -> Effect {
    Effect {
        run: RunToken {
            session: "2b78de32-f0bc-40a3-9fd6-22ec3e0bd1cb".into(),
            generation: Counter(3),
        },
        sequence: Counter(sequence),
        at_us: Counter(20_000),
        sink_id: "edge".into(),
        endpoint: osg_mapping::program::Endpoint {
            node_id: "game".into(),
            port_id: port.id.clone(),
        },
        sample: SignalSample::Known(SignalValue::ActionPack(ActionPack {
            arguments: vec![NamedArgument {
                id: "value".into(),
                value: PortValue::Number(1.0),
            }],
        })),
        reason: ReportReason::Change,
    }
}

#[test]
fn dcs_uses_actual_catalogue_decoder_and_never_promotes_first_or_frozen_counter() {
    let fixture = Dcs::new();
    let mut source = fixture.sources();
    assert!(source.delivery_batch().is_none());
    assert!(!feed(&mut source, 1, true, 0).fresh);
    assert!(!feed(&mut source, 1, true, 10_000).fresh);
    assert!(source.delivery_batch().is_none());
    let baseline = feed(&mut source, 2, true, 20_000);
    assert!(baseline.fresh);
    assert_eq!(baseline.frames[0].reason, Reason::Baseline);
    assert_eq!(
        baseline.frames[0].changes[0].sample,
        SignalSample::scalar(PortValue::Number(1.0))
    );
    let batch = source.delivery_batch().unwrap();
    let route = batch.observations[0].route.as_ref().unwrap();
    assert!(route.baseline_ready);
    assert!(!route.writable);
    assert_eq!(batch.observations[0].ports.as_ref().unwrap().ports.len(), 2);
    authorize(&mut source);
    assert!(
        source.delivery_batch().unwrap().observations[0]
            .route
            .as_ref()
            .unwrap()
            .writable
    );
    let Some(Reader::Dcs { decoder, .. }) = source.reader.as_mut() else {
        panic!()
    };
    decoder.tick(121);
    let snapshot = decoder.snapshot(121);
    let loss = source.frames(snapshot, 121_000);
    assert!(!loss.fresh);
    assert_eq!(loss.frames[0].changes[0].sample, SignalSample::Stale);
    assert!(
        !source.delivery_batch().unwrap().observations[0]
            .route
            .as_ref()
            .unwrap()
            .writable
    );
    assert!(!fixture.folder.path().join("integration.json").exists());
}
#[test]
fn actual_udp_reader_preserves_raw_value_and_context_loss_fences_only_game_frames() {
    let fixture = Dcs::new();
    let mut source = fixture.sources();
    let reserve = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = reserve.local_addr().unwrap().port();
    drop(reserve);
    let Some(Reader::Dcs {
        network, transport, ..
    }) = source.reader.as_mut()
    else {
        panic!()
    };
    network.listen_port = port;
    *transport = Some(crate::dcs_bios::Transport::open_network(network).unwrap());
    let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
    sender
        .send_to(&bytes(1, true), ("127.0.0.1", port))
        .unwrap();
    let snapshot = source.read(0).unwrap().unwrap();
    assert!(!source.frames(snapshot, 0).fresh);
    sender
        .send_to(&bytes(2, true), ("127.0.0.1", port))
        .unwrap();
    let snapshot = source.read(10_000).unwrap().unwrap();
    assert!(source.frames(snapshot, 10_000).fresh);
    let lost = source.lose(20_000, "Verified process exited".into());
    assert!(lost.context_lost);
    assert_eq!(lost.frames.len(), 1);
    assert_eq!(lost.frames[0].source, "game");
    assert_eq!(lost.frames[0].reason, Reason::Disconnect);
    assert!(source.delivery_batch().is_none());
    assert!(source.provider_identity().is_some());
}
#[test]
fn commands_are_exact_ordered_owner_fenced_and_ambiguous_sends_cannot_be_replayed() {
    let fixture = Dcs::new();
    let mut source = fixture.sources();
    feed(&mut source, 1, true, 0);
    feed(&mut source, 2, true, 10_000);
    assert!(source
        .command_request(&effect(&fixture.command, 1))
        .is_err());
    authorize(&mut source);
    let mut forged = effect(&fixture.command, 1);
    forged.run.generation = Counter(2);
    assert!(source.command_request(&forged).is_err());
    let mut wrong = effect(&fixture.command, 1);
    let SignalSample::Known(SignalValue::ActionPack(pack)) = &mut wrong.sample else {
        panic!()
    };
    pack.arguments[0].id = "other".into();
    assert!(source.command_request(&wrong).is_err());
    let (command, argument) = source
        .command_request(&effect(&fixture.command, 1))
        .unwrap();
    assert_eq!(
        (command.as_str(), argument.as_str()),
        ("UFC_MASTER_CAUTION", "1")
    );
    assert!(source
        .command_request(&effect(&fixture.command, 1))
        .is_err());
    let mut stale = effect(&fixture.command, 2);
    stale.at_us = Counter(120_000);
    assert!(source.command_request(&stale).is_err());
}
#[test]
fn missing_wrong_game_and_unadvertised_contracts_never_open_sources() {
    let fixture = Dcs::new();
    let mut dormant = GameSources::prepare(&fixture.plan, None).unwrap();
    assert!(dormant.delivery_batch().is_none());
    assert!(dormant.reader.is_none());
    let wrong = prepared(
        "x-plane-12",
        crate::xplane::defaults(12),
        crate::xplane::catalogue(),
    );
    let wrong = GameSources::prepare(&fixture.plan, Some(wrong)).unwrap();
    assert!(wrong.reader.is_none());
    let mut invalid = fixture.plan.clone();
    invalid.sources[0].contract = PortContract::Event {
        schema: osg_mapping::signal::ScalarSchema::Boolean {},
    };
    assert!(GameSources::prepare(&invalid, Some(fixture.game.clone())).is_err());
    invalid = fixture.plan.clone();
    invalid.sources[0].observed_port_id = None;
    assert!(GameSources::prepare(&invalid, Some(fixture.game.clone())).is_err());
    dormant.stop();
    assert!(dormant.lost);
}

struct ProcessFixture {
    process: Mutex<Option<crate::game_discovery::ProcessIdentity>>,
}
impl crate::game_discovery::Platform for ProcessFixture {
    fn integration_setup_supported(&self) -> bool {
        true
    }
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
        vec![
            json!({"id":"x-plane-12","name":"X-Plane 12","provider":"x-plane","installed":true,"executable":"/fixture/X-Plane.exe","launchSupported":true,"processMonitoringSupported":true}),
        ]
    }
    fn validate(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn launch(&self, _: &Value) -> Result<crate::game_discovery::ProcessIdentity, String> {
        Err("No launch expected".into())
    }
    fn find(&self, _: &Value) -> Result<Option<crate::game_discovery::ProcessIdentity>, String> {
        Ok(self.process.lock().unwrap().clone())
    }
    fn alive(&self, identity: &crate::game_discovery::ProcessIdentity) -> Result<bool, String> {
        Ok(self.process.lock().unwrap().as_ref() == Some(identity))
    }
}
#[test]
fn preparation_alone_cannot_fabricate_native_acquisition_or_a_delivery_baseline() {
    let folder = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let platform = Arc::new(ProcessFixture {
        process: Mutex::new(Some(crate::game_discovery::ProcessIdentity {
            pid: 42,
            path: "/fixture/X-Plane.exe".into(),
            epoch: 10,
        })),
    });
    let games = GamesRuntime::new(
        Arc::new(
            |_| json!({"ok":true,"state":{"integration":{"running":false},"dcsBios":{"running":false}}}),
        ),
        folder.path().join("games.json"),
        platform.clone(),
    );
    let state = games.resource_catalogue();
    let revision = state
        .iter()
        .find(|game| game["id"] == "x-plane-12")
        .unwrap()["integrationSetup"]["providerSetupRevision"]
        .clone();
    games.request(&json!({"command":"game_integration_settings_save","gameId":"x-plane-12","expectedSetupRevision":revision,"connection":crate::xplane::defaults(12)})).unwrap();
    let game = games
        .prepare_profile_game_context("x-plane-12")
        .unwrap()
        .unwrap();
    let descriptors = crate::profile_resource_ports::integration(&game.catalogue).unwrap();
    let mut plan = empty_plan();
    source(&mut plan, "x-plane-12", &descriptors[0]);
    let mut reader = GameSources::prepare(&plan, Some(game)).unwrap();
    assert!(reader.requires_baseline());
    assert!(
        reader.acquire(&games).is_err(),
        "A fixture process must not pass the native source's real image/process gate."
    );
    assert!(reader.admit(&games).is_err());
    assert!(!reader.baseline(&games, 0).fresh);
    assert!(reader.delivery_batch().is_none());
    games
        .set_profile_context_owner(Some(ProfileOwner {
            session: "2b78de32-f0bc-40a3-9fd6-22ec3e0bd1cb".into(),
            profile_id: osg_profiles::DEFAULT_PROFILE_ID.into(),
            applied_revision: 1,
            run_generation: 3,
            game_id: Some("x-plane-12".into()),
        }))
        .unwrap();
    reader.admit(&games).unwrap();
    assert!(!reader.poll(&games, 1).fresh);
    assert!(reader.delivery_batch().is_none());
    assert!(reader
        .invoke(&games, &effect(&Dcs::new().command, 1))
        .is_err());
    *platform.process.lock().unwrap() = None;
    let lost = reader.poll(&games, 2);
    assert!(lost.context_lost);
    assert_eq!(lost.frames[0].reason, Reason::Disconnect);
    assert!(!lost.diagnostics.is_empty());
    assert!(reader.poll(&games, 3).context_lost);
    assert!(reader.acquire(&games).is_err());
    let mut dormant = GameSources::prepare(&plan, None).unwrap();
    dormant.acquire(&games).unwrap();
    dormant.admit(&games).unwrap();
    assert!(dormant.poll(&games, 4).frames.is_empty());
    assert!(!dormant.requires_baseline());
    games.shutdown();
    assert!(!folder.path().join("integration.json").exists());
}
#[test]
fn rally_fixture_uses_exact_native_channels_and_preserves_unknown_optional_values() {
    let game = prepared(
        "assetto-corsa-rally",
        json!({"source":"shared-memory","freshnessMs":100}),
        crate::game_telemetry::catalogue(),
    );
    let descriptors = crate::profile_resource_ports::integration(&game.catalogue).unwrap();
    let mut plan = empty_plan();
    for port in descriptors {
        source(&mut plan, "assetto-corsa-rally", &port);
    }
    let mut source = GameSources::prepare(&plan, Some(game)).unwrap();
    source.acquired = true;
    let mut queue = crate::game_telemetry::FixtureSource::new();
    let mut sample = crate::game_telemetry::RawSample {
        source: crate::game_telemetry::SourceIdentity {
            id: "fixture".into(),
            epoch: 1,
        },
        physics: include_bytes!("../../tests/fixtures/acr/live.physics.bin").to_vec(),
        graphics: include_bytes!("../../tests/fixtures/acr/live.graphics.bin").to_vec(),
        statics: include_bytes!("../../tests/fixtures/acr/live.static.bin").to_vec(),
    };
    // Sparse developer-observed status remains acceptable; optional static
    // maximum metadata is unknown rather than inferred from current RPM.
    sample.graphics[4..8].fill(0);
    sample.statics[..60].fill(0);
    sample.statics[412..416].fill(0);
    for counter in 10u32..13 {
        sample.physics[..4].copy_from_slice(&counter.to_le_bytes());
        queue.push(sample.clone()).unwrap();
    }
    let Some(Reader::Acr { source: reader, .. }) = &mut source.reader else {
        panic!()
    };
    *reader = Some(Box::new(queue));
    for now in [0, 10_000] {
        let snapshot = source.read(now).unwrap().unwrap();
        assert!(!source.frames(snapshot, now).fresh);
    }
    let snapshot = source.read(20_000).unwrap().unwrap();
    let baseline = source.frames(snapshot, 20_000);
    assert!(baseline.fresh);
    let changes = &baseline.frames[0].changes;
    assert_eq!(
        changes
            .iter()
            .find(|change| change.port_id == "channel:rpm")
            .unwrap()
            .sample,
        SignalSample::scalar(PortValue::Number(6300.0))
    );
    assert_eq!(
        changes
            .iter()
            .find(|change| change.port_id == "channel:gear")
            .unwrap()
            .sample,
        SignalSample::scalar(PortValue::Number(4.0))
    );
    assert_eq!(
        changes
            .iter()
            .find(|change| change.port_id == "channel:max_rpm")
            .unwrap()
            .sample,
        SignalSample::Unknown
    );
    assert!(
        !source.delivery_batch().unwrap().observations[0]
            .route
            .as_ref()
            .unwrap()
            .writable
    );
    source.stop();
    assert!(source.read(30_000).unwrap().is_none());
}

#[test]
fn separate_readers_of_one_preparation_never_share_delivery_or_withdrawal_identity() {
    let fixture = Dcs::new();
    let mut outgoing = fixture.sources();
    let mut incoming = fixture.sources();
    for source in [&mut outgoing, &mut incoming] {
        feed(source, 1, true, 0);
        feed(source, 2, true, 10_000);
    }
    let old = outgoing.delivery_batch().unwrap();
    let new = incoming.delivery_batch().unwrap();
    assert_ne!(old.session, new.session);
    assert_eq!(
        old.observations[0].attributes["gamePreparationSession"],
        new.observations[0].attributes["gamePreparationSession"]
    );
    for batch in [&old, &new] {
        assert_eq!(
            batch.observations[0].attributes["gameReaderSession"],
            batch.session
        );
        assert_eq!(
            batch.observations[0].route.as_ref().unwrap().attributes["gameReaderSession"],
            batch.session
        );
    }
    let old_identity = outgoing.provider_identity().unwrap().1.to_owned();
    outgoing.stop();
    assert_eq!(outgoing.provider_identity().unwrap().1, old_identity);
    assert_ne!(incoming.provider_identity().unwrap().1, old_identity);
    assert!(incoming.delivery_batch().is_none());
    assert_eq!(incoming.provider_identity().unwrap().1, new.session);
}

#[test]
fn sample_changes_do_not_republish_catalogues_but_delivery_authority_changes_do() {
    let fixture = Dcs::new();
    let mut source = fixture.sources();
    feed(&mut source, 1, true, 0);
    feed(&mut source, 2, true, 10_000);
    let baseline = source.delivery_batch().unwrap();
    assert!(
        baseline.observations[0]
            .route
            .as_ref()
            .unwrap()
            .baseline_ready
    );
    assert!(!baseline.observations[0].route.as_ref().unwrap().writable);
    for counter in 3..=10 {
        let sample = feed(
            &mut source,
            counter,
            counter % 2 == 0,
            u64::from(counter) * 10_000,
        );
        assert!(sample.fresh);
        assert_eq!(sample.frames[0].reason, Reason::Change);
        assert_eq!(
            sample.frames[0].changes[0].sample,
            SignalSample::scalar(PortValue::Number(if counter % 2 == 0 { 1.0 } else { 0.0 }))
        );
        assert!(
            source.delivery_batch().is_none(),
            "Value/counter changes are frames, not new route/catalogue authority."
        );
    }
    assert_eq!(source.revision, baseline.revision);
    authorize(&mut source);
    let admitted = source.delivery_batch().unwrap();
    assert!(admitted.observations[0].route.as_ref().unwrap().writable);
    assert!(source.delivery_batch().is_none());
    let Some(Reader::Dcs { decoder, .. }) = source.reader.as_mut() else {
        panic!()
    };
    decoder.tick(201);
    let snapshot = decoder.snapshot(201);
    assert!(!source.frames(snapshot, 201_000).fresh);
    let stale = source.delivery_batch().unwrap();
    assert!(!stale.observations[0].route.as_ref().unwrap().baseline_ready);
    assert!(!stale.observations[0].route.as_ref().unwrap().writable);
    assert!(source.delivery_batch().is_none());
    assert!(!feed(&mut source, 11, true, 210_000).fresh);
    assert!(source.delivery_batch().is_none());
    assert!(feed(&mut source, 12, true, 220_000).fresh);
    let fresh = source.delivery_batch().unwrap();
    assert!(fresh.observations[0].route.as_ref().unwrap().writable);
    assert!(source.delivery_batch().is_none());
    let Some(Reader::Dcs { decoder, .. }) = source.reader.as_mut() else {
        panic!()
    };
    decoder.reset_stream(230);
    feed(&mut source, 13, true, 230_000);
    let replaced = source.delivery_batch().unwrap();
    assert_ne!(
        replaced.observations[0]
            .route
            .as_ref()
            .unwrap()
            .provider_generation,
        fresh.observations[0]
            .route
            .as_ref()
            .unwrap()
            .provider_generation
    );
    assert!(
        !replaced.observations[0]
            .route
            .as_ref()
            .unwrap()
            .baseline_ready
    );
    feed(&mut source, 14, true, 240_000);
    assert!(
        source.delivery_batch().unwrap().observations[0]
            .route
            .as_ref()
            .unwrap()
            .baseline_ready
    );
    assert!(source.delivery_batch().is_none());
    assert!(
        source
            .fail(250_000, "Native process exited".into())
            .context_lost
    );
    assert!(source.delivery_batch().is_none());
}

#[test]
fn receive_backlogs_are_disarmed_and_exact_commands_are_sent_once_over_native_udp() {
    let fixture = Dcs::new();
    let mut source = fixture.sources();
    let reserve = UdpSocket::bind("127.0.0.1:0").unwrap();
    let listen = reserve.local_addr().unwrap().port();
    drop(reserve);
    let commands = UdpSocket::bind("127.0.0.1:0").unwrap();
    commands
        .set_read_timeout(Some(std::time::Duration::from_millis(100)))
        .unwrap();
    let Some(Reader::Dcs {
        network, transport, ..
    }) = &mut source.reader
    else {
        panic!()
    };
    network.listen_port = listen;
    network.command_port = commands.local_addr().unwrap().port();
    *transport = Some(crate::dcs_bios::Transport::open_network(network).unwrap());
    feed(&mut source, 1, true, 0);
    feed(&mut source, 2, true, 10_000);
    authorize(&mut source);
    let request = effect(&fixture.command, 1);
    let (command, argument) = source.command_request(&request).unwrap();
    let Some(Reader::Dcs {
        transport: Some(transport),
        ..
    }) = &source.reader
    else {
        panic!()
    };
    transport.send_control(&command, &argument).unwrap();
    let mut data = [0; 128];
    let (count, _) = commands.recv_from(&mut data).unwrap();
    assert_eq!(&data[..count], b"UFC_MASTER_CAUTION 1\n");
    assert!(source.command_request(&request).is_err());
    assert!(commands.recv_from(&mut data).is_err());
    let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
    for counter in 3..83 {
        sender
            .send_to(&bytes(counter, true), ("127.0.0.1", listen))
            .unwrap();
    }
    let error = source.read(30_000).unwrap_err();
    assert!(error.contains("receive queue"));
    let lost = source.fail(30_000, error);
    assert!(lost.context_lost);
    assert_eq!(lost.frames[0].reason, Reason::Disconnect);
    assert!(source.delivery_batch().is_none());
}
