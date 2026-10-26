
use super::*;
use osg_profiles::{
    resources::{EvidenceKind, IdentityEvidence},
    resources_ports::{PortContract, PortDescriptor, PortUpdate, ScalarSchema},
    Direction,
};
use std::sync::mpsc;

fn metadata(name: &str) -> Observation {
    let game = "assetto-corsa";
    Observation {
        key: game.into(),
        name: name.into(),
        resource_type: ResourceType::GameIntegration,
        evidence: vec![IdentityEvidence {
            kind: EvidenceKind::GameScope,
            value: game.into(),
        }],
        attributes: BTreeMap::from([
            ("gameId".into(), game.into()),
            ("integrationKey".into(), game.into()),
            ("processVerified".into(), "true".into()),
            ("gameContextStatus".into(), "ready".into()),
            ("providerSetupRevision".into(), "setup".into()),
            ("catalogueRevision".into(), "catalogue".into()),
        ]),
        diagnostics: vec![],
        route: Some(Route {
            id: format!("game:{game}"),
            transport: Transport::Integration,
            provider_generation: "metadata".into(),
            attributes: BTreeMap::new(),
            baseline_ready: false,
            writable: false,
            priority: 0,
        }),
        ports: Some(
            PortUpdate::complete(vec![PortDescriptor {
                id: "channel:rpm".into(),
                name: "RPM".into(),
                direction: Direction::Output,
                contract: PortContract::State {
                    schema: ScalarSchema::Integer {
                        min: 0,
                        max: 30000,
                        unit: Some("rpm".into()),
                    },
                    freshness_ms: 500,
                    fallback: None,
                },
            }])
            .unwrap(),
        ),
    }
}
fn adopt_metadata(runtime: &Runtime, revision: u64, name: &str) {
    Runtime::adopt(
        &runtime.state,
        vec![(
            "game:assetto-corsa".into(),
            Ok(batch(
                "game:assetto-corsa",
                "metadata",
                revision,
                vec![metadata(name)],
            )),
        )],
    );
}
fn delivery() -> ProviderBatch {
    let mut item = metadata("Current native reader");
    item.attributes
        .insert("gameReaderSession".into(), "reader".into());
    item.attributes
        .insert("gamePreparationSession".into(), "preparation".into());
    item.route = Some(Route {
        id: "profile-game:assetto-corsa".into(),
        transport: Transport::Integration,
        provider_generation: "reader:1".into(),
        attributes: item.attributes.clone(),
        baseline_ready: true,
        writable: false,
        priority: 100,
    });
    batch("profile-game:assetto-corsa", "reader", 1, vec![item])
}

#[test]
fn blocked_persistence_never_blocks_native_delivery_or_cached_capture_and_retries_raced_changes() {
    let directory = crate::test_directory();
    // macOS's temporary parent can be a symlink; use its physical path so
    // this persistence/concurrency fixture passes the real storage guard.
    let directory_path = directory.path().canonicalize().unwrap();
    let runtime = Arc::new(Runtime::new(
        Some(directory_path.clone()),
        Arc::new(|_| panic!("No discovery during cached native work")),
        false,
    ));
    adopt_metadata(&runtime, 1, "Installed catalogue");
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let writer = runtime.clone();
    let writer = thread::spawn(move || {
        Runtime::persist_with(&writer.state, &writer.persistence_gate, |store, saved| {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            store.save(saved)
        })
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let (finished_tx, finished_rx) = mpsc::channel();
    let native = runtime.clone();
    let publisher = thread::spawn(move || {
        let result = (|| {
            native.publish_profile_game_delivery(delivery())?;
            native.validate_profile_game_delivery("assetto-corsa", "reader")?;
            native.snapshot_for_compile(vec![])?;
            native.withdraw_profile_game_delivery("assetto-corsa", "reader")?;
            Ok::<_, String>(())
        })();
        finished_tx.send(result).unwrap();
    });
    let completed = finished_rx.recv_timeout(Duration::from_secs(2));
    release_tx.send(()).unwrap();
    writer.join().unwrap();
    publisher.join().unwrap();
    completed
        .expect("Native delivery and cached reads waited for metadata persistence")
        .unwrap();
    assert!(
        runtime.state.lock().unwrap().pending_save,
        "A mutation after the writer snapshot must remain dirty"
    );
    Runtime::persist(&runtime.state, &runtime.persistence_gate);
    let state = runtime.state.lock().unwrap();
    assert!(!state.pending_save);
    let restored = Registry::restore(Store::new(directory_path).load().unwrap().unwrap()).unwrap();
    assert_eq!(
        restored.saved_fingerprint().unwrap(),
        state.registry.saved_fingerprint().unwrap()
    );
}

#[test]
fn queued_metadata_writers_snapshot_after_the_writer_gate_and_never_restore_older_catalogues() {
    let directory = crate::test_directory();
    let directory_path = directory.path().canonicalize().unwrap();
    let runtime = Arc::new(Runtime::new(
        Some(directory_path.clone()),
        Arc::new(|_| vec![]),
        false,
    ));
    adopt_metadata(&runtime, 1, "Before repair");
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let first = runtime.clone();
    let first = thread::spawn(move || {
        Runtime::persist_with(&first.state, &first.persistence_gate, |store, saved| {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            store.save(saved)
        })
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    adopt_metadata(&runtime, 2, "After repair");
    let second = runtime.clone();
    let second = thread::spawn(move || Runtime::persist(&second.state, &second.persistence_gate));
    release_tx.send(()).unwrap();
    first.join().unwrap();
    second.join().unwrap();
    assert!(!runtime.state.lock().unwrap().pending_save);
    let restored = Registry::restore(Store::new(directory_path).load().unwrap().unwrap()).unwrap();
    assert_eq!(
        restored.list(0, 1).unwrap().resources[0].name,
        "After repair"
    );
}

#[test]
fn unchanged_catalogue_fingerprint_never_bypasses_an_invalid_provider_revision() {
    let runtime = Runtime::new(None, Arc::new(|_| vec![]), false);
    adopt_metadata(&runtime, 1, "Installed catalogue");
    runtime.publish_profile_game_delivery(delivery()).unwrap();
    adopt_metadata(&runtime, 0, "Installed catalogue");
    assert!(runtime
        .validate_profile_game_delivery("assetto-corsa", "reader")
        .is_err());
}

#[test]
fn large_catalogue_validation_is_admitted_once_and_repeated_binding_checks_only_selected_contracts()
{
    use osg_profiles::{
        compiler_models::SourcePlan, resources::PortExpectation, store::LocalAssignment, Endpoint,
    };
    let runtime = Runtime::new(
        None,
        Arc::new(|_| panic!("Binding cannot rediscover")),
        false,
    );
    let mut item = metadata("Large native SDK");
    let rpm = item.ports.as_ref().unwrap().ports[0].clone();
    let mut ports = item.ports.take().unwrap().ports;
    for index in 0..32768 {
        let mut port = rpm.clone();
        port.id = format!("channel:axis-{index}");
        ports.push(port);
    }
    item.ports = Some(PortUpdate::complete(ports).unwrap());
    Runtime::adopt(
        &runtime.state,
        vec![(
            "game:assetto-corsa".into(),
            Ok(batch(
                "game:assetto-corsa",
                "metadata",
                1,
                vec![item.clone()],
            )),
        )],
    );
    let (resource, admitted) = {
        let state = runtime.state.lock().unwrap();
        (
            state.registry.list(0, 1).unwrap().resources[0].id.clone(),
            state.game_metadata["assetto-corsa"].clone(),
        )
    };
    assert_eq!(admitted.contracts.len(), 32769);
    let (_, pins) = runtime
        .snapshot_for_preparation(vec![LocalAssignment {
            resource_id: "game".into(),
            source_id: resource.clone(),
            ports: vec![PortExpectation {
                profile_port_id: "rpm".into(),
                observed_port_id: rpm.id.clone(),
                expected_contract_hash: rpm.contract_hash().unwrap(),
            }],
        }])
        .unwrap();
    let pin = pins[0].clone();
    let prepared = ExecutionPlan {
        requirements: Default::default(),
        program: osg_mapping::program::Program {
            sources: vec![],
            nodes: vec![],
            sinks: vec![],
            controllers: vec![],
        },
        topological_order: vec![],
        trace: vec![],
        sources: (0..8)
            .map(|index| SourcePlan {
                endpoint: Endpoint {
                    node_id: format!("source-{index}"),
                    port_id: "rpm".into(),
                },
                inventory_resource_id: Some(resource.clone()),
                observed_port_id: Some(rpm.id.clone()),
                contract: rpm.contract.clone(),
                token: None,
                route: Some(pin.resolved().route.clone()),
                preparation: Some(pin.clone()),
                readiness: PlanReadiness::Ready,
            })
            .collect(),
        sinks: vec![],
        branches: vec![],
        controllers: vec![],
        visibility: vec![],
        runtime_overrides: vec![],
        companion_assignments: vec![],
        budgets: Default::default(),
    };
    runtime.publish_profile_game_delivery(delivery()).unwrap();
    let started = std::time::Instant::now();
    for _ in 0..64 {
        let bound = runtime.bind_delivery_reads(&prepared).unwrap();
        assert!(bound.sources.iter().all(|source| source
            .token
            .as_ref()
            .is_some_and(|token| token.provider_session == "reader")));
    }
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "Repeated selected-port binding rescanned the complete SDK catalogue: {:?}",
        started.elapsed()
    );
    assert!(Arc::ptr_eq(
        &admitted,
        &runtime.state.lock().unwrap().game_metadata["assetto-corsa"]
    ));
    // A changed exact fingerprint must replace the proof even if a provider
    // incorrectly reuses its old human-facing setup/catalogue revision.
    if let PortContract::State {
        schema: ScalarSchema::Integer { max, .. },
        ..
    } = &mut item.ports.as_mut().unwrap().ports[0].contract
    {
        *max = 20000;
    }
    item.ports = Some(PortUpdate::complete(item.ports.take().unwrap().ports).unwrap());
    Runtime::adopt(
        &runtime.state,
        vec![(
            "game:assetto-corsa".into(),
            Ok(batch("game:assetto-corsa", "metadata", 2, vec![item])),
        )],
    );
    assert!(!Arc::ptr_eq(
        &admitted,
        &runtime.state.lock().unwrap().game_metadata["assetto-corsa"]
    ));
    assert!(runtime.bind_delivery_reads(&prepared).is_err());
    assert!(runtime
        .validate_profile_game_delivery("assetto-corsa", "reader")
        .is_err());
}
