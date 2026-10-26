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
        "../../../../../crates/companion-service/tests/fixtures/c1-flight-publication.json"
    ))
    .unwrap();
    osg_companion_service::Publication::validate_legacy_test_fixture(upload).unwrap()
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
