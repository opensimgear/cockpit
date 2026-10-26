#[test]
fn unpaired_retained_companion_descriptors_are_omitted_without_initializing_host_services() {
    use osg_companion_service::SourceSink;
    let (sources, _temporary) = sources();
    let publication = publication();
    sources
        .companions
        .register(publication.definition("test-companion"))
        .unwrap();
    assert!(sources.companion_observations().unwrap().is_empty());
}

#[test]
fn companion_membership_keeps_offline_assigned_panels_and_drops_revoked_or_unassigned_sources() {
    struct Memory(Mutex<Vec<u8>>);
    impl osg_companion_service::persistence::StateStorage for Memory {
        fn read(&self) -> Result<Option<Vec<u8>>, String> {
            Ok(Some(self.0.lock().unwrap().clone()))
        }
        fn write(&self, bytes: &[u8]) -> Result<(), String> {
            *self.0.lock().unwrap() = bytes.to_vec();
            Ok(())
        }
    }
    for removal in ["companion_unassign", "companion_revoke"] {
        let (mut sources, temporary) = sources();
        let publication = publication();
        let record = json!({"hostId":"inventory-host","credentials":[{"companionId":"paired","name":"Offline screen","tokenHash":"a".repeat(64),"kind":"browser"}],"instances":[{"instanceId":"test-companion","companionId":"paired","publicationHash":publication.publication_hash,"definition":publication.definition("test-companion")}]});
        let hub = osg_companion_service::Hub::with_existing_storage(
            Arc::new(Memory(Mutex::new(serde_json::to_vec(&record).unwrap()))),
            sources.companions.clone(),
        )
        .unwrap()
        .unwrap();
        sources.host = Arc::new(crate::companion_host::Runtime::with_profile_metadata(
            hub.clone(),
            sources.companions.clone(),
            temporary.path().into(),
        ));
        let items = sources.companion_observations().unwrap();
        assert_eq!(items.len(), 2);
        assert!(items
            .iter()
            .all(|item| item.route.is_none() && item.ports.is_none()));
        let component = items
            .iter()
            .find(|item| {
                item.attributes
                    .get("componentKind")
                    .is_some_and(|kind| kind == "paired-companion")
            })
            .unwrap();
        assert_eq!(component.name, "Offline screen");
        assert_eq!(component.key, "paired:paired");
        let request = if removal == "companion_unassign" {
            json!({"command":removal,"instanceId":"test-companion"})
        } else {
            json!({"command":removal,"companionId":"paired"})
        };
        hub.command(&request).unwrap();
        assert_eq!(
            sources.companions.resource_inventory().len(),
            1,
            "Sink definitions intentionally remain cached."
        );
        let remaining = sources.companion_observations().unwrap();
        if removal == "companion_unassign" {
            assert_eq!(remaining.len(), 1);
            assert_eq!(remaining[0].key, "paired:paired");
            assert!(remaining[0].ports.is_none());
            assert!(remaining[0].route.is_none());
            assert_eq!(
                remaining[0].evidence[0].value,
                osg_companion_service::profile::profile_instance_id("paired")
            );
        } else {
            assert!(remaining.is_empty());
        }
    }
}

#[test]
fn disconnected_or_vanished_gateway_is_not_an_authoritative_empty_inventory() {
    let (mut sources, _temporary) = sources();
    let inputs = Arc::new(Mutex::new(vec![("one".to_owned(), can_state())]));
    let feed = inputs.clone();
    sources.gateways = Arc::new(move || feed.lock().unwrap().clone());
    let initial = sources
        .collect(1)
        .into_iter()
        .find(|(scope, _)| scope == "gateway:one")
        .unwrap()
        .1
        .unwrap();
    assert!(initial.complete);
    assert_eq!(initial.observations.len(), 1);
    let identity = initial.observations[0].evidence.clone();
    let mut registry = Registry::new();
    registry.reconcile(initial).unwrap();
    let id = registry.list(0, 32).unwrap().resources[0].id.clone();
    let token = registry.token(&id, "button:0", Access::Read).unwrap();
    *inputs.lock().unwrap() = vec![("one".into(), json!({"connected":false,"modules":[]}))];
    for revision in [2, 3] {
        if revision == 3 {
            inputs.lock().unwrap().clear();
        }
        let absent = sources
            .collect(revision)
            .into_iter()
            .find(|(scope, _)| scope == "gateway:one")
            .unwrap()
            .1
            .unwrap();
        assert!(!absent.complete);
        assert_eq!(absent.observations.len(), 1);
        assert_eq!(absent.observations[0].evidence, identity);
        assert!(absent.observations[0].route.is_none() && absent.observations[0].ports.is_none());
        registry.reconcile(absent).unwrap();
        let retained = registry.inspect(&id, 0, 32).unwrap();
        assert_eq!(
            retained.resource.availability,
            osg_profiles::resources::Availability::Offline
        );
        assert!(retained.resource.connections.is_empty());
        assert_eq!(retained.ports.len(), 2);
        assert!(retained
            .ports
            .iter()
            .all(|port| !port.current && port.route_ids.is_empty()));
        assert!(registry.token(&id, "button:0", Access::Read).is_err());
        assert!(registry
            .validate_token(&token, "button:0", Access::Read)
            .is_err());
    }
}

#[test]
fn connected_gateway_inventory_or_capability_errors_remain_unavailable() {
    let (mut sources, _temporary) = sources();
    let inputs = Arc::new(Mutex::new(vec![("one".to_owned(), can_state())]));
    let feed = inputs.clone();
    sources.gateways = Arc::new(move || feed.lock().unwrap().clone());
    let mut registry = Registry::new();
    let initial = sources
        .collect(1)
        .into_iter()
        .find(|(scope, _)| scope == "gateway:one")
        .unwrap()
        .1
        .unwrap();
    registry.reconcile(initial).unwrap();
    let id = registry.list(0, 32).unwrap().resources[0].id.clone();
    for (index, fault) in ["inventory", "capability", "connection-state"]
        .into_iter()
        .enumerate()
    {
        let mut state = can_state();
        match fault {
            "inventory" => state["modules"] = Value::Null,
            "capability" => state["moduleEvents"][MODULE]["buttonCount"] = json!("invalid"),
            _ => {
                state.as_object_mut().unwrap().remove("connected");
            }
        }
        *inputs.lock().unwrap() = vec![("one".into(), state)];
        let failure = sources
            .collect(index as u64 + 2)
            .into_iter()
            .find(|(scope, _)| scope == "gateway:one")
            .unwrap()
            .1
            .unwrap_err();
        registry.provider_failed("gateway:one", &failure);
        let retained = registry.inspect(&id, 0, 32).unwrap();
        assert_eq!(
            retained.resource.availability,
            osg_profiles::resources::Availability::Unavailable
        );
        assert_eq!(retained.ports.len(), 2);
        assert!(retained.resource.connections.is_empty());
    }
    // A later confirmed absence can retire the failure without granting access.
    *inputs.lock().unwrap() = vec![("one".into(), json!({"connected":false}))];
    registry
        .reconcile(
            sources
                .collect(5)
                .into_iter()
                .find(|(scope, _)| scope == "gateway:one")
                .unwrap()
                .1
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        registry.inspect(&id, 0, 32).unwrap().resource.availability,
        osg_profiles::resources::Availability::Offline
    );
}

#[test]
fn disconnected_gateway_after_restart_retains_saved_identity_and_ports_without_a_live_cache() {
    let (mut sources, _temporary) = sources();
    sources.gateways = Arc::new(|| vec![("one".into(), can_state())]);
    let mut registry = Registry::new();
    registry
        .reconcile(
            sources
                .collect(1)
                .into_iter()
                .find(|(scope, _)| scope == "gateway:one")
                .unwrap()
                .1
                .unwrap(),
        )
        .unwrap();
    let id = registry.list(0, 32).unwrap().resources[0].id.clone();
    let mut restored = Registry::restore(registry.saved()).unwrap();
    sources.can_retained.clear();
    sources.session = "restarted-native-provider".into();
    sources.gateways = Arc::new(|| vec![("one".into(), json!({"connected":false,"modules":[]}))]);
    let absent = sources
        .collect(1)
        .into_iter()
        .find(|(scope, _)| scope == "gateway:one")
        .unwrap()
        .1
        .unwrap();
    assert!(!absent.complete);
    assert!(absent.observations.is_empty());
    restored.reconcile(absent).unwrap();
    let retained = restored.inspect(&id, 0, 32).unwrap();
    assert_eq!(
        retained.resource.availability,
        osg_profiles::resources::Availability::Offline
    );
    assert_eq!(retained.ports.len(), 2);
    assert!(retained.resource.connections.is_empty());
}

#[test]
fn unavailable_durable_inventory_never_becomes_an_authoritative_empty_batch() {
    let (mut sources, temporary) = sources();
    std::fs::write(
        temporary.path().join("companion-host-state.dpapi"),
        b"unreadable protected host state",
    )
    .unwrap();
    crate::persistence::test_support::record(
        &temporary.path().join("companion-host-state.dpapi"),
        "secrets",
        "companion-host",
        b"unreadable protected SQLite state",
    );
    sources.host = Arc::new(crate::companion_host::Runtime::new(
        Some(temporary.path().into()),
        sources.companions.clone(),
    ));
    assert!(sources.companion_observations().is_err());
    let controllers = temporary.path().join("controllers.json");
    std::fs::write(&controllers, b"corrupt controller settings").unwrap();
    crate::persistence::test_support::record(
        &controllers,
        "controllers",
        "identities",
        &std::fs::read(&controllers).unwrap(),
    );
    sources.controllers = Arc::new(osg_controller_input::ControllerService::new(
        Some(controllers),
        false,
    ));
    assert!(sources
        .controllers
        .authoritative_resource_inventory()
        .is_err());
    assert!(sources
        .can_observations(
            "gateway",
            &json!({"error":"not available"}),
            &BTreeMap::new()
        )
        .is_err());
}
