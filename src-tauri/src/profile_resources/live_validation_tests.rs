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

fn delivery() -> (Runtime, ResourceToken) {
    let (runtime, assignment) = fixture();
    let snapshot = runtime.snapshot_for_compile(vec![assignment]).unwrap();
    (runtime, snapshot.resources[0].tokens[0].clone())
}

#[test]
fn confirmed_delivery_survives_a_held_writer_and_unchanged_commit() {
    let (runtime, token) = delivery();
    let route = runtime.validate_delivery(&token).unwrap();
    let mut writer = runtime.state.lock().unwrap();
    writer.diagnostics.push("Metadata refresh in progress".into());
    let before = std::time::Instant::now();
    assert_eq!(runtime.try_validate_live_delivery(&token).unwrap(), Some(route.clone()));
    assert!(before.elapsed() < Duration::from_millis(100));
    drop(writer);
    let _writer = runtime.state.lock().unwrap();
    assert_eq!(runtime.validate_live_delivery(&token).unwrap(), route);
}

#[test]
fn offline_commit_revokes_cached_route_before_state_unlock() {
    let (runtime, token) = delivery();
    let route = runtime.validate_delivery(&token).unwrap();
    let mut writer = runtime.state.lock().unwrap();
    writer.registry.reconcile(ProviderBatch { scope: "host-controllers".into(),
        session: "health-fixture".into(), revision: 2, complete: true, observations: vec![] }).unwrap();
    assert_eq!(runtime.validate_live_delivery(&token).unwrap(), route, "Uncommitted work cannot replace the last committed view");
    drop(writer);
    let _next_writer = runtime.state.lock().unwrap();
    assert!(runtime.try_validate_live_delivery(&token).is_err(), "Committed offline state must revoke even while another writer holds inventory");
}

#[test]
fn reconnect_wakes_after_catalogue_commit_releases_the_registry() {
    let (runtime, assignment) = fixture();
    let observations = crate::observations::Runtime::new();
    let wake = Arc::new(osg_streams::NativeWake::default());
    wake.bind_thread(std::thread::current()).unwrap();
    runtime.watch_reconnect_catalogue(&wake);
    runtime.observe_catalogue(&observations.broker()).unwrap();
    let before = wake.observed();
    let mut writer = runtime.state.lock().unwrap();
    writer.registry.reconcile(ProviderBatch { scope: "host-controllers".into(),
        session: "health-fixture".into(), revision: 2, complete: true, observations: vec![] }).unwrap();
    assert_eq!(wake.observed(), before);
    drop(writer);
    assert!(wake.wait(before, Duration::from_secs(2)));
    assert!(runtime.try_snapshot_for_preparation(vec![assignment]).unwrap().is_some());
    runtime.shutdown();
}

#[test]
fn contended_reconnect_wakes_on_read_unlock_without_a_catalogue_commit() {
    let (runtime, assignment) = fixture();
    let wake = Arc::new(osg_streams::NativeWake::default());
    runtime.watch_reconnect_catalogue(&wake);
    let before = wake.observed();
    let reader = runtime.state.lock().unwrap();
    assert!(runtime.try_snapshot_for_preparation(vec![assignment.clone()]).unwrap().is_none());
    assert_eq!(wake.observed(), before);
    drop(reader);
    let unlocked = wake.observed();
    assert_ne!(unlocked, before);
    assert!(runtime.try_snapshot_for_preparation(vec![assignment]).unwrap().is_some());
    assert_eq!(wake.observed(), unlocked);
}

#[test]
fn generation_commit_wakes_routing_with_the_exact_old_token_already_revoked() {
    let (runtime, token) = delivery();
    let route = runtime.validate_delivery(&token).unwrap();
    let routing = Arc::new(osg_streams::NativeWake::default());
    routing.bind_thread(thread::current()).unwrap();
    runtime.watch_routing_catalogue(&routing);
    // Transient preparation/learning and the automatic reconnect owner must
    // retain independent slots from the live coordinator's invalidations.
    let activation = Arc::new(osg_streams::NativeWake::default());
    let learning = Arc::new(osg_streams::NativeWake::default());
    let reconnect = Arc::new(osg_streams::NativeWake::default());
    runtime.watch_activation_catalogue(&activation);
    runtime.watch_learning_catalogue(&learning);
    runtime.watch_reconnect_catalogue(&reconnect);
    let before = routing.observed();
    let mut replacement = route.clone();
    replacement.provider_generation = "2".into();
    let mut writer = runtime.state.lock().unwrap();
    let selected = PortExpectation { profile_port_id: "pressed".into(), observed_port_id: token.port_id.clone(),
        expected_contract_hash: token.contract_hash.clone() };
    let port = writer.registry.inspect_selected(&token.resource_id, &[selected]).unwrap().ports[0].descriptor.clone();
    writer.registry.reconcile(ProviderBatch { scope: "host-controllers".into(),
        session: "health-fixture".into(), revision: 2, complete: true,
        observations: vec![Observation { key: "controller".into(), name: "Controller".into(),
            resource_type: ResourceType::HostController, evidence: vec![], attributes: BTreeMap::new(),
            diagnostics: vec![], route: Some(replacement), ports: Some(PortUpdate::complete(vec![port]).unwrap()),
        }],
    }).unwrap();
    assert_eq!(routing.observed(), before, "Uncommitted changes cannot wake routing");
    assert_eq!(runtime.try_validate_live_delivery(&token).unwrap(), Some(route));
    drop(writer);
    assert_ne!(routing.observed(), before, "Routing must observe the committed generation change");
    assert!(!routing.wait(before, Duration::from_secs(2)), "A pre-park commit must request another drain immediately");
    let _writer = runtime.state.lock().unwrap();
    assert!(runtime.try_validate_live_delivery(&token).is_err(), "The wake must expose committed revocation even with inventory locked again");
    assert!(activation.observed() > 0);
    assert!(learning.observed() > 0);
}

#[test]
fn read_only_delivery_validation_does_not_wake_routing() {
    let (runtime, token) = delivery();
    let routing = Arc::new(osg_streams::NativeWake::default());
    runtime.watch_routing_catalogue(&routing);
    let before = routing.observed();
    let route = runtime.validate_delivery(&token).unwrap();
    assert_eq!(runtime.validate_live_delivery(&token).unwrap(), route);
    {
        let _reader = runtime.state.lock().unwrap();
        assert_eq!(runtime.try_validate_live_delivery(&token).unwrap(), Some(route));
    }
    assert_eq!(routing.observed(), before, "Read-only checks and confirmed-cache reads must leave parked routing quiet");
}

#[test]
fn uncached_and_different_tokens_cannot_borrow_confirmed_authority() {
    let (runtime, token) = delivery();
    let writer = runtime.state.lock().unwrap();
    assert!(runtime.try_validate_live_delivery(&token).unwrap().is_none());
    drop(writer);
    runtime.validate_delivery(&token).unwrap();
    let _writer = runtime.state.lock().unwrap();
    let mut other = token.clone();
    other.provider_generation.push_str("-replacement");
    assert!(runtime.try_validate_live_delivery(&other).unwrap().is_none());
    runtime.stop.store(true, Ordering::Release);
    assert!(runtime.try_validate_live_delivery(&token).unwrap_err().contains("stopped"));
}

#[test]
fn poisoned_mutation_publishes_revocation_and_preserves_poison_recovery() {
    let (runtime, token) = delivery();
    runtime.validate_delivery(&token).unwrap();
    let state = runtime.state.clone();
    assert!(thread::spawn(move || {
        let mut writer = state.lock().unwrap();
        writer.registry.reconcile(ProviderBatch { scope: "host-controllers".into(),
            session: "health-fixture".into(), revision: 2, complete: true, observations: vec![] }).unwrap();
        panic!("Test failed writer after mutation");
    }).join().is_err());
    let _writer = runtime.state.lock().unwrap_or_else(|poison| poison.into_inner());
    assert!(runtime.try_validate_live_delivery(&token).is_err());
}

#[test]
fn confirmed_token_cache_is_bounded_and_eviction_returns_pending() {
    let (runtime, token) = delivery();
    runtime.validate_delivery(&token).unwrap();
    for index in 0..CAPACITY {
        let mut invalid = token.clone();
        invalid.port_id = format!("missing-{index}");
        assert!(runtime.validate_delivery(&invalid).is_err());
    }
    assert_eq!(runtime.state.confirmed.lock().unwrap().entries.len(), CAPACITY);
    let _writer = runtime.state.lock().unwrap();
    assert!(runtime.try_validate_live_delivery(&token).unwrap().is_none());
}

fn game_owner() -> Runtime {
    let runtime = Runtime::new(None, Arc::new(|_| vec![]), false);
    runtime.state.lock().unwrap().game_deliveries.insert("dcs".into(), ProviderBatch {
        scope: "profile-game:dcs".into(), session: "reader-a".into(), revision: 1,
        complete: true, observations: vec![],
    });
    runtime
}

#[test]
fn confirmed_game_reader_survives_unrelated_writer_without_claiming_unknown_readers() {
    let runtime = game_owner();
    runtime.validate_profile_game_delivery("dcs", "reader-a").unwrap();
    let mut writer = runtime.state.lock().unwrap();
    writer.pending_save = true;
    assert_eq!(runtime.try_validate_profile_game_delivery("dcs", "reader-a").unwrap(), Some(()));
    assert_eq!(runtime.try_validate_profile_game_delivery("dcs", "reader-b").unwrap(), None);
    assert_eq!(runtime.try_validate_profile_game_delivery("xplane", "reader-a").unwrap(), None);
    drop(writer);
    let _writer = runtime.state.lock().unwrap();
    assert_eq!(runtime.try_validate_profile_game_delivery("dcs", "reader-a").unwrap(), Some(()));
}

#[test]
fn game_withdrawal_and_replacement_revoke_last_confirmed_reader_at_commit() {
    for replacement in [false, true] {
        let runtime = game_owner();
        runtime.validate_profile_game_delivery("dcs", "reader-a").unwrap();
        let mut writer = runtime.state.lock().unwrap();
        if replacement {
            writer.game_deliveries.get_mut("dcs").unwrap().session = "reader-b".into();
        } else {
            writer.game_deliveries.remove("dcs");
        }
        assert_eq!(runtime.try_validate_profile_game_delivery("dcs", "reader-a").unwrap(), Some(()));
        drop(writer);
        let writer = runtime.state.lock().unwrap();
        assert!(runtime.try_validate_profile_game_delivery("dcs", "reader-a").is_err());
        assert_eq!(runtime.try_validate_profile_game_delivery("dcs", "reader-b").unwrap(), replacement.then_some(()));
        drop(writer);
        if replacement {
            assert_eq!(runtime.try_validate_profile_game_delivery("dcs", "reader-b").unwrap(), Some(()));
            let _writer = runtime.state.lock().unwrap();
            assert_eq!(runtime.try_validate_profile_game_delivery("dcs", "reader-b").unwrap(), Some(()));
        }
    }
}

#[test]
fn failed_lookups_cannot_displace_the_committed_reader_and_retirement_history_is_bounded() {
    let runtime = game_owner();
    runtime.validate_profile_game_delivery("dcs", "reader-a").unwrap();
    for index in 0..OWNER_CAPACITY {
        assert!(runtime.validate_profile_game_delivery("dcs", &format!("unknown-{index}")).is_err());
    }
    {
        let _writer = runtime.state.lock().unwrap();
        assert_eq!(runtime.try_validate_profile_game_delivery("dcs", "reader-a").unwrap(), Some(()));
        assert_eq!(runtime.try_validate_profile_game_delivery("dcs", "unknown-0").unwrap(), None);
    }
    for index in 0..=OWNER_CAPACITY {
        runtime.state.lock().unwrap().game_deliveries.get_mut("dcs").unwrap().session = format!("reader-{index}");
    }
    let _writer = runtime.state.lock().unwrap();
    assert_eq!(runtime.state.confirmed.lock().unwrap().retired_readers.len(), OWNER_CAPACITY);
    assert_eq!(runtime.try_validate_profile_game_delivery("dcs", "reader-a").unwrap(), None);
    assert!(runtime.try_validate_profile_game_delivery("dcs", "reader-0").is_err());
    assert_eq!(runtime.try_validate_profile_game_delivery("dcs", &format!("reader-{OWNER_CAPACITY}")).unwrap(), Some(()));
}
