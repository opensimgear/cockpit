use super::*;
use osg_profiles::{
    resources::{PortExpectation, ResourceType, Transport},
    resources_ports::{PortContract, PortDescriptor, PortUpdate, ScalarSchema},
    store::LocalAssignment,
    Direction,
};

fn fixture() -> (Runtime, LocalAssignment) {
    let runtime = Runtime::new(
        None,
        Arc::new(|_| panic!("A readiness capture cannot discover or acquire a provider")),
        false,
    );
    let port = PortDescriptor {
        id: "button".into(),
        name: "Button".into(),
        direction: Direction::Output,
        contract: PortContract::State {
            schema: ScalarSchema::Boolean {},
            freshness_ms: 1000,
            fallback: None,
        },
    };
    let mut state = runtime.state.lock().unwrap();
    state
        .registry
        .reconcile(ProviderBatch {
            scope: "host-controllers".into(),
            session: "health-fixture".into(),
            revision: 1,
            complete: true,
            observations: vec![Observation {
                key: "controller".into(),
                name: "Controller".into(),
                resource_type: ResourceType::HostController,
                evidence: vec![],
                attributes: BTreeMap::new(),
                diagnostics: vec![],
                ports: Some(PortUpdate::complete(vec![port.clone()]).unwrap()),
                route: Some(Route {
                    id: "hid:controller".into(),
                    transport: Transport::Hid,
                    provider_generation: "1".into(),
                    attributes: BTreeMap::new(),
                    baseline_ready: true,
                    writable: false,
                    priority: 1,
                }),
            }],
        })
        .unwrap();
    let source_id = state.registry.list(0, 1).unwrap().resources[0].id.clone();
    drop(state);
    (
        runtime,
        LocalAssignment {
            resource_id: "controller".into(),
            source_id,
            ports: vec![PortExpectation {
                profile_port_id: "pressed".into(),
                observed_port_id: port.id.clone(),
                expected_contract_hash: port.contract_hash().unwrap(),
            }],
        },
    )
}

#[test]
fn busy_inventory_defers_readiness_without_waiting_then_returns_the_exact_current_capture() {
    let (runtime, assignment) = fixture();
    let runtime = Arc::new(runtime);
    let locked = runtime.state.lock().unwrap();
    let reader = runtime.clone();
    let requested = assignment.clone();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let _ = sender.send(
            reader
                .try_snapshot_for_preparation(vec![requested])
                .map(|capture| capture.is_none()),
        );
    });
    let deferred = receiver.recv_timeout(Duration::from_secs(1));
    // Release before checking the deadline so a regression fails promptly,
    // rather than leaving a fixture thread blocked until the suite times out.
    drop(locked);
    worker.join().unwrap();
    assert!(deferred
        .expect("Readiness must return while registry adoption still owns the mutex")
        .unwrap());
    let (snapshot, pins) = runtime
        .try_snapshot_for_preparation(vec![assignment.clone()])
        .unwrap()
        .unwrap();
    let (expected, expected_pins) = runtime.snapshot_for_preparation(vec![assignment]).unwrap();
    assert_eq!(
        serde_json::to_value(snapshot).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert_eq!(pins.len(), 1);
    assert_eq!(
        pins[0].resolved().resource_id,
        expected_pins[0].resolved().resource_id
    );
    assert_eq!(pins[0].resolved().route, expected_pins[0].resolved().route);
    runtime.validate_preparation(&pins[0]).unwrap();
}

#[test]
fn try_capture_preserves_read_only_poison_recovery_and_input_limits() {
    let (runtime, assignment) = fixture();
    runtime.state.lock().unwrap().read_only = true;
    let (snapshot, pins) = runtime
        .try_snapshot_for_preparation(vec![assignment.clone()])
        .unwrap()
        .unwrap();
    assert!(snapshot.read_only);
    assert!(pins.is_empty());
    assert!(runtime
        .try_snapshot_for_preparation(vec![assignment.clone(); 65])
        .unwrap_err()
        .contains("bounded"));
    let mut excessive_ports = assignment.clone();
    excessive_ports.ports = vec![assignment.ports[0].clone(); 1025];
    assert!(runtime
        .try_snapshot_for_preparation(vec![excessive_ports])
        .unwrap_err()
        .contains("bounded"));
    let poisoned = runtime.state.clone();
    assert!(thread::spawn(move || {
        let _guard = poisoned.lock().unwrap();
        panic!("Poison the readiness fixture");
    })
    .join()
    .is_err());
    assert!(
        runtime
            .try_snapshot_for_preparation(vec![assignment.clone()])
            .unwrap()
            .unwrap()
            .0
            .read_only
    );
    runtime.stop.store(true, Ordering::Release);
    assert!(runtime
        .try_snapshot_for_preparation(vec![assignment])
        .unwrap_err()
        .contains("stopped"));
}

fn delivery_fixture() -> (Runtime, ResourceToken) {
    let (runtime, assignment) = fixture();
    let snapshot = runtime.snapshot_for_compile(vec![assignment]).unwrap();
    let token = snapshot.resources[0].tokens[0].clone();
    (runtime, token)
}

#[test]
fn live_delivery_pending_is_distinct_from_valid_and_revoked_tokens() {
    let (runtime, token) = delivery_fixture();
    let held = runtime.state.lock().unwrap();
    let started = std::time::Instant::now();
    assert_eq!(runtime.try_validate_live_delivery(&token).unwrap(), None);
    assert!(started.elapsed() < Duration::from_millis(100));
    drop(held);
    assert!(runtime.try_validate_live_delivery(&token).unwrap().is_some());
    let mut revoked = token;
    revoked.provider_generation = "revoked".into();
    assert!(runtime.try_validate_live_delivery(&revoked).is_err());
}

#[test]
fn live_delivery_validation_returns_promptly_when_inventory_is_busy_then_checks_current_route() {
    let (runtime, token) = delivery_fixture();
    let expected = runtime.validate_delivery(&token).unwrap();
    let runtime = Arc::new(runtime);
    let held = runtime.state.lock().unwrap();
    let reader = runtime.clone();
    let requested = token.clone();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let started = std::time::Instant::now();
        let result = reader.validate_live_delivery(&requested);
        let _ = sender.send((started.elapsed(), result));
    });

    let response = receiver.recv_timeout(Duration::from_millis(250));
    drop(held);
    if response.is_err() {
        // Ensure a blocking regression is joined before failing the test.
        worker.join().unwrap();
    } else {
        worker.join().unwrap();
    }
    let (elapsed, result) =
        response.expect("Busy live validation must not wait for the inventory mutex");
    assert!(
        elapsed < Duration::from_millis(100),
        "busy validation took {elapsed:?}"
    );
    assert_eq!(result.unwrap(), expected, "Contention preserves the last committed exact-token route");

    assert_eq!(runtime.validate_live_delivery(&token).unwrap(), expected);
}

#[test]
fn live_delivery_validation_rechecks_revocation_and_recovers_a_poisoned_inventory_lock() {
    let (runtime, token) = delivery_fixture();
    assert!(runtime.validate_live_delivery(&token).is_ok());

    let poisoned = runtime.state.clone();
    assert!(thread::spawn(move || {
        let _guard = poisoned.lock().unwrap();
        panic!("Poison the live-validation fixture");
    })
    .join()
    .is_err());
    assert!(runtime.validate_live_delivery(&token).is_ok());

    // A current registry revision invalidates the old provider-generation token.
    let mut state = runtime
        .state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    state
        .registry
        .reconcile(ProviderBatch {
            scope: "host-controllers".into(),
            session: "health-fixture".into(),
            revision: 2,
            complete: true,
            observations: vec![Observation {
                key: "controller".into(),
                name: "Controller".into(),
                resource_type: ResourceType::HostController,
                evidence: vec![],
                attributes: BTreeMap::new(),
                diagnostics: vec![],
                ports: Some(
                    PortUpdate::complete(vec![PortDescriptor {
                        id: "button".into(),
                        name: "Button".into(),
                        direction: Direction::Output,
                        contract: PortContract::State {
                            schema: ScalarSchema::Boolean {},
                            freshness_ms: 1000,
                            fallback: None,
                        },
                    }])
                    .unwrap(),
                ),
                route: Some(Route {
                    id: "hid:controller".into(),
                    transport: Transport::Hid,
                    provider_generation: "2".into(),
                    attributes: BTreeMap::new(),
                    baseline_ready: true,
                    writable: false,
                    priority: 1,
                }),
            }],
        })
        .unwrap();
    drop(state);
    assert!(runtime.validate_live_delivery(&token).is_err());
}
