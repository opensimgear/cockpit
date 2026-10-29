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
use std::fs;
use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
        integration_key: id.into(),
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
            vehicle_id: None,
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
        let folder = crate::test_directory();
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
pub(super) fn bytes(counter: u8, on: bool) -> Vec<u8> {
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
pub(super) fn baseline_reader() -> (GameSources, std::net::SocketAddr, tempfile::TempDir) {
    let fixture = Dcs::new();
    let mut source = fixture.sources();
    // The real decoder/socket is exercised; native process proof has separate
    // tests. This fixture cannot admit simulator commands or a real game.
    source.game = None;
    let Some(Reader::Dcs {
        network, transport, ..
    }) = &mut source.reader
    else {
        panic!()
    };
    network.listen_port = 0;
    *transport = Some(crate::dcs_bios::Transport::open_network(network).unwrap());
    let address = transport.as_ref().unwrap().local_addr().unwrap();
    network.listen_port = address.port();
    (source, address, fixture.folder)
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
        vehicle_id: proof.vehicle_id.clone(),
    });
}
fn effect(port: &PortDescriptor, sequence: u64) -> Effect {
    Effect {
        producer: None,
        observations: vec![],
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
fn dcs_physics_updates_and_withdrawal_do_not_depend_on_the_bios_counter() {
    let mut fixture = Dcs::new();
    fixture.game.catalogue["channels"] = json!(crate::dcs_physics::catalogue());
    crate::game_channels::catalogue("dcs", &mut fixture.game.catalogue);
    let ports = crate::profile_resource_ports::integration(&fixture.game.catalogue).unwrap();
    let speed = "channel:flight.airspeed.true";
    source(
        &mut fixture.plan,
        "dcs",
        ports.iter().find(|port| port.id == speed).unwrap(),
    );
    let mut bios_only = fixture.plan.sources[0].clone();
    bios_only.endpoint.node_id = "bios-only".into();
    fixture.plan.sources.push(bios_only);
    let mut source = fixture.sources();
    authorize(&mut source);
    let Some(Reader::Dcs {
        physics_outputs, ..
    }) = &source.reader
    else {
        panic!()
    };
    let outputs = physics_outputs.clone();
    let mut physics = crate::dcs_physics::Snapshot {
        epoch: 1,
        session: Some("mission-1".into()),
        aircraft: Some("A-10C".into()),
        plane_id: Some(1),
        model_time: Some(10.0),
        sequence: Some(1),
        fresh: true,
        status: "live".into(),
        age_ms: Some(0),
        values: BTreeMap::from([("flight.airspeed.true".into(), json!(7))]),
        malformed_frames: 0,
        error: None,
    };
    let poll = |source: &mut GameSources, counter, physics: &crate::dcs_physics::Snapshot| {
        let mut bios = ExportSnapshot {
            epoch: 1,
            aircraft: Some("A-10C".into()),
            mission_active: true,
            fresh: true,
            age_ms: Some(0),
            counter: Some(counter),
            malformed_frames: 0,
            values: BTreeMap::from([
                (native_key("game", "channel:MASTER_CAUTION:0"), json!(1)),
                (
                    native_key("bios-only", "channel:MASTER_CAUTION:0"),
                    json!(1),
                ),
            ]),
        };
        source.physics = Some(merge_dcs_physics(&mut bios, &outputs, Some(physics)));
        source.frames(bios, 20_000)
    };
    assert!(!poll(&mut source, 1, &physics).fresh);
    let baseline = poll(&mut source, 2, &physics);
    assert_eq!(baseline.frames.len(), 2);
    let generation = source.sources["game"].generation;
    let bios_generation = source.sources["bios-only"].generation;
    assert!(source.can_command());
    assert!(poll(&mut source, 2, &physics).frames.is_empty());

    physics.sequence = Some(2);
    physics
        .values
        .insert("flight.airspeed.true".into(), json!(8));
    let update = poll(&mut source, 2, &physics);
    assert_eq!(update.frames.len(), 1);
    assert_eq!(update.frames[0].source, "game");
    assert_eq!(update.frames[0].reason, Reason::Change);
    assert_eq!(
        update.frames[0]
            .changes
            .iter()
            .find(|change| change.port_id == speed)
            .unwrap()
            .sample,
        SignalSample::scalar(PortValue::Number(8.0))
    );

    physics.fresh = false;
    physics.status = "paused".into();
    let paused = poll(&mut source, 2, &physics);
    assert_eq!(paused.frames.len(), 1);
    assert_eq!(
        paused.frames[0]
            .changes
            .iter()
            .find(|change| change.port_id == speed)
            .unwrap()
            .sample,
        SignalSample::Stale
    );
    assert_eq!(
        paused.frames[0]
            .changes
            .iter()
            .find(|change| change.port_id == "channel:MASTER_CAUTION:0")
            .unwrap()
            .sample,
        SignalSample::scalar(PortValue::Number(1.0))
    );
    assert!(
        source.can_command(),
        "Physics withdrawal must not grant or revoke BIOS command authority"
    );
    physics.fresh = true;
    physics.status = "live".into();
    physics.sequence = Some(3);
    let resumed = poll(&mut source, 2, &physics);
    assert_eq!(resumed.frames[0].reason, Reason::Baseline);
    assert_eq!(source.sources["game"].generation, generation + 1);
    assert_eq!(source.sources["bios-only"].generation, bios_generation);

    physics.epoch = 2;
    physics.session = Some("mission-2".into());
    physics.sequence = Some(1);
    let respawn = poll(&mut source, 2, &physics);
    assert_eq!(respawn.frames[0].reason, Reason::Baseline);
    assert_eq!(source.sources["game"].generation, generation + 2);
    assert_eq!(source.sources["bios-only"].generation, bios_generation);

    physics.sequence = Some(2);
    physics.values.clear();
    let missing = poll(&mut source, 2, &physics);
    assert_eq!(
        missing.frames[0]
            .changes
            .iter()
            .find(|change| change.port_id == speed)
            .unwrap()
            .sample,
        SignalSample::Unknown
    );
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
fn dcs_delivery_keeps_base_and_aircraft_resources_separate_and_excludes_other_modules() {
    let mut fixture = Dcs::new();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/dcs-bios/CommonData-altitude.json"),
        fixture.folder.path().join("CommonData.json"),
    )
    .unwrap();
    let aliases_path = fixture.folder.path().join("AircraftAliases.json");
    let mut aliases =
        crate::dcs_bios::read_json(&aliases_path, crate::dcs_bios::MAX_DOCUMENT).unwrap();
    aliases["A-10C"]
        .as_array_mut()
        .unwrap()
        .push(json!("CommonData"));
    fs::write(&aliases_path, serde_json::to_vec(&aliases).unwrap()).unwrap();
    fixture.game.catalogue = crate::dcs_profile::resource_catalogue(
        &json!({"cataloguePath":fixture.folder.path(),"aircraftModule":"A-10C"}),
    )
    .unwrap();
    let base_port = crate::profile_resource_ports::integration(&fixture.game.catalogue)
        .unwrap()
        .into_iter()
        .find(|port| port.id == "channel:common:CommonData:ALT_MSL_FT:0")
        .unwrap();
    let module_attributes = BTreeMap::from([
        ("gameId".into(), "dcs".into()),
        ("aircraftModule".into(), "A-10C".into()),
        (
            "catalogueRevision".into(),
            fixture.game.proof.catalogue_revision.clone(),
        ),
    ]);
    fixture.plan.sources[0].route.as_mut().unwrap().attributes = module_attributes.clone();
    fixture.plan.sinks[0].route.as_mut().unwrap().attributes = module_attributes;
    let mut other = fixture.plan.sources[0].clone();
    other.endpoint.node_id = "other-aircraft".into();
    other
        .route
        .as_mut()
        .unwrap()
        .attributes
        .insert("aircraftModule".into(), "F-16C_50".into());
    fixture.plan.sources.push(other);
    source(&mut fixture.plan, "dcs", &base_port);
    let base = fixture.plan.sources.last_mut().unwrap();
    base.endpoint.node_id = "base".into();
    let base_revision = format!("sha256:{}", "c".repeat(64));
    base.route.as_mut().unwrap().attributes = BTreeMap::from([
        ("gameId".into(), "dcs".into()),
        ("aircraftModule".into(), "CommonData".into()),
        ("catalogueRevision".into(), base_revision.clone()),
    ]);
    let mut reader = fixture.sources();
    reader.acquired = true;
    assert!(!reader.sources.contains_key("other-aircraft"));
    feed(&mut reader, 1, true, 0);
    assert!(feed(&mut reader, 2, true, 10_000).fresh);
    authorize(&mut reader);
    let batch = reader.delivery_batch().unwrap();
    batch.validate().unwrap();
    assert_eq!(batch.scope, "profile-game:dcs");
    assert_eq!(batch.observations.len(), 2);
    let base = batch
        .observations
        .iter()
        .find(|item| item.key == "dcs")
        .unwrap();
    assert_eq!(base.attributes["aircraftModule"], "CommonData");
    assert_eq!(base.attributes["catalogueRevision"], base_revision);
    assert_eq!(base.ports.as_ref().unwrap().ports, vec![base_port]);
    assert!(!base.route.as_ref().unwrap().writable);
    assert_eq!(base.route.as_ref().unwrap().id, "profile-game:dcs");
    let aircraft = batch
        .observations
        .iter()
        .find(|item| item.key == "dcs:A-10C")
        .unwrap();
    assert_eq!(
        aircraft.attributes["catalogueRevision"],
        fixture.game.proof.catalogue_revision
    );
    assert_eq!(aircraft.ports.as_ref().unwrap().ports.len(), 2);
    assert!(aircraft.route.as_ref().unwrap().writable);
    assert_eq!(
        aircraft.route.as_ref().unwrap().id,
        "profile-game:dcs:A-10C"
    );
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
    // UDP delivery is asynchronous even on loopback. Observe each actual
    // decoded counter before sending the next; otherwise both packets may be
    // drained together and the first-witness guard correctly withholds fresh.
    let receive_counter = |source: &mut GameSources, counter: u16, now_us| {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let snapshot = source.read(now_us, false).unwrap().unwrap();
            if snapshot.counter == Some(counter) {
                return snapshot;
            }
            assert!(
                Instant::now() < deadline,
                "UDP counter {counter} was not decoded"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    };
    sender
        .send_to(&bytes(1, true), ("127.0.0.1", port))
        .unwrap();
    let snapshot = receive_counter(&mut source, 1, 0);
    assert!(!source.frames(snapshot, 0).fresh);
    sender
        .send_to(&bytes(2, true), ("127.0.0.1", port))
        .unwrap();
    let snapshot = receive_counter(&mut source, 2, 10_000);
    let baseline = source.frames(snapshot, 10_000);
    assert!(baseline.fresh);
    assert_eq!(
        baseline.frames[0].changes[0].sample,
        SignalSample::scalar(PortValue::Number(1.0))
    );
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
fn publication_command_withdrawal_is_permanent_without_stopping_fresh_telemetry() {
    let fixture = Dcs::new();
    let mut reader = fixture.sources();
    feed(&mut reader, 1, true, 0);
    feed(&mut reader, 2, true, 10_000);
    authorize(&mut reader);
    assert!(reader.command_request(&effect(&fixture.command, 1)).is_ok());
    let identity = reader.reader_session.clone();
    let first = reader.commands.values().next().unwrap();
    let other_key = ("independent".into(), "command".into());
    reader.commands.insert(
        other_key,
        CommandPort {
            descriptor: first.descriptor.clone(),
            command: first.command.clone(),
        },
    );
    let affected = fixture.plan.sinks[0].endpoint.clone();
    reader.revoke_commands(&[affected.clone()]);
    assert!(reader
        .command_request(&effect(&fixture.command, 2))
        .is_err());
    let mut unrelated = effect(&fixture.command, 2);
    unrelated.endpoint.node_id = "independent".into();
    unrelated.endpoint.port_id = "command".into();
    assert!(reader.command_request(&unrelated).is_ok());
    let update = feed(&mut reader, 3, false, 20_000);
    assert!(update.fresh && !update.context_lost && reader.reader.is_some());
    assert_eq!(reader.reader_session, identity);
    assert!(reader.delivery_batch().is_some());
    reader.revoke_commands(&[affected]);
    assert!(reader
        .command_request(&effect(&fixture.command, 3))
        .is_err());
    unrelated.sequence = Counter(3);
    assert!(reader.command_request(&unrelated).is_ok());
    assert!(reader.fresh);
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
struct AcrProcessFixture {
    game: Value,
    process: crate::game_discovery::ProcessIdentity,
}
impl crate::game_discovery::Platform for AcrProcessFixture {
    fn integration_setup_supported(&self) -> bool {
        true
    }
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
        vec![self.game.clone()]
    }
    fn validate(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn launch(&self, _: &Value) -> Result<crate::game_discovery::ProcessIdentity, String> {
        Err("No launch expected".into())
    }
    fn find(&self, _: &Value) -> Result<Option<crate::game_discovery::ProcessIdentity>, String> {
        Ok(Some(self.process.clone()))
    }
    fn alive(&self, process: &crate::game_discovery::ProcessIdentity) -> Result<bool, String> {
        Ok(process == &self.process)
    }
}
#[test]
fn acr_acquisition_passes_only_verified_basename_to_reader_and_keeps_full_path_fenced() {
    for separator in ['\\', '/'] {
        let folder = crate::test_directory();
        let executable = folder.path().join("acr.exe");
        fs::write(
            &executable,
            b"Authored executable identity fixture, never launched",
        )
        .unwrap();
        let fingerprint = crate::acr_setup::fingerprint(&executable).unwrap();
        let paths = crate::games::Paths::in_directory(folder.path());
        fs::write(&paths.provider_setup,serde_json::to_vec(&json!({"schemaVersion":1,"games":{"assetto-corsa-rally":{
            "connection":{"source":"shared-memory","freshnessMs":1500},"configuredFingerprint":fingerprint,"verifiedFingerprint":fingerprint
        }}})).unwrap()).unwrap();
        crate::persistence::test_support::record(
            &paths.provider_setup,
            "games",
            "setup",
            &std::fs::read(&paths.provider_setup).unwrap(),
        );
        let full_path = executable
            .to_string_lossy()
            .replace(['\\', '/'], &separator.to_string());
        let platform = Arc::new(AcrProcessFixture {
            game: json!({"id":"assetto-corsa-rally","provider":"assetto-corsa-rally","installed":true,"executable":executable,"launchSupported":true,"processMonitoringSupported":true}),
            process: crate::game_discovery::ProcessIdentity {
                pid: 42,
                path: full_path.clone(),
                epoch: 10,
            },
        });
        let games = GamesRuntime::test_runtime(
            Arc::new(|request| panic!("Acquisition must not mutate game configuration: {request}")),
            paths,
            platform,
        );
        let game = games
            .prepare_profile_game_context("assetto-corsa-rally")
            .unwrap()
            .unwrap();
        assert_eq!(game.proof.process.path, full_path);
        let descriptors = crate::profile_resource_ports::integration(&game.catalogue).unwrap();
        let mut plan = empty_plan();
        source(&mut plan, "assetto-corsa-rally", &descriptors[0]);
        let mut reader = GameSources::prepare(&plan, Some(game.clone())).unwrap();
        let mut calls = vec![];
        reader
            .acquire_with_acr(&games, |pid, image| {
                calls.push((pid, image.to_owned()));
                Ok(Box::new(crate::game_telemetry::FixtureSource::new()))
            })
            .unwrap();
        assert_eq!(calls, vec![(42, "acr.exe".to_owned())]);
        assert_eq!(
            reader.game.as_ref().unwrap().proof.process.path,
            full_path,
            "Only the transport's basename argument is shortened"
        );
        reader.stop();
        let mut forged = game;
        forged.proof.process.path = format!("different-directory{separator}acr.exe");
        let mut changed = GameSources::prepare(&plan, Some(forged)).unwrap();
        assert!(changed
            .acquire_with_acr(&games, |_, _| panic!(
                "A changed full process path must fail before opening the reader"
            ))
            .is_err());
        games.shutdown();
    }
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
fn replacement_reader_requires_fresh_native_context_and_advances_source_identity() {
    let fixture = Dcs::new();
    let mut old = fixture.sources();
    feed(&mut old, 1, false, 0);
    let prior = feed(&mut old, 2, true, 10_000);
    assert!(prior.fresh);
    let disconnected = old.lose(20_000, "Game closed".into());
    assert!(old.needs_context() && !old.can_command());
    assert!(old.accepts_replacement(&fixture.game), "A lost reader may retry an unchanged strictly verified proof");
    let mut next_game = fixture.game.clone();
    next_game.proof.session = uuid::Uuid::new_v4().to_string();
    next_game.proof.process.epoch += 1;
    assert!(old.accepts_replacement(&next_game));
    let mut replacement = GameSources::prepare(&fixture.plan, Some(next_game)).unwrap();
    replacement.inherit_source_counters(&old);
    assert!(
        !feed(&mut replacement, 1, true, 30_000).fresh,
        "A single old counter cannot authorize resumed commands"
    );
    let baseline = feed(&mut replacement, 2, false, 40_000);
    assert!(baseline.fresh);
    assert_eq!(baseline.frames[0].reason, Reason::Baseline);
    assert!(baseline.frames[0].generation > prior.frames[0].generation);
    assert!(baseline.frames[0].sequence > disconnected.frames[0].sequence);
    assert!(
        !replacement.can_command(),
        "Telemetry alone cannot mint applied command authority"
    );
}

#[test]
fn unchanged_proof_recovers_only_a_lost_reader_with_a_new_advancing_baseline() {
    let fixture = Dcs::new();
    let mut old = fixture.sources();
    let original_reader = old.provider_identity().unwrap().1.to_owned();
    assert!(!old.accepts_replacement(&fixture.game), "A healthy reader cannot be duplicated");
    feed(&mut old, 1, false, 0);
    let prior = feed(&mut old, 2, true, 10_000);
    assert!(prior.fresh);
    let lost = old.lose(20_000, "Transient proof validation failure".into());
    assert!(old.accepts_replacement_from(old.reader_session(), &fixture.game));
    let mut replacement = GameSources::prepare(&fixture.plan, Some(fixture.game.clone())).unwrap();
    assert_ne!(replacement.provider_identity().unwrap().1, original_reader);
    replacement.inherit_source_counters(&old);
    assert!(!feed(&mut replacement, 3, false, 30_000).fresh);
    let recovered = feed(&mut replacement, 4, false, 40_000);
    assert!(recovered.fresh);
    assert_eq!(recovered.frames[0].reason, Reason::Baseline);
    assert!(recovered.frames[0].generation > prior.frames[0].generation);
    assert!(recovered.frames[0].sequence > lost.frames[0].sequence);
    assert!(!replacement.can_command(), "A retried proof and telemetry cannot substitute for strict native admission");
}

#[test]
fn unchanged_session_requires_exact_proof_and_late_predecessors_are_rejected() {
    let fixture = Dcs::new();
    let mut old = fixture.sources();
    old.lose(10_000, "Transient validation failure".into());
    for field in 0..5 {
        let mut changed = fixture.game.clone();
        match field {
            0 => changed.proof.process.epoch += 1,
            1 => changed.proof.provider_setup_revision.push('x'),
            2 => changed.proof.catalogue_revision.push('x'),
            3 => changed.proof.vehicle_id = Some("other".into()),
            _ => changed.aircraft_module = Some("other".into()),
        }
        assert!(!old.accepts_replacement(&changed), "A reused session cannot change immutable proof field {field}");
    }
    let mut next = fixture.game.clone();
    next.proof.session = uuid::Uuid::new_v4().to_string();
    let mut current = GameSources::prepare(&fixture.plan, Some(next.clone())).unwrap();
    assert!(!current.accepts_replacement(&next), "Healthy readers never accept duplicate workers");
    current.lose(20_000, "Later transient failure".into());
    assert!(!current.accepts_replacement_from(old.reader_session(), &fixture.game),
        "A late worker for the prior reader cannot replace the changed current proof");
    assert!(current.accepts_replacement_from(current.reader_session(), &next));
    let mut same_proof_reader = GameSources::prepare(&fixture.plan, Some(fixture.game.clone())).unwrap();
    same_proof_reader.lose(30_000, "A later loss after recovery under the same proof".into());
    assert_ne!(same_proof_reader.reader_session(), old.reader_session());
    assert!(!same_proof_reader.accepts_replacement_from(old.reader_session(), &fixture.game),
        "The preparation proof may be unchanged while the exact predecessor reader has already changed");
    assert!(same_proof_reader.accepts_replacement_from(same_proof_reader.reader_session(), &fixture.game));
}
#[test]
fn preparation_alone_cannot_fabricate_native_acquisition_or_a_delivery_baseline() {
    let folder = crate::test_directory();
    let platform = Arc::new(ProcessFixture {
        process: Mutex::new(Some(crate::game_discovery::ProcessIdentity {
            pid: 42,
            path: "/fixture/X-Plane.exe".into(),
            epoch: 10,
        })),
    });
    let games = GamesRuntime::test_runtime(
        Arc::new(
            |_| json!({"ok":true,"state":{"integration":{"running":false},"dcsBios":{"running":false}}}),
        ),
        {
            let fixture_metadata_path = folder.path().join("games.json");
            crate::games::Paths {
                installations: fixture_metadata_path.with_extension("installations-v1.json"),
                provider_setup: fixture_metadata_path.with_extension("integration-setup.json"),
                recovery: fixture_metadata_path.with_extension("recovery.json"),
            }
        },
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
    assert!(!reader.baseline(&games, || 0).fresh);
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
    assert!(!reader.poll(&games, || 1).fresh);
    assert!(reader.delivery_batch().is_none());
    assert!(reader
        .invoke(&games, &effect(&Dcs::new().command, 1))
        .is_err());
    *platform.process.lock().unwrap() = None;
    let deadline = Instant::now() + std::time::Duration::from_secs(2);
    let lost = loop {
        let poll = reader.poll(&games, || 2);
        if poll.context_lost { break poll; }
        assert!(Instant::now() < deadline, "Changed process was not invalidated by the live validator");
        std::thread::yield_now();
    };
    assert!(lost.context_lost);
    assert_eq!(lost.frames[0].reason, Reason::Disconnect);
    assert!(!lost.diagnostics.is_empty());
    assert!(reader.poll(&games, || 3).context_lost);
    assert!(reader.acquire(&games).is_err());
    let mut dormant = GameSources::prepare(&plan, None).unwrap();
    dormant.acquire(&games).unwrap();
    dormant.admit(&games).unwrap();
    assert!(dormant.poll(&games, || 4).frames.is_empty());
    assert!(!dormant.requires_baseline());
    games.shutdown();
    assert!(!folder.path().join("integration.json").exists());
}

#[test]
fn acquisition_clock_follows_native_guards_without_refreshing_silent_or_changed_sources() {
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    struct GuardClock {
        thread: std::thread::ThreadId,
        armed: AtomicBool,
        clock: AtomicU64,
        validation_cost: AtomicU64,
        process_checks: AtomicUsize,
    }
    impl GuardClock {
        fn identity() -> crate::game_discovery::ProcessIdentity {
            crate::game_discovery::ProcessIdentity {
                pid: 42,
                path: "/fixture/DCS.exe".into(),
                epoch: 10,
            }
        }
        fn checked(&self, find: bool) {
            // Ignore independent observers; only this poll's synchronous native
            // proof validation may advance its deterministic acquisition clock.
            if self.armed.load(Ordering::SeqCst) && std::thread::current().id() == self.thread {
                self.process_checks.fetch_add(1, Ordering::SeqCst);
                if find {
                    self.clock.fetch_add(
                        self.validation_cost.load(Ordering::SeqCst),
                        Ordering::SeqCst,
                    );
                }
            }
        }
    }
    impl crate::game_discovery::Platform for GuardClock {
        fn integration_setup_supported(&self) -> bool {
            true
        }
        fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
            vec![
                json!({"id":"dcs","provider":"dcs-bios","installed":true,"executable":"/fixture/DCS.exe","launchSupported":true,"processMonitoringSupported":true}),
            ]
        }
        fn validate(&self, _: &str, path: &str) -> Result<(), String> {
            if path == Self::identity().path {
                Ok(())
            } else {
                Err("Wrong executable".into())
            }
        }
        fn launch(&self, _: &Value) -> Result<crate::game_discovery::ProcessIdentity, String> {
            Err("No launch expected".into())
        }
        fn find(
            &self,
            _: &Value,
        ) -> Result<Option<crate::game_discovery::ProcessIdentity>, String> {
            self.checked(true);
            Ok(Some(Self::identity()))
        }
        fn alive(&self, identity: &crate::game_discovery::ProcessIdentity) -> Result<bool, String> {
            self.checked(false);
            Ok(identity == &Self::identity())
        }
    }
    let folder = crate::test_directory();
    let target = folder.path().join("DCS");
    fs::create_dir(&target).unwrap();
    crate::dcs_setup::fixture_module(&target);
    fs::write(
        target.join("Scripts/Export.lua"),
        b"dofile(lfs.writedir() .. [[Scripts\\DCS-BIOS\\BIOS.lua]])\n",
    )
    .unwrap();
    crate::dcs_setup::fixture_exports(&target);
    let catalogue = target.join("Scripts/DCS-BIOS/doc/json");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/dcs-bios/CommonData-altitude.json"),
        catalogue.join("CommonData.json"),
    )
    .unwrap();
    let aliases_path = catalogue.join("AircraftAliases.json");
    let mut aliases =
        crate::dcs_bios::read_json(&aliases_path, crate::dcs_bios::MAX_DOCUMENT).unwrap();
    aliases["A-10C"]
        .as_array_mut()
        .unwrap()
        .push(json!("CommonData"));
    fs::write(&aliases_path, serde_json::to_vec(&aliases).unwrap()).unwrap();
    let paths = crate::games::Paths::in_directory(folder.path());
    let mut connection = crate::game_integration_setup::dcs_connection(&catalogue);
    connection["freshnessMs"] = json!(100);
    fs::write(&paths.provider_setup, serde_json::to_vec(&json!({"schemaVersion":1,"games":{"dcs":{"userFolder":target,"connection":connection,"aircraftModule":"A-10C"}}})).unwrap()).unwrap();
    crate::persistence::test_support::record(
        &paths.provider_setup,
        "games",
        "setup",
        &std::fs::read(&paths.provider_setup).unwrap(),
    );
    let platform = Arc::new(GuardClock {
        thread: std::thread::current().id(),
        armed: AtomicBool::new(false),
        clock: AtomicU64::new(0),
        validation_cost: AtomicU64::new(2_000_000),
        process_checks: AtomicUsize::new(0),
    });
    let games = GamesRuntime::test_runtime(
        Arc::new(|request| panic!("Acquisition guards must not submit output: {request}")),
        paths,
        platform.clone(),
    );
    let game = games.prepare_profile_game_context("dcs").unwrap().unwrap();
    let descriptors = crate::profile_resource_ports::integration(&game.catalogue).unwrap();
    let mut plan = empty_plan();
    source(
        &mut plan,
        "dcs",
        descriptors
            .iter()
            .find(|p| p.id == "channel:common:CommonData:ALT_MSL_FT:0")
            .unwrap(),
    );
    let mut reader = GameSources::prepare(&plan, Some(game)).unwrap();
    // Keep the real reader/decoder while isolating test datagrams from the
    // passive observer's socket. Process proof validation remains authoritative;
    // this test transport cannot acquire admitted simulator command authority.
    let Some(Reader::Dcs {
        network, transport, ..
    }) = &mut reader.reader
    else {
        panic!()
    };
    network.listen_address = std::net::Ipv4Addr::LOCALHOST;
    network.listen_port = 0;
    network.multicast_group = None;
    network.multicast_interface = std::net::Ipv4Addr::LOCALHOST;
    *transport = Some(crate::dcs_bios::Transport::open_network(network).unwrap());
    let address = transport.as_ref().unwrap().local_addr().unwrap();
    let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
    platform.armed.store(true, Ordering::SeqCst);
    let clock = || {
        assert_eq!(
            platform.process_checks.swap(0, Ordering::SeqCst),
            2,
            "Sample the clock after this poll validates both process identity and liveness"
        );
        platform.clock.load(Ordering::SeqCst)
    };
    sender.send_to(&bytes(1, true), address).unwrap();
    let first = reader.baseline(&games, clock);
    assert!(!first.fresh);
    platform.validation_cost.store(0, Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_secs(1);
    while reader.witness.is_none() {
        assert!(
            Instant::now() < deadline,
            "The first isolated UDP counter was not received"
        );
        assert!(!reader.poll_baseline(&games, clock).fresh);
        std::thread::sleep(Duration::from_millis(1));
    }
    platform.validation_cost.store(10_000, Ordering::SeqCst);
    sender.send_to(&bytes(2, true), address).unwrap();
    let mut baseline = reader.poll_baseline(&games, clock);
    platform.validation_cost.store(0, Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_secs(1);
    while !baseline.fresh {
        assert!(
            Instant::now() < deadline,
            "The advancing isolated UDP counter was not received: {:?}",
            baseline.diagnostics
        );
        baseline = reader.poll_baseline(&games, clock);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        baseline.frames[0].captured_us.0, 2_010_000,
        "New bytes must not inherit the clock from before slow native validation"
    );
    platform.validation_cost.store(101_000, Ordering::SeqCst);
    assert!(
        !reader.poll_baseline(&games, clock).fresh,
        "Advancing the clock cannot extend a silent decoder's lease"
    );
    platform.validation_cost.store(10_000, Ordering::SeqCst);
    sender.send_to(&bytes(2, true), address).unwrap();
    assert!(
        !reader.poll_baseline(&games, clock).fresh,
        "A frozen old counter cannot become a new baseline"
    );

    let selected = catalogue.join("CommonData.json");
    let original = fs::read(&selected).unwrap();
    let original_modified = fs::metadata(&selected).unwrap().modified().unwrap();
    let mut changed = original.clone();
    changed.push(b'\n');
    fs::write(&selected, changed).unwrap();
    let lost = reader.baseline(&games, || {
        // Restoring in the callback would hide the changed file if the clock
        // were sampled before the forced catalogue guard.
        fs::write(&selected, &original).unwrap();
        fs::File::options()
            .write(true)
            .open(&selected)
            .unwrap()
            .set_modified(original_modified)
            .unwrap();
        platform.clock.load(Ordering::SeqCst)
    });
    assert!(lost.context_lost && !lost.fresh);
    assert!(lost
        .diagnostics
        .iter()
        .any(|error| error.contains("catalogue changed")));
    assert!(reader.reader.is_none());
    games.shutdown();
}

#[test]
fn each_assetto_provider_acquires_its_decoder_and_publishes_advancing_read_only_channels() {
    use crate::game_telemetry::{FixtureSource, Game, RawSample, SourceIdentity};
    for game in [Game::Corsa, Game::Competizione, Game::Evo] {
        let id = game.id();
        let prepared = prepared(
            id,
            json!({"source":"shared-memory","freshnessMs":100}),
            crate::game_telemetry::catalogue_for_game(id),
        );
        let descriptors = crate::profile_resource_ports::integration(&prepared.catalogue).unwrap();
        let mut plan = empty_plan();
        for port in descriptors {
            source(&mut plan, id, &port);
        }
        let mut acquired = GameSources::prepare(&plan, Some(prepared)).unwrap();
        acquired.acquired = true;
        let mut queue = FixtureSource::new_for_game(game);
        let mut sample = RawSample {
            source: SourceIdentity {
                id: game.source_id(42),
                epoch: 1,
            },
            physics: include_bytes!("../../tests/fixtures/acr/live.physics.bin").to_vec(),
            graphics: include_bytes!("../../tests/fixtures/acr/live.graphics.bin").to_vec(),
            statics: include_bytes!("../../tests/fixtures/acr/live.static.bin").to_vec(),
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
        for packet in 10u32..13 {
            sample.physics[..4].copy_from_slice(&packet.to_le_bytes());
            queue.push(sample.clone()).unwrap();
        }
        let Some(Reader::Acr { source, .. }) = &mut acquired.reader else {
            panic!()
        };
        *source = Some(Box::new(queue));
        for now in [0, 10_000] {
            let snapshot = acquired.read(now, false).unwrap().unwrap();
            assert!(!acquired.frames(snapshot, now).fresh);
        }
        let snapshot = acquired.read(20_000, false).unwrap().unwrap();
        let poll = acquired.frames(snapshot, 20_000);
        assert!(poll.fresh, "{id}");
        assert_eq!(
            poll.frames[0]
                .changes
                .iter()
                .find(|change| change.port_id == "channel:rpm")
                .unwrap()
                .sample,
            SignalSample::scalar(PortValue::Number(6300.0))
        );
        let delivery = acquired.delivery_batch().unwrap();
        assert_eq!(delivery.scope, format!("profile-game:{id}"));
        assert_eq!(delivery.observations[0].key, id);
        assert!(
            delivery.observations[0]
                .route
                .as_ref()
                .unwrap()
                .baseline_ready
        );
        assert!(!delivery.observations[0].route.as_ref().unwrap().writable);
        let snapshot = acquired.read(121_000, false).unwrap().unwrap();
        assert!(!acquired.frames(snapshot, 121_000).fresh);
        acquired.stop();
    }
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
        let snapshot = source.read(now, false).unwrap().unwrap();
        assert!(!source.frames(snapshot, now).fresh);
    }
    let snapshot = source.read(20_000, false).unwrap().unwrap();
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
    assert!(source.read(30_000, false).unwrap().is_none());
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
    let error = source.read(30_000, false).unwrap_err();
    assert!(error.contains("receive queue"));
    let lost = source.fail(30_000, error);
    assert!(lost.context_lost);
    assert_eq!(lost.frames[0].reason, Reason::Disconnect);
    assert!(source.delivery_batch().is_none());
}

#[test]
fn common_catalogue_or_alias_change_after_live_baseline_withdraws_without_rebinding() {
    for changed in ["CommonData.json", "AircraftAliases.json"] {
        let mut fixture = Dcs::new();
        let root = fixture.folder.path();
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tests/fixtures/dcs-bios/CommonData-altitude.json"),
            root.join("CommonData.json"),
        )
        .unwrap();
        let aliases_path = root.join("AircraftAliases.json");
        let mut aliases =
            crate::dcs_bios::read_json(&aliases_path, crate::dcs_bios::MAX_DOCUMENT).unwrap();
        aliases["A-10C"]
            .as_array_mut()
            .unwrap()
            .push(json!("CommonData"));
        fs::write(&aliases_path, serde_json::to_vec(&aliases).unwrap()).unwrap();
        fixture.game.catalogue = crate::dcs_profile::resource_catalogue(
            &json!({"cataloguePath":root,"aircraftModule":"A-10C"}),
        )
        .unwrap();
        let port = crate::profile_resource_ports::integration(&fixture.game.catalogue)
            .unwrap()
            .into_iter()
            .find(|port| port.id == "channel:common:CommonData:ALT_MSL_FT:0")
            .unwrap();
        let mut plan = empty_plan();
        source(&mut plan, "dcs", &port);
        let mut reader = GameSources::prepare(&plan, Some(fixture.game.clone())).unwrap();
        reader.acquired = true;
        for counter in [1u8, 2] {
            let mut frame = vec![0x55; 4];
            for (address, data) in [
                (0u16, b"A-10C                   ".to_vec()),
                (1076, 12000u16.to_le_bytes().to_vec()),
                (65534, vec![counter, 0]),
            ] {
                frame.extend(address.to_le_bytes());
                frame.extend((data.len() as u16).to_le_bytes());
                frame.extend(data);
            }
            let Some(Reader::Dcs { decoder, .. }) = &mut reader.reader else {
                panic!()
            };
            decoder.feed(&frame, u64::from(counter));
            let snapshot = decoder.snapshot(u64::from(counter));
            reader.frames(snapshot, u64::from(counter) * 1000);
        }
        assert!(reader.fresh);
        if changed == "CommonData.json" {
            let path = root.join(changed);
            let mut common =
                crate::dcs_bios::read_json(&path, crate::dcs_bios::MAX_DOCUMENT).unwrap();
            common["Altitude"]["ALT_MSL_FT"]["outputs"][0]["address"] = json!(1078);
            fs::write(path, serde_json::to_vec(&common).unwrap()).unwrap();
        } else {
            aliases["A-10C"] = json!(["A-10C"]);
            fs::write(&aliases_path, serde_json::to_vec(&aliases).unwrap()).unwrap();
        }
        let Some(Reader::Dcs {
            catalogue_guard, ..
        }) = &mut reader.reader
        else {
            panic!()
        };
        // Exercise the bounded live validation tick without a timing sleep.
        catalogue_guard.expire_for_test();
        let folder = crate::test_directory();
        let metadata = folder.path().join("games.json");
        let games = GamesRuntime::test_runtime(
            Arc::new(|_| json!({"ok":true})),
            crate::games::Paths {
                installations: metadata.with_extension("installations-v1.json"),
                provider_setup: metadata.with_extension("integration-setup.json"),
                recovery: metadata.with_extension("recovery.json"),
            },
            Arc::new(ProcessFixture {
                process: Mutex::new(None),
            }),
        );
        // This catalogue-only fixture supplies an independently healthy process
        // proof; native process fencing is exercised by the other reader tests.
        reader.validation.seed(Instant::now());
        reader.validation.start(|| Ok(Instant::now()));
        let deadline = Instant::now() + std::time::Duration::from_secs(2);
        let lost = loop {
            let poll = reader.poll(&games, || 3000);
            if poll.context_lost { break poll; }
            assert!(Instant::now() < deadline, "Changed catalogue was not invalidated by the live validator: {changed}");
            std::thread::yield_now();
        };
        assert!(lost.context_lost && !lost.fresh, "{changed}");
        assert!(
            lost.diagnostics
                .iter()
                .any(|message| message.contains("catalogue changed")),
            "{:?}",
            lost.diagnostics
        );
        assert_eq!(lost.frames.len(), 1);
        assert_eq!(lost.frames[0].reason, Reason::Disconnect);
        assert!(reader.delivery_batch().is_none());
        assert!(
            reader.reader.is_none(),
            "Loss cannot refresh captured addresses or reopen a reader"
        );
    }
}

#[test]
fn activation_backlog_requires_new_identity_and_counter_progress_without_replaying_old_values() {
    let fixture = Dcs::new();
    let mut source = fixture.sources();
    let reserve = UdpSocket::bind("127.0.0.1:0").unwrap();
    let listen = reserve.local_addr().unwrap().port();
    drop(reserve);
    let Some(Reader::Dcs {
        network, transport, ..
    }) = &mut source.reader
    else {
        panic!()
    };
    network.listen_port = listen;
    *transport = Some(crate::dcs_bios::Transport::open_network(network).unwrap());
    feed(&mut source, 1, true, 0);
    assert!(feed(&mut source, 2, true, 10_000).fresh);
    let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
    for counter in 3..83 {
        sender
            .send_to(&bytes(counter, true), ("127.0.0.1", listen))
            .unwrap();
    }
    assert!(source.read(30_000, true).unwrap().is_none());
    assert!(!source.fresh);
    assert!(source.witness.is_none());
    let empty = source.read(31_000, true).unwrap().unwrap();
    assert!(!empty.fresh);
    assert!(empty.aircraft.is_none());
    assert!(empty.values.is_empty());
    for (counter, now) in [(100, 40_000), (101, 50_000)] {
        sender
            .send_to(&bytes(counter, false), ("127.0.0.1", listen))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        let snapshot = loop {
            let snapshot = source.read(now, true).unwrap().unwrap();
            if snapshot.counter == Some(counter as u16) {
                break snapshot;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        let sample = source.frames(snapshot, now);
        assert_eq!(sample.fresh, counter == 101);
        if sample.fresh {
            assert_eq!(
                sample.frames[0].changes[0].sample,
                SignalSample::scalar(PortValue::Number(0.0))
            );
        }
    }
}
