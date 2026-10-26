#[test]
fn restored_host_metadata_remains_a_distinct_offline_controller_without_guessing_osg_identity() {
    use osg_controller_input::registry::{Identity, SavedSource};
    let (mut sources, temporary) = sources();
    let path = temporary.path().join("controllers.json");
    let saved = SavedSource {
        id: uuid::Uuid::new_v4().to_string(),
        name: "OpenSimGear named third party".into(),
        role: "controller".into(),
        identity: Identity {
            backend: "fixture".into(),
            vendor_id: 0x1209,
            product_id: 1,
            serial: Some("123456789012345678901234".into()),
            collection: "joystick".into(),
            fingerprint: "0".repeat(64),
            path: "remembered-physical-path".into(),
        },
    };
    std::fs::write(&path, serde_json::to_vec(&vec![saved]).unwrap()).unwrap();
    crate::persistence::test_support::record(
        &path,
        "controllers",
        "identities",
        &std::fs::read(&path).unwrap(),
    );
    sources.controllers = Arc::new(osg_controller_input::ControllerService::new(
        Some(path),
        false,
    ));
    let batches = sources.collect(1);
    let batch = batches
        .into_iter()
        .find(|(scope, _)| scope == "host-controllers")
        .unwrap()
        .1
        .unwrap();
    let item = &batch.observations[0];
    assert_eq!(item.resource_type, ResourceType::HostController);
    assert!(item.route.is_none());
    assert!(item.ports.is_none());
    assert!(item
        .evidence
        .iter()
        .all(|proof| proof.kind != EvidenceKind::VerifiedPhysical));
}
#[test]
fn dormant_game_catalogue_failure_recovers_without_starting_an_integration() {
    let (mut sources, _temporary) = sources();
    let games = sources.games.resource_catalogue();
    let setup_revision = games
        .iter()
        .find(|game| game["id"] == "x-plane-11")
        .unwrap()["integrationSetup"]["providerSetupRevision"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    sources.catalogues.insert(
        "x-plane-11".into(),
        (
            Instant::now(),
            setup_revision,
            Err("Catalogue unavailable".into()),
        ),
    );
    let batches = sources.collect(1);
    let games = batches
        .into_iter()
        .filter(|(scope, _)| scope.starts_with("game:"))
        .flat_map(|(_, batch)| batch.unwrap().observations)
        .collect::<Vec<_>>();
    assert!(games
        .iter()
        .any(|game| game.attributes["provider"] == "x-plane"
            && game.ports.is_none()
            && game
                .diagnostics
                .iter()
                .any(|d| d == "Catalogue unavailable")));
    sources.catalogues.get_mut("x-plane-11").unwrap().0 = Instant::now() - Duration::from_secs(6);
    let batches = sources.collect(2);
    assert!(batches
        .into_iter()
        .filter(|(scope, _)| scope.starts_with("game:"))
        .flat_map(|(_, batch)| batch.unwrap().observations)
        .any(|game| game.attributes["provider"] == "x-plane"
            && game.ports.is_some()
            && !game.route.unwrap().writable));
}
#[test]
fn game_catalogue_cache_is_scoped_by_game_and_invalidates_changed_setup_revisions() {
    let (mut sources, _temporary) = sources();
    let first = sources.collect(1);
    let game = |batches: Vec<(String, Result<ProviderBatch, String>)>, id: &str| {
        batches
            .into_iter()
            .find(|(scope, _)| scope == &format!("game:{id}"))
            .unwrap()
            .1
            .unwrap()
            .observations
            .remove(0)
    };
    let initial = game(first, "x-plane-11");
    assert!(initial.ports.as_ref().unwrap().total > 0);
    assert_ne!(
        sources.catalogues["x-plane-11"].1,
        sources.catalogues["x-plane-12"].1
    );
    // A cached error for one installation never suppresses another version's
    // read-only authoring catalogue, even though both use the same provider.
    sources.catalogues.get_mut("x-plane-11").unwrap().2 = Err("Old catalogue failed".into());
    let second = sources.collect(2);
    let xplane11 = game(second.clone(), "x-plane-11");
    let xplane12 = game(second, "x-plane-12");
    assert!(xplane11.ports.is_none());
    assert!(xplane12.ports.as_ref().unwrap().total > 0);
    sources.catalogues.get_mut("x-plane-11").unwrap().1 = "old-setup-revision".into();
    let refreshed = game(sources.collect(3), "x-plane-11");
    assert!(refreshed.ports.as_ref().unwrap().total > 0);
    assert_eq!(
        refreshed.attributes["providerSetupRevision"],
        initial.attributes["providerSetupRevision"]
    );
    assert_eq!(
        refreshed.attributes["catalogueRevision"],
        initial.attributes["catalogueRevision"]
    );
    assert!(refreshed.attributes["providerSetupRevision"].starts_with("sha256:"));
    assert!(!refreshed.route.as_ref().unwrap().baseline_ready);
    assert!(!refreshed.route.unwrap().writable);
}
#[test]
fn capability_refresh_uses_only_bounded_read_commands_and_deduplicates_inflight_gateway() {
    let (mut sources, _temporary) = sources();
    sources.native = true;
    let calls = Arc::new(Mutex::new(vec![]));
    let recorded = calls.clone();
    let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let released = release.clone();
    sources.panel = Arc::new(move |request| {
        recorded.lock().unwrap().push(request);
        while !released.load(std::sync::atomic::Ordering::Acquire) {
            thread::yield_now();
        }
        json!({})
    });
    let mut state = can_state();
    state["moduleEvents"] = json!({});
    state["moduleActions"] = json!({});
    sources.refresh_can_capabilities("gateway", &state);
    let deadline = Instant::now() + Duration::from_secs(2);
    while calls.lock().unwrap().is_empty() && Instant::now() < deadline {
        thread::yield_now();
    }
    assert_eq!(calls.lock().unwrap().len(), 1);
    sources.refresh_can_capabilities("gateway", &state);
    release.store(true, std::sync::atomic::Ordering::Release);
    let deadline = Instant::now() + Duration::from_secs(2);
    while sources.refreshing.lock().unwrap().contains("gateway") && Instant::now() < deadline {
        thread::yield_now();
    }
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls
            .iter()
            .map(|v| v["command"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["events_state", "action_describe"]
    );
    assert!(calls
        .iter()
        .all(|v| v["moduleId"] == MODULE && v["clientId"] == "profile-resource-discovery"));
}

#[test]
fn retained_collector_preserves_provider_sessions_across_read_only_refreshes() {
    let (sources, _temporary) = sources();
    let collect = collector(
        sources.controllers,
        sources.companions,
        sources.host,
        sources.games,
        sources.gateways,
        sources.panel,
        sources.usb,
        false,
    );
    let first = collect(1)
        .into_iter()
        .map(|(scope, batch)| (scope, batch.unwrap()))
        .collect::<BTreeMap<_, _>>();
    let next = collect(2)
        .into_iter()
        .map(|(scope, batch)| (scope, batch.unwrap()))
        .collect::<BTreeMap<_, _>>();
    assert!(
        first.contains_key("host-controllers")
            && first.contains_key("standalone-usb")
            && first.contains_key("companions")
    );
    for (scope, previous) in first {
        assert_eq!(previous.session, next[&scope].session);
        assert_eq!(next[&scope].revision, 2);
        assert!(next[&scope].complete);
    }
}

#[test]
fn can_action_only_capabilities_publish_without_inventing_an_event_baseline() {
    let (sources, _temporary) = sources();
    let mut state = can_state();
    state["moduleEvents"][MODULE] =
        json!({"status":"unsupported","route":[1,1,2,100],"configurationGeneration":1});
    state["moduleActions"][MODULE] = json!({"status":"ready","route":[1,1,2,100],"configurationGeneration":1,"actions":[setter()]});
    let item = sources
        .can_observations("gateway", &state, &BTreeMap::new())
        .unwrap()
        .remove(0);
    assert!(!item.route.as_ref().unwrap().baseline_ready);
    assert!(item.route.as_ref().unwrap().writable);
    assert_eq!(item.ports.as_ref().unwrap().ports.len(), 1);
    let mut registry = Registry::new();
    registry
        .reconcile(batch("gateway", "session", 1, vec![item]))
        .unwrap();
    let id = registry.list(0, 32).unwrap().resources[0].id.clone();
    assert!(registry.token(&id, "action:led", Access::Invoke).is_ok());
    assert!(registry.token(&id, "button:0", Access::Read).is_err());
    state["moduleEvents"][MODULE]["configurationGeneration"] = json!(0);
    assert!(sources
        .can_observations("gateway", &state, &BTreeMap::new())
        .unwrap()[0]
        .ports
        .is_none());
}
#[test]
fn known_negative_can_components_are_not_reprobed_until_their_route_or_configuration_changes() {
    let (mut sources, _temporary) = sources();
    sources.native = true;
    let mut state = can_state();
    state["moduleEvents"][MODULE] =
        json!({"status":"unsupported","route":[1,1,2,100],"configurationGeneration":1});
    sources.refresh_can_capabilities("gateway", &state); // The fixture panel panics on any I/O.
    assert!(sources.refreshing.lock().unwrap().is_empty());
    let calls = Arc::new(Mutex::new(vec![]));
    let captured = calls.clone();
    sources.panel = Arc::new(move |request| {
        captured.lock().unwrap().push(request);
        json!({})
    });
    state["moduleEvents"][MODULE]["configurationGeneration"] = json!(0);
    sources.refresh_can_capabilities("gateway", &state);
    let deadline = Instant::now() + Duration::from_secs(2);
    while sources.refreshing.lock().unwrap().contains("gateway") && Instant::now() < deadline {
        thread::yield_now();
    }
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["command"], "events_state");
}
