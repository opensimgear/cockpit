use super::*;
use osg_profiles::resources::{EvidenceKind, IdentityEvidence, ResourceType, Transport};
use osg_profiles::resources_ports::{PortContract, PortDescriptor, PortUpdate, ScalarSchema};

fn metadata(game: &str) -> Observation {
    Observation {
        key: game.into(), name: game.into(), resource_type: ResourceType::GameIntegration,
        evidence: vec![IdentityEvidence { kind: EvidenceKind::GameScope, value: game.into() }],
        attributes: attributes([
            ("gameId", game.into()), ("integrationKey", "dcs".into()),
            ("providerSetupRevision", "setup".into()), ("catalogueRevision", "catalogue".into()),
            ("gameContextStatus", "waiting-profile".into()), ("processVerified", "true".into()),
        ]),
        diagnostics: vec![],
        route: Some(Route {
            id: format!("game:{game}"), transport: Transport::Integration,
            provider_generation: "setup:catalogue".into(), attributes: BTreeMap::new(),
            baseline_ready: false, writable: false, priority: 0,
        }),
        ports: Some(PortUpdate::complete(vec![PortDescriptor {
            id: "pressed".into(), name: "Pressed".into(), direction: osg_profiles::Direction::Output,
            contract: PortContract::State { schema: ScalarSchema::Boolean {}, freshness_ms: 1000, fallback: None },
        }]).unwrap()),
    }
}
fn collect(games: &[&str], revision: u64) -> Vec<(String, Result<ProviderBatch, String>)> {
    let mut batches = vec![("game-catalogue".into(), Ok(batch("game-catalogue", "native", revision, vec![])))];
    batches.extend(games.iter().map(|game| {
        let scope = format!("game:{game}");
        (scope.clone(), Ok(batch(&scope, "native", revision, vec![metadata(game)])))
    }));
    batches
}
fn runtime() -> Runtime { Runtime::new(None, Arc::new(|_| vec![]), false) }

fn seed_overlay(registry: &mut Registry) {
    registry.reconcile(batch("game:dcs", "native", 1, vec![metadata("dcs")])).unwrap();
    let mut overlay = metadata("dcs");
    overlay.ports.as_mut().unwrap().ports[0].id = "obsolete".into();
    overlay.route.as_mut().unwrap().id = "profile-game:dcs".into();
    registry.reconcile(batch("profile-game:dcs", "reader", 1, vec![overlay])).unwrap();
}

#[test]
fn restart_persists_retirement_of_reader_only_catalogues() {
    let directory = crate::test_directory();
    let root = directory.path().join("resources");
    let mut registry = Registry::new();
    seed_overlay(&mut registry);
    let id = registry.list(0, 1).unwrap().resources[0].id.clone();
    Store::new(root.clone()).save(&registry.saved()).unwrap();
    let runtime = Runtime::new(Some(root.clone()), Arc::new(|_| vec![]), false);
    assert!(runtime.state.lock().unwrap().pending_save);
    assert_eq!(runtime.state.lock().unwrap().registry.inspect(&id, 0, 128).unwrap().ports.len(), 1);
    runtime.refresh_native().unwrap();
    assert!(!runtime.state.lock().unwrap().pending_save);
    let reopened = Registry::restore(Store::new(root).load().unwrap().unwrap()).unwrap();
    assert_eq!(reopened.inspect(&id, 0, 128).unwrap().ports[0].descriptor.id, "pressed");
}

#[test]
fn changed_native_catalogue_retires_delivery_only_ports() {
    let runtime = runtime();
    {
        let mut state = runtime.state.lock().unwrap();
        seed_overlay(&mut state.registry);
        state.game_deliveries.insert("dcs".into(), batch("profile-game:dcs", "reader", 1, vec![metadata("dcs")]));
    }
    let id = runtime.state.lock().unwrap().registry.list(0, 1).unwrap().resources[0].id.clone();
    assert_eq!(runtime.state.lock().unwrap().registry.inspect(&id, 0, 128).unwrap().ports.len(), 2);
    Runtime::adopt(&runtime.state, collect(&["dcs"], 2));
    let state = runtime.state.lock().unwrap();
    assert!(state.game_deliveries.is_empty());
    assert_eq!(state.registry.inspect(&id, 0, 128).unwrap().ports[0].descriptor.id, "pressed");
    assert_eq!(state.registry.inspect(&id, 0, 128).unwrap().ports.len(), 1);
}

#[test]
fn authoritative_catalogue_removes_resources_and_all_native_delivery_caches() {
    let runtime = runtime();
    Runtime::adopt(&runtime.state, collect(&["removed", "dcs"], 1));
    let stale = runtime.state.lock().unwrap().registry.list(0, 128).unwrap().resources
        .into_iter().find(|resource| resource.attributes["gameId"] == "removed").unwrap();
    let mut delivery = batch("profile-game:removed", "reader", 1, vec![metadata("removed")]);
    {
        let mut state = runtime.state.lock().unwrap();
        assert!(state.game_metadata.contains_key("removed"));
        state.game_deliveries.insert("removed".into(), delivery.clone());
        state.game_command_withdrawals.insert(("removed".into(), "reader".into()), BTreeSet::from(["pressed".into()]));
        state.fingerprints.insert("profile-game:removed".into(), "old".into());
    }
    // The marker fingerprint has not changed, and dcs has not changed either.
    Runtime::adopt(&runtime.state, collect(&["dcs"], 2));
    {
        let state = runtime.state.lock().unwrap();
        assert!(state.registry.inspect(&stale.id, 0, 1).is_err());
        assert_eq!(state.registry.list(0, 128).unwrap().resources.len(), 1);
        assert!(!state.game_metadata.contains_key("removed"));
        assert!(state.game_deliveries.is_empty());
        assert!(state.game_command_withdrawals.is_empty());
        assert!(!state.fingerprints.contains_key("game:removed"));
        assert!(!state.fingerprints.contains_key("profile-game:removed"));
    }
    delivery.observations[0].route.as_mut().unwrap().id = "profile-game:removed".into();
    assert!(runtime.publish_profile_game_delivery(delivery).is_err());
    Runtime::adopt(&runtime.state, collect(&["removed", "dcs"], 3));
    assert!(runtime.state.lock().unwrap().game_metadata.contains_key("removed"));
}

#[test]
fn missing_failed_incomplete_or_invalid_snapshot_cannot_retire_configured_sources() {
    let runtime = runtime();
    Runtime::adopt(&runtime.state, collect(&["dcs"], 1));
    Runtime::adopt(&runtime.state, vec![("game-catalogue".into(), Err("Cannot read configuration".into()))]);
    Runtime::adopt(&runtime.state, vec![]);
    let mut incomplete = collect(&[], 2);
    incomplete[0].1.as_mut().unwrap().complete = false;
    Runtime::adopt(&runtime.state, incomplete);
    let mut invalid = collect(&[], 3);
    invalid[0].1.as_mut().unwrap().observations.push(metadata("dcs"));
    Runtime::adopt(&runtime.state, invalid);
    let mut failed_game = collect(&["dcs"], 4);
    failed_game[1].1 = Err("Temporary SDK failure".into());
    Runtime::adopt(&runtime.state, failed_game);
    assert_eq!(runtime.state.lock().unwrap().registry.list(0, 128).unwrap().resources.len(), 1);
    Runtime::adopt(&runtime.state, collect(&[], 5));
    assert!(runtime.state.lock().unwrap().registry.list(0, 128).unwrap().resources.is_empty());
}
