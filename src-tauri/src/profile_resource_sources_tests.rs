use super::*;
use osg_profiles::resources::{Access, Registry};
const MODULE: &str = "4f534744123456789012345678901234";
fn sources() -> (Sources, tempfile::TempDir) {
    let temporary = {
        let parent = std::env::temp_dir();
        #[cfg(unix)]
        let parent = parent.canonicalize().unwrap();
        tempfile::tempdir_in(parent).unwrap()
    };
    let panel: Panel = Arc::new(|_| {
        panic!("Read-only inventory must not issue transport requests in this fixture")
    });
    let controllers = Arc::new(osg_controller_input::ControllerService::new(None, false));
    let companions = Arc::new(osg_mapping_host::companions::CompanionProvider::new(
        Arc::new(|| 0),
    ));
    let host = Arc::new(crate::companion_host::Runtime::new(
        None,
        companions.clone(),
    ));
    let games = Arc::new(crate::games::GamesRuntime::new(
        panel.clone(),
        temporary.path().join("games.json"),
        Arc::new(crate::game_discovery::NativePlatform),
    ));
    (
        Sources {
            session: "provider-session".into(),
            controllers,
            companions,
            host,
            games,
            gateways: Arc::new(|| vec![]),
            panel,
            native: false,
            usb: Arc::new(crate::module_usb_fleet::Fleet::new(
                Arc::new(|| panic!("No serial enumeration in a synthetic provider")),
                false,
            )),
            can_scopes: BTreeSet::new(),
            refreshing: Arc::new(Mutex::new(BTreeSet::new())),
            catalogues: BTreeMap::new(),
        },
        temporary,
    )
}
fn can_state() -> Value {
    json!({"connected":true,"epoch":1,"canProtocol":3,"modules":[{"id":MODULE,"name":"OSG module","online":true,"boot":100,"node":1,"lease":2,"configurationGeneration":1,"canConfigure":true}],
            "moduleEvents":{MODULE:{"ok":true,"schemaVersion":1,"stream":7,"head":0,"oldest":1,"overflow":0,"buttonCount":1,"held":[false],"encoderButtons":[],"route":[1,1,2,100],"configurationGeneration":1}},
            "moduleActions":{MODULE:{"status":"unsupported","route":[1,1,2,100],"configurationGeneration":1}}})
}
#[test]
fn inventory_retains_distinct_gateway_modules_and_closed_game_ports_without_effects() {
    let (mut sources, _temporary) = sources();
    sources.gateways = Arc::new(|| {
        vec![
            ("stm32:aaaaaaaaaaaaaaaaaaaaaaaa".into(), can_state()),
            ("stm32:bbbbbbbbbbbbbbbbbbbbbbbb".into(), can_state()),
        ]
    });
    let mut registry = Registry::new();
    for (_, batch) in sources.collect(1) {
        registry.reconcile(batch.unwrap()).unwrap();
    }
    let page = registry.list(0, 32).unwrap();
    let modules = page
        .resources
        .iter()
        .filter(|r| r.resource_type == ResourceType::OsgModule)
        .collect::<Vec<_>>();
    assert_eq!(modules.len(), 2);
    assert_ne!(modules[0].id, modules[1].id);
    assert!(modules.iter().all(|r| r.port_count == 2)); // state and ordered events; no hats
    let first = modules[0].id.clone();
    let token = registry.token(&first, "button:0", Access::Read).unwrap();
    let integrations = page
        .resources
        .iter()
        .filter(|r| r.resource_type == ResourceType::GameIntegration)
        .collect::<Vec<_>>();
    assert_eq!(integrations.len(), 4);
    assert!(integrations
        .iter()
        .filter(|r| r.attributes["provider"] != "dcs-bios")
        .all(|r| r.port_count > 0));
    sources.gateways = Arc::new(|| vec![]);
    for (_, batch) in sources.collect(2) {
        registry.reconcile(batch.unwrap()).unwrap();
    }
    let offline = registry.inspect(&first, 0, 32).unwrap();
    assert_eq!(
        offline.resource.availability,
        osg_profiles::resources::Availability::Offline
    );
    assert_eq!(offline.ports.len(), 2);
    assert!(registry
        .validate_token(&token, "button:0", Access::Read)
        .is_err());
}
#[test]
fn stale_can_cache_cannot_verify_ports_on_a_new_boot() {
    let (sources, _temporary) = sources();
    let mut state = can_state();
    state["modules"][0]["boot"] = json!(101);
    let items = sources
        .can_observations("gateway", &state, &BTreeMap::new())
        .unwrap();
    assert!(items[0].ports.is_none());
    assert!(!items[0].route.as_ref().unwrap().baseline_ready);
    let verified = BTreeMap::from([(MODULE.into(), ("123456789012345678901234".into(), 100))]);
    let conflicts = sources
        .can_observations("gateway", &state, &verified)
        .unwrap();
    assert!(!conflicts[0].route.as_ref().unwrap().writable);
    assert!(conflicts[0]
        .evidence
        .iter()
        .all(|e| e.kind != EvidenceKind::VerifiedPhysical));
}

fn usb_slot() -> crate::module_usb_fleet::SlotSnapshot {
    let route = crate::module_usb::UsbRouteToken {
        module_id: MODULE.into(),
        serial: "123456789012345678901234".into(),
        connection_generation: 11,
        boot: 100,
        configuration_generation: 1,
    };
    let marker = json!({"route":route,"boot":100,"status":"unsupported"});
    let mut events = can_state()["moduleEvents"][MODULE].clone();
    events["route"] = json!(route);
    crate::module_usb_fleet::SlotSnapshot {
        key: "usb-fixture".into(),
        present: true,
        automatic: true,
        admissible: true,
        diagnostics: vec![],
        port: crate::transport::PortInfo {
            path: "COM8".into(),
            serial_number: Some(route.serial.clone()),
            vendor_id: Some("1209".into()),
            product_id: Some("0001".into()),
            manufacturer: Some("OpenSimGear".into()),
            friendly_name: Some("Device Template".into()),
        },
        snapshot: crate::module_usb::UsbSnapshot {
            connected: true,
            port: Some("COM8".into()),
            identity: Some(crate::module_console::UsbIdentity {
                model: "device-template".into(),
                board: "0.1.0".into(),
                profile: "USB fixture".into(),
                module_id: MODULE.into(),
                serial: route.serial.clone(),
                protocol: 3,
                button_count: 1,
                axis_count: 0,
                hat_count: 0,
            }),
            route: Some(route),
            events: Some(events),
            actions: Some(marker),
            ..Default::default()
        },
    }
}
fn setter() -> Value {
    json!({"id":"led","label":"LED","endpoint":"led","semantics":"setter","readable":true,"parameters":[{"id":"on","label":"On","type":"boolean"}]})
}
#[test]
fn usb_translation_requires_current_complete_capabilities_and_withdraws_stale_authority() {
    let mut slot = usb_slot();
    slot.snapshot.actions =
        Some(json!({"route":slot.snapshot.route,"status":"ready","actions":[setter()]}));
    let first = Sources::usb_observation(slot.clone()).unwrap().unwrap();
    assert_eq!(first.ports.as_ref().unwrap().ports.len(), 3);
    assert_eq!(first.route.as_ref().unwrap().transport, Transport::Usb);
    assert!(first.route.as_ref().unwrap().baseline_ready);
    let mut registry = Registry::new();
    registry
        .reconcile(batch("usb", "session", 1, vec![first]))
        .unwrap();
    let id = registry.list(0, 32).unwrap().resources[0].id.clone();
    let token = registry.token(&id, "action:led", Access::Invoke).unwrap();
    slot.snapshot
        .route
        .as_mut()
        .unwrap()
        .configuration_generation = 2;
    slot.snapshot.events.as_mut().unwrap()["route"] = json!(slot.snapshot.route);
    let partial = Sources::usb_observation(slot).unwrap().unwrap();
    assert!(partial.ports.is_none());
    registry
        .reconcile(batch("usb", "session", 2, vec![partial]))
        .unwrap();
    assert_eq!(registry.inspect(&id, 0, 32).unwrap().ports.len(), 3);
    assert!(registry
        .validate_token(&token, "action:led", Access::Invoke)
        .is_err());
    assert!(registry.token(&id, "action:led", Access::Invoke).is_err());
}
#[test]
fn unsupported_usb_components_are_known_empty_without_fake_event_baselines() {
    let mut slot = usb_slot();
    slot.snapshot.events = Some(json!({"route":slot.snapshot.route,"status":"unsupported"}));
    slot.snapshot.actions =
        Some(json!({"route":slot.snapshot.route,"status":"ready","actions":[setter()]}));
    let observed = Sources::usb_observation(slot.clone()).unwrap().unwrap();
    assert_eq!(observed.ports.as_ref().unwrap().ports.len(), 1);
    assert_eq!(observed.ports.as_ref().unwrap().ports[0].id, "action:led");
    assert!(!observed.route.as_ref().unwrap().baseline_ready);
    assert!(observed.route.as_ref().unwrap().writable);
    slot.snapshot.actions = Some(json!({"route":slot.snapshot.route,"status":"unsupported"}));
    assert!(Sources::usb_observation(slot)
        .unwrap()
        .unwrap()
        .ports
        .unwrap()
        .ports
        .is_empty());
}
#[test]
fn offline_unverified_and_uncertain_usb_observations_never_offer_writable_routes() {
    let mut absent = usb_slot();
    absent.snapshot.identity = None;
    assert!(Sources::usb_observation(absent).unwrap().is_none());
    for change in ["offline", "removed", "inadmissible", "uncertain", "error"] {
        let mut slot = usb_slot();
        match change {
            "offline" => slot.snapshot.connected = false,
            "removed" => slot.present = false,
            "inadmissible" => slot.admissible = false,
            "uncertain" => {
                slot.snapshot.actions =
                    Some(json!({"route":slot.snapshot.route,"status":"uncertain"}))
            }
            _ => slot.snapshot.error = Some("Lost response".into()),
        }
        let observed = Sources::usb_observation(slot).unwrap().unwrap();
        assert!(
            observed.route.as_ref().is_none_or(|route| !route.writable),
            "{change}"
        );
    }
    let mut malformed = usb_slot();
    malformed.snapshot.events.as_mut().unwrap()["held"] = json!([]);
    assert!(Sources::usb_observation(malformed).is_err());
}
#[test]
fn every_can_route_generation_component_invalidates_cached_capabilities() {
    let (sources, _temporary) = sources();
    for change in ["epoch", "boot", "node", "lease", "config", "actions"] {
        let mut state = can_state();
        match change {
            "epoch" => state["epoch"] = json!(2),
            "boot" => state["modules"][0]["boot"] = json!(101),
            "node" => state["modules"][0]["node"] = json!(2),
            "lease" => state["modules"][0]["lease"] = json!(3),
            "config" => state["modules"][0]["configurationGeneration"] = json!(2),
            _ => state["moduleActions"][MODULE]["configurationGeneration"] = json!(0),
        }
        let observed = sources
            .can_observations("gateway", &state, &BTreeMap::new())
            .unwrap()
            .remove(0);
        assert!(observed.ports.is_none(), "{change}");
    }
    let mut offline = can_state();
    offline["connected"] = json!(false);
    assert!(sources
        .can_observations("gateway", &offline, &BTreeMap::new())
        .unwrap()[0]
        .route
        .is_none());
}
#[test]
fn verified_usb_can_boot_proof_and_action_state_are_required_for_write_selection() {
    let (sources, _temporary) = sources();
    let verified = BTreeMap::from([(MODULE.into(), ("123456789012345678901234".into(), 100))]);
    let mut state = can_state();
    state["moduleActions"][MODULE] = json!({"status":"ready","route":[1,1,2,100],"configurationGeneration":1,"actions":[setter()]});
    let item = sources
        .can_observations("gateway", &state, &verified)
        .unwrap()
        .remove(0);
    assert!(item
        .evidence
        .iter()
        .any(|e| e.kind == EvidenceKind::VerifiedPhysical));
    assert_eq!(item.ports.unwrap().ports.len(), 3);
    state["moduleActions"][MODULE]["status"] = json!("uncertain");
    assert!(
        !sources
            .can_observations("gateway", &state, &verified)
            .unwrap()[0]
            .route
            .as_ref()
            .unwrap()
            .writable
    );
    state["moduleActions"][MODULE]["status"] = json!("ready");
    state["canProtocol"] = json!(2);
    let item = sources
        .can_observations("gateway", &state, &verified)
        .unwrap()
        .remove(0);
    assert!(item
        .evidence
        .iter()
        .all(|e| e.kind != EvidenceKind::VerifiedPhysical));
    assert!(!item.route.unwrap().writable);
}
fn publication() -> osg_companion_service::Publication {
    let upload = serde_json::from_str(include_str!(
        "../../crates/companion-service/tests/fixtures/c1-flight-publication.json"
    ))
    .unwrap();
    osg_companion_service::Publication::validate(upload).unwrap()
}
fn companion(
    publication: &osg_companion_service::Publication,
) -> osg_mapping_host::companions::ResourceDescriptor {
    use osg_companion_service::SourceSink;
    let provider = osg_mapping_host::companions::CompanionProvider::new(Arc::new(|| 0));
    provider
        .register(publication.definition("test-companion"))
        .unwrap();
    let mut descriptor = provider.resource_inventory().remove(0);
    descriptor.online = true;
    descriptor.baseline_ready = true;
    descriptor.generation = 4;
    descriptor
}
#[test]
fn companion_translation_uses_published_controls_feedback_units_and_native_transport_proof() {
    use osg_companion_service::SourceTransport;
    use osg_profiles::resources_ports::{PortContract, ScalarSchema};
    let publication = publication();
    let descriptor = companion(&publication);
    for (native, expected) in [
        (SourceTransport::Usb, Transport::Usb),
        (SourceTransport::Network, Transport::Network),
    ] {
        let observed = Sources::companion_observation(
            descriptor.clone(),
            Ok(publication.clone()),
            Some(native),
        )
        .unwrap();
        assert_eq!(observed.resource_type, ResourceType::Companion);
        assert_eq!(observed.route.as_ref().unwrap().transport, expected);
        assert!(observed.route.as_ref().unwrap().writable);
        let ports = &observed.ports.unwrap().ports;
        assert_eq!(
            ports.len(),
            publication.controls.len()
                + publication
                    .feedback
                    .iter()
                    .map(|f| f.output_ids.len())
                    .sum::<usize>()
        );
        assert!(ports.iter().any(|p| matches!(
            &p.contract,
            PortContract::State {
                schema: ScalarSchema::Boolean {},
                ..
            }
        )));
        assert!(ports.iter().any(|p| matches!(
            &p.contract,
            PortContract::State {
                schema: ScalarSchema::Text { max_bytes: 256, .. },
                ..
            }
        ) && p.direction == Direction::Input));
        for feedback in &publication.feedback {
            if feedback.kind == osg_companion_service::FeedbackKind::Number {
                for output in &feedback.output_ids {
                    let port = ports
                        .iter()
                        .find(|p| p.id == format!("feedback:{}:{output}", feedback.channel_id))
                        .unwrap();
                    assert!(
                        matches!(&port.contract,PortContract::State {schema:ScalarSchema::Number {unit,..},fallback:None,..} if *unit==feedback.unit)
                    );
                }
            }
        }
    }
    assert!(
        Sources::companion_observation(descriptor, Ok(publication), None)
            .unwrap()
            .route
            .is_none()
    );
}
#[test]
fn changed_or_missing_companion_publications_retain_metadata_without_write_authority() {
    let publication = publication();
    let descriptor = companion(&publication);
    let initial = Sources::companion_observation(
        descriptor.clone(),
        Ok(publication.clone()),
        Some(osg_companion_service::SourceTransport::Network),
    )
    .unwrap();
    let mut registry = Registry::new();
    registry
        .reconcile(batch("companions", "session", 1, vec![initial]))
        .unwrap();
    let id = registry.list(0, 32).unwrap().resources[0].id.clone();
    let total = registry.inspect(&id, 0, 128).unwrap().total;
    let mut changed = publication.clone();
    changed.publication_hash = format!("sha256:{}", "f".repeat(64));
    for (revision, replacement) in [
        (2, Ok(changed)),
        (3, Err("Publication cache is unavailable".into())),
    ] {
        let item = Sources::companion_observation(
            descriptor.clone(),
            replacement,
            Some(osg_companion_service::SourceTransport::Network),
        )
        .unwrap();
        assert!(item.ports.is_none());
        assert!(!item.route.as_ref().unwrap().writable);
        registry
            .reconcile(batch("companions", "session", revision, vec![item]))
            .unwrap();
        assert_eq!(registry.inspect(&id, 0, 128).unwrap().total, total);
    }
    let mut no_baseline = descriptor;
    no_baseline.baseline_ready = false;
    no_baseline.fault = Some("Baseline expired".into());
    let item = Sources::companion_observation(
        no_baseline,
        Ok(publication),
        Some(osg_companion_service::SourceTransport::Usb),
    )
    .unwrap();
    assert!(!item.route.as_ref().unwrap().writable);
    assert!(item.diagnostics.iter().any(|v| v == "Baseline expired"));
}
#[test]
fn missing_cached_companion_publication_does_not_initialize_native_host_services() {
    use osg_companion_service::SourceSink;
    let (sources, _temporary) = sources();
    let publication = publication();
    sources
        .companions
        .register(publication.definition("test-companion"))
        .unwrap();
    let item = sources.companion_observations().unwrap().remove(0);
    assert!(item.ports.is_none());
    assert!(item.route.is_none());
    assert!(!item.diagnostics.is_empty());
}
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
