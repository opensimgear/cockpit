use super::*;
use osg_profiles::{
    resources::{EvidenceKind, IdentityEvidence, Observation, ProviderBatch},
    resources_ports::PortUpdate,
};

#[test]
fn preview_without_local_companion_assignment_returns_compiler_input() {
    let fixture = NativeFixture::new();
    let candidate = fixture.candidate(false);
    let mut preview = fixture
        .resources
        .snapshot_for_preparation(vec![])
        .unwrap()
        .0;
    assert!(preview.assignments.is_empty());
    let original = preview.clone();

    NativeProfileHost::project_companion_preview(
        &mut preview,
        &fixture.hub,
        &candidate.document,
    )
    .unwrap();

    assert!(preview.assignments.is_empty());
    assert_eq!(
        preview.resources.iter().map(|resource| &resource.summary.id).collect::<Vec<_>>(),
        original.resources.iter().map(|resource| &resource.summary.id).collect::<Vec<_>>()
    );
    let report = osg_profiles::compiler::compile(&candidate.document, &preview);
    assert!(!report.diagnostics.is_empty());
}

#[test]
fn offline_unassigned_pair_keeps_base_healthy_and_reconnects_its_desired_publication() {
    let mut fixture = NativeFixture::new();
    fixture.quit.store(true, Ordering::Release);
    fixture.peer.take().unwrap().join().unwrap();
    fixture.inventory.take().unwrap().join().unwrap();
    fixture
        .hub
        .command(&json!({"command":"companion_unassign","instanceId":"instance"}))
        .unwrap();
    let instance = osg_companion_service::profile::profile_instance_id("screen");
    let online = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let connected = online.clone();
    let hub = fixture.hub.clone();
    let provider = fixture.companions.profile_provider();
    let collect = Arc::new(move |revision| {
        let paired = hub
            .resource_companions()
            .into_iter()
            .find(|paired| paired.companion_id == "screen")
            .unwrap();
        let source = provider
            .resource_inventory()
            .into_iter()
            .find(|source| source.definition.source_id == paired.instance_id);
        let mut attributes = BTreeMap::from([
            ("componentKind".into(), "paired-companion".into()),
            ("companionId".into(), "screen".into()),
            (
                "pairedIdentityFingerprint".into(),
                paired.identity_fingerprint.clone(),
            ),
        ]);
        let mut ports = vec![];
        let route = if let Some(source) =
            source.filter(|source| source.online && source.baseline_ready)
        {
            let publication = hub.assigned_publication(&paired.instance_id).unwrap();
            ports =
                crate::profile_resource_ports::companion(&source.definition, &publication.feedback)
                    .unwrap();
            attributes.insert("publicationHash".into(), publication.publication_hash);
            attributes.insert(
                "capabilityFingerprint".into(),
                publication.capability_fingerprint,
            );
            Some(Route {
                id: format!("companion:{}", paired.instance_id),
                transport: Transport::Network,
                provider_generation: source.generation.to_string(),
                attributes: BTreeMap::new(),
                baseline_ready: true,
                writable: true,
                priority: 100,
            })
        } else if connected.load(Ordering::Acquire) {
            Some(Route {
                id: format!("companion:{}", paired.instance_id),
                transport: Transport::Integration,
                provider_generation: paired.identity_fingerprint,
                attributes: BTreeMap::from([("preparationOnly".into(), "paired-companion".into())]),
                baseline_ready: false,
                writable: false,
                priority: 0,
            })
        } else {
            None
        };
        vec![(
            "paired-companions".into(),
            Ok(ProviderBatch {
                scope: "paired-companions".into(),
                session: "offline-pair-fixture".into(),
                revision,
                complete: true,
                observations: vec![Observation {
                    key: "paired:screen".into(),
                    name: "Optional screen".into(),
                    resource_type: ResourceType::Companion,
                    evidence: vec![IdentityEvidence {
                        kind: EvidenceKind::CompanionInstance,
                        value: paired.instance_id,
                    }],
                    attributes,
                    diagnostics: vec![],
                    route,
                    ports: Some(PortUpdate::complete(ports).unwrap()),
                }],
            }),
        )]
    });
    fixture.resources = Arc::new(crate::profile_resources::Runtime::new(None, collect, false));
    fixture.resources.refresh_native().unwrap();
    let mut candidate = fixture.candidate(false);
    candidate.document.resources[0].required = false;
    candidate
        .document
        .processing_nodes
        .push(osg_profiles::ProcessingNode {
            id: "base".into(),
            name: "Independent Base".into(),
            draft: None,
            operation: osg_profiles::Operation::Constant {
                value: osg_profiles::PortValue::Boolean(true),
            },
            inputs: vec![],
            outputs: vec![osg_profiles::Port {
                id: "value".into(),
                name: "Value".into(),
                direction: osg_profiles::Direction::Output,
                contract: osg_profiles::PortContract::State {
                    schema: ScalarSchema::Boolean {},
                    freshness_ms: 1000,
                    fallback: None,
                },
            }],
        });
    let initial = fixture
        .resources
        .snapshot_for_preparation(candidate.assignments.clone())
        .unwrap()
        .0;
    assert_eq!(
        initial.resources[0].summary.availability,
        Availability::Offline
    );
    assert!(initial.resources[0].ports.is_empty());
    let (catalogues, bindings) = prepare_catalogues(&fixture.hub, &initial, &candidate).unwrap();
    assert_eq!(catalogues.len(), 1);
    assert_eq!(catalogues[0].instance_id, instance);
    assert_eq!(catalogues[0].ports.len(), 2);
    assert!(
        bindings.is_empty(),
        "An offline screen acquires no subscription or assignment"
    );
    let mut preview = initial.clone();
    assert!(
        osg_profiles::compiler::compile(&candidate.document, &preview)
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "portContractChanged")
    );
    NativeProfileHost::project_companion_preview(&mut preview, &fixture.hub, &candidate.document)
        .unwrap();
    let preview_report = osg_profiles::compiler::compile(&candidate.document, &preview);
    assert_eq!(
        preview_report.readiness,
        PlanReadiness::Ready,
        "{:?}",
        preview_report.diagnostics
    );
    assert!(preview_report
        .plan
        .unwrap()
        .sources
        .iter()
        .all(|source| source.readiness == PlanReadiness::Unavailable
            && source.token.is_none()
            && source.route.is_none()
            && source.preparation.is_none()));
    let (captured, pins) = fixture
        .resources
        .snapshot_for_preparation_with_companions(candidate.assignments.clone(), &catalogues)
        .unwrap();
    assert!(pins.is_empty() && captured.resources[0].tokens.is_empty());
    // Required virtual and desktop sinks reproduce the real multi-output rig:
    // losing the optional source contract at final capture must not block them.
    let mut multi_output = candidate.document.clone();
    let button = multi_output.resources[0].ports[0].clone();
    let mut virtual_port = button.clone();
    virtual_port.id = "button".into();
    virtual_port.direction = osg_profiles::Direction::Input;
    multi_output
        .virtual_outputs
        .push(osg_profiles::VirtualOutput {
            id: "virtual".into(),
            name: "Required virtual controller".into(),
            required: true,
            layout: osg_profiles::ControllerLayout {
                buttons: vec!["button".into()],
                axes: vec![],
                hats: vec![],
            },
            ports: vec![virtual_port],
        });
    let key = osg_mapping::desktop::Action::KeyHold {
        key: osg_mapping::desktop::Key(44),
    };
    multi_output
        .desktop_outputs
        .push(osg_profiles::DesktopOutput {
            id: "desktop".into(),
            name: "Required keyboard".into(),
            required: true,
            scope: Some(osg_profiles::DesktopScope::DesktopWide),
            ports: vec![osg_profiles::Port {
                id: "hold".into(),
                name: "Hold".into(),
                direction: osg_profiles::Direction::Input,
                contract: key.contract(),
            }],
            actions: vec![osg_profiles::DesktopActionDefinition {
                port_id: "hold".into(),
                action: key,
            }],
        });
    for (id, node, port) in [
        ("virtual-route", "virtual", "button"),
        ("desktop-route", "desktop", "hold"),
    ] {
        multi_output.bindings.push(osg_profiles::Binding {
            id: id.into(),
            from: osg_profiles::Endpoint {
                node_id: "panel".into(),
                port_id: button.id.clone(),
            },
            to: osg_profiles::Endpoint {
                node_id: node.into(),
                port_id: port.into(),
            },
        });
    }
    let mut prepared_capture = captured.clone();
    prepared_capture.backend.available = true;
    prepared_capture.backend.qualified = true;
    prepared_capture.backend.desktop_available = true;
    prepared_capture.backend.desktop_qualified = true;
    prepared_capture.backend.desktop_enabled = true;
    let prepared_report =
        osg_profiles::compiler::compile_for_preparation(&multi_output, &prepared_capture, &pins);
    assert_eq!(
        prepared_report.readiness,
        PlanReadiness::Ready,
        "{:?}",
        prepared_report.diagnostics
    );
    let mut raw_final = fixture
        .resources
        .snapshot_for_compile(candidate.assignments.clone())
        .unwrap();
    raw_final.backend = prepared_capture.backend.clone();
    assert_eq!(
        osg_profiles::compiler::compile(&multi_output, &raw_final).readiness,
        PlanReadiness::Blocked,
        "The raw final capture must reproduce the required-output regression"
    );
    let mut final_capture = fixture
        .resources
        .snapshot_for_compile_with_companions(candidate.assignments.clone(), &catalogues)
        .unwrap();
    final_capture.backend = prepared_capture.backend;
    let final_report = osg_profiles::compiler::compile(&multi_output, &final_capture);
    assert_eq!(
        final_report.readiness,
        PlanReadiness::Ready,
        "{:?}",
        final_report.diagnostics
    );
    assert_eq!(final_report.program_hash, prepared_report.program_hash);
    assert!(final_report
        .plan
        .unwrap()
        .sources
        .iter()
        .all(|source| source.readiness == PlanReadiness::Unavailable
            && source.token.is_none()
            && source.route.is_none()
            && source.preparation.is_none()));
    assert!(captured.resources[0]
        .ports
        .iter()
        .all(|port| !port.current && port.route_ids.is_empty()));
    assert!(
        fixture
            .resources
            .snapshot_for_compile(candidate.assignments.clone())
            .unwrap()
            .resources[0]
            .ports
            .is_empty(),
        "Prospective contracts cannot modify observed inventory"
    );
    candidate.stage = Some(
        NativeProfileHost::stage(
            &fixture.resources,
            &fixture.games,
            &fixture.companions,
            fixture.controllers.clone(),
            false,
            &candidate,
        )
        .unwrap(),
    );
    let mut coordinator = crate::profile_session::Coordinator::new(fixture.host(), None);
    coordinator.apply(candidate.clone()).unwrap();
    coordinator.poll().unwrap();
    assert_eq!(
        coordinator.status().state,
        crate::profile_session::State::Running
    );
    assert!(coordinator.status().degradation.is_empty());
    assert!(fixture.hub.resource_instance_ids().is_empty());
    let owner = coordinator.status().owner.clone().unwrap();
    let request = crate::profile_session::inspection::Request::parse(
        &json!({"command":"profile_runtime_inspect","owner":owner,"nodeId":"base"}),
    )
    .unwrap();
    let inspected = serde_json::to_value(coordinator.inspect(&request).unwrap()).unwrap();
    assert!(inspected["ports"]
        .as_array()
        .unwrap()
        .iter()
        .any(|port| port["sample"]
            == serde_json::to_value(osg_mapping::program::SignalSample::scalar(
                osg_profiles::PortValue::Boolean(true)
            ))
            .unwrap()));
    online.store(true, Ordering::Release);
    fixture.resources.refresh_native().unwrap();
    let readiness=coordinator.peripheral_change().expect("A connected paired metadata route can prepare its retained desired panel before an assignment exists");
    fixture.resources.refresh_native().unwrap();
    assert_eq!(coordinator.peripheral_change(), Some(readiness));
    fixture.quit.store(false, Ordering::Release);
    let quit = fixture.quit.clone();
    let peer_hub = fixture.hub.clone();
    let secret = fixture.secret.clone();
    let peer_instance = instance.clone();
    let controls = fixture.desired.controls.clone();
    fixture.peer = Some(std::thread::spawn(move || {
        peer_hub.handle_wire(
            json!({"type":"auth","token":secret}),
            "returning-peer",
            peer_hub.now_ms(),
        );
        let mut opened = None;
        let mut sequence = 0u64;
        while !quit.load(Ordering::Acquire) {
            if opened.is_none() {
                let reply = peer_hub.handle_wire(
                    json!({"type":"open","instanceId":peer_instance}),
                    "returning-peer",
                    peer_hub.now_ms(),
                );
                if reply["ok"] == true {
                    let baseline=peer_hub.handle_wire(json!({"version":1,"type":"baseline","clientId":reply["clientId"],"sessionId":reply["sessionId"],"generation":reply["generation"],"publicationHash":reply["publicationHash"],"sequence":"0",
                        "values":controls.iter().map(|control|(control.id.clone(),control.fallback())).collect::<BTreeMap<_,_>>()}),"returning-peer",peer_hub.now_ms());
                    assert_eq!(baseline["ok"], true, "{baseline}");
                    opened = Some(reply);
                }
            } else if let Some(reply) = &opened {
                sequence += 1;
                let heartbeat=peer_hub.handle_wire(json!({"version":1,"type":"heartbeat","clientId":reply["clientId"],"sessionId":reply["sessionId"],"generation":reply["generation"],"publicationHash":reply["publicationHash"],"sequence":sequence.to_string()}),"returning-peer",peer_hub.now_ms());
                if heartbeat["ok"] != true {
                    opened = None;
                    sequence = 0;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }));
    let resources = fixture.resources.clone();
    let quit = fixture.quit.clone();
    fixture.inventory = Some(std::thread::spawn(move || {
        while !quit.load(Ordering::Acquire) {
            resources.refresh_native().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }));
    candidate.stage = None;
    candidate.stage = Some(
        NativeProfileHost::stage(
            &fixture.resources,
            &fixture.games,
            &fixture.companions,
            fixture.controllers.clone(),
            false,
            &candidate,
        )
        .unwrap(),
    );
    coordinator.apply(candidate).unwrap();
    assert_eq!(
        coordinator.status().state,
        crate::profile_session::State::Running
    );
    assert!(coordinator.status().owner.as_ref().unwrap().generation > owner.generation);
    let assignment = fixture
        .hub
        .assignment_snapshot(std::slice::from_ref(&instance))
        .unwrap();
    assert_eq!(
        assignment["assignments"][0]["publicationHash"],
        fixture.desired.publication_hash
    );
    assert_eq!(
        fixture
            .hub
            .command(&json!({"command":"companion_status"}))
            .unwrap()["instances"][0]["profileOwned"],
        true
    );
    coordinator.stop().unwrap();
}
