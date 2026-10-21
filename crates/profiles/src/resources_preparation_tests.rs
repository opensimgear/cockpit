use super::*;
use crate::resources_ports::*;
use crate::{
    compiler::{compile, compile_for_preparation},
    compiler_models::*,
    store::LocalAssignment,
    Direction, Port, PortValue, ProfileDocument, Resource, ResourceKind,
};
fn observation(game: bool, baseline: bool, writable: bool) -> Observation {
    Observation {
        key: "panel-one".into(),
        name: "Panel".into(),
        resource_type: if game {
            ResourceType::GameIntegration
        } else {
            ResourceType::OsgModule
        },
        evidence: vec![],
        attributes: BTreeMap::new(),
        diagnostics: vec![],
        route: Some(Route {
            id: "route-one".into(),
            transport: if game {
                Transport::Integration
            } else {
                Transport::Usb
            },
            provider_generation: "boot-1".into(),
            attributes: BTreeMap::new(),
            baseline_ready: baseline,
            writable,
            priority: 1,
        }),
        ports: Some(
            PortUpdate::complete(vec![
                PortDescriptor {
                    id: "button:0".into(),
                    name: "Pressed".into(),
                    direction: Direction::Output,
                    contract: PortContract::State {
                        schema: ScalarSchema::Boolean {},
                        freshness_ms: 1000,
                        fallback: Some(PortValue::Boolean(false)),
                    },
                },
                PortDescriptor {
                    id: "led.set".into(),
                    name: "LED".into(),
                    direction: Direction::Input,
                    contract: PortContract::Action {
                        endpoint: "led".into(),
                        parameters: vec![ActionParameter {
                            id: "on".into(),
                            name: "On".into(),
                            schema: ScalarSchema::Boolean {},
                        }],
                        semantics: ActionSemantics::Setter,
                        readable: true,
                        display: None,
                        ownership: None,
                    },
                },
            ])
            .unwrap(),
        ),
    }
}
fn batch(revision: u64, item: Option<Observation>) -> ProviderBatch {
    ProviderBatch {
        scope: "provider".into(),
        session: "provider-session".into(),
        revision,
        complete: true,
        observations: item.into_iter().collect(),
    }
}
fn seeded_registry(game: bool, baseline: bool, writable: bool) -> Registry {
    let mut registry = Registry::new();
    registry
        .reconcile(batch(1, Some(observation(game, baseline, writable))))
        .unwrap();
    registry
}
fn capture(
    registry: &Registry,
    game: bool,
) -> (ProfileDocument, CompileSnapshot, Vec<PortPreparation>) {
    let id = registry.list(0, 1).unwrap().resources[0].id.clone();
    let details = registry.inspect(&id, 0, 128).unwrap();
    let mut document = crate::new_document("Preparation").unwrap();
    document.resources.push(Resource {
        id: "panel".into(),
        name: "Panel".into(),
        kind: if game {
            ResourceKind::Game
        } else {
            ResourceKind::Device
        },
        required: true,
        ports: details
            .ports
            .iter()
            .map(|p| Port {
                id: if p.descriptor.direction == Direction::Output {
                    "pressed"
                } else {
                    "led"
                }
                .into(),
                name: p.descriptor.name.clone(),
                direction: p.descriptor.direction,
                contract: p.descriptor.contract.clone(),
            })
            .collect(),
    });
    let parameters = match &document.resources[0].ports[1].contract {
        PortContract::Action { parameters, .. } => parameters.clone(),
        _ => unreachable!(),
    };
    document.processing_nodes.push(crate::ProcessingNode { draft: None,
        id: "setter".into(),
        name: "Setter".into(),
        operation: osg_mapping::program::Operation::SetOnChange {
            arguments: vec!["on".into()],
        },
        inputs: vec![Port {
            id: "on".into(),
            name: "On".into(),
            direction: Direction::Input,
            contract: document.resources[0].ports[0].contract.clone(),
        }],
        outputs: vec![Port {
            id: "request".into(),
            name: "Request".into(),
            direction: Direction::Output,
            contract: PortContract::Invocation {
                parameters,
                semantics: ActionSemantics::Setter,
            },
        }],
    });
    document.bindings = vec![
        crate::Binding {
            id: "source-edge".into(),
            from: crate::Endpoint {
                node_id: "panel".into(),
                port_id: "pressed".into(),
            },
            to: crate::Endpoint {
                node_id: "setter".into(),
                port_id: "on".into(),
            },
        },
        crate::Binding {
            id: "sink-edge".into(),
            from: crate::Endpoint {
                node_id: "setter".into(),
                port_id: "request".into(),
            },
            to: crate::Endpoint {
                node_id: "panel".into(),
                port_id: "led".into(),
            },
        },
    ];
    let expectations = details
        .ports
        .iter()
        .map(|p| PortExpectation {
            profile_port_id: if p.descriptor.direction == Direction::Output {
                "pressed"
            } else {
                "led"
            }
            .into(),
            observed_port_id: p.descriptor.id.clone(),
            expected_contract_hash: p.contract_hash.clone(),
        })
        .collect();
    let tokens = details
        .ports
        .iter()
        .filter_map(|p| {
            registry
                .token(
                    &id,
                    &p.descriptor.id,
                    if p.descriptor.direction == Direction::Output {
                        Access::Read
                    } else {
                        Access::Invoke
                    },
                )
                .ok()
        })
        .collect();
    let preparations = details
        .ports
        .iter()
        .filter_map(|p| {
            registry
                .prepare_port(
                    &id,
                    &p.descriptor.id,
                    &p.contract_hash,
                    if p.descriptor.direction == Direction::Output {
                        Access::Read
                    } else {
                        Access::Invoke
                    },
                )
                .ok()
        })
        .collect();
    let snapshot = CompileSnapshot {
        inventory_session: details.session,
        inventory_revision: details.revision,
        assignments: vec![LocalAssignment {
            resource_id: "panel".into(),
            source_id: id,
            ports: expectations,
        }],
        resources: vec![CapturedResource {
            summary: details.resource,
            ports: details.ports,
            tokens,
            provider_setup_revision: game.then(|| format!("sha256:{}", "a".repeat(64))),
            game_id: game.then(|| "dcs".into()),
        }],
        game_context: None,
        backend: BackendCapabilities::default(),
        inventory_diagnostics: vec![],
        read_only: false,
    };
    (document, snapshot, preparations)
}
#[test]
fn acquisition_pin_never_grants_delivery_and_all_live_fences_still_apply() {
    let mut registry = seeded_registry(false, false, true);
    let (document, snapshot, pins) = capture(&registry, false);
    let id = &snapshot.assignments[0].source_id;
    let pin = pins
        .iter()
        .find(|pin| pin.access() == Access::Read)
        .unwrap();
    assert!(pin.requires_baseline());
    assert!(!pin.requires_provider_acquisition());
    registry.validate_preparation(pin).unwrap();
    assert!(registry.token(id, "button:0", Access::Read).is_err());
    assert!(registry
        .prepare_port(id, "button:0", "sha256:bad", Access::Read)
        .is_err());
    assert_eq!(
        compile(&document, &snapshot).readiness,
        PlanReadiness::Blocked
    );
    let prepared = compile_for_preparation(&document, &snapshot, &pins);
    assert_eq!(
        prepared.program_hash,
        compile(&document, &snapshot).program_hash
    );
    assert_eq!(prepared.readiness, PlanReadiness::Ready);
    let plan = prepared.plan.unwrap();
    assert!(plan
        .sources
        .iter()
        .all(|source| source.token.is_none() && source.preparation.is_some()));
    assert!(plan
        .sinks
        .iter()
        .all(|sink| sink.token.is_none() && sink.preparation.is_some()));
    assert!(!serde_json::to_string(&plan)
        .unwrap()
        .contains("preparation"));
    registry
        .reconcile(batch(2, Some(observation(false, true, true))))
        .unwrap();
    assert!(registry.validate_preparation(pin).is_err());
    let delivery = registry.token(id, "button:0", Access::Read).unwrap();
    registry
        .validate_token(&delivery, "button:0", Access::Read)
        .unwrap();
    let (_, fresh, pins) = capture(&registry, false);
    let delivery_report = compile(&document, &fresh);
    assert_eq!(
        delivery_report.program_hash,
        compile(&document, &snapshot).program_hash
    );
    assert_eq!(delivery_report.readiness, PlanReadiness::Ready);
    assert!(!pins
        .iter()
        .find(|p| p.access() == Access::Read)
        .unwrap()
        .requires_baseline());
    registry.provider_failed("provider", "disconnected");
    assert!(registry
        .validate_token(&delivery, "button:0", Access::Read)
        .is_err());
    assert!(registry
        .prepare_port(id, "button:0", &delivery.contract_hash, Access::Read)
        .is_err());
}
#[test]
fn preparation_compilation_preserves_graph_storage_capability_and_game_gates() {
    let registry = seeded_registry(false, false, true);
    let (document, snapshot, pins) = capture(&registry, false);
    assert_eq!(
        compile_for_preparation(&document, &snapshot, &[]).readiness,
        PlanReadiness::Blocked
    );
    let mut readonly = snapshot.clone();
    readonly.read_only = true;
    assert_eq!(
        compile_for_preparation(&document, &readonly, &pins).readiness,
        PlanReadiness::Blocked
    );
    let mut changed = snapshot.clone();
    changed.resources[0].ports[0].current = false;
    assert_eq!(
        compile_for_preparation(&document, &changed, &pins).readiness,
        PlanReadiness::Blocked
    );
    let mut malformed = document.clone();
    malformed.bindings[0].to.port_id = "missing".into();
    assert_eq!(
        compile_for_preparation(&malformed, &snapshot, &pins).readiness,
        PlanReadiness::Invalid
    );
    assert_eq!(
        compile_for_preparation(&document, &snapshot, &vec![pins[0].clone(); 1025]).readiness,
        PlanReadiness::Invalid
    );
    let game = seeded_registry(true, false, false);
    let (document, mut snapshot, pins) = capture(&game, true);
    assert!(pins
        .iter()
        .any(PortPreparation::requires_provider_acquisition));
    assert_eq!(
        compile_for_preparation(&document, &snapshot, &pins).readiness,
        PlanReadiness::Dormant
    );
    snapshot.game_context = Some(VerifiedGameContext {
        game_id: "x-plane-12".into(),
        session: "native-context".into(),
        provider_setup_revisions: BTreeMap::from([(
            snapshot.resources[0].summary.id.clone(),
            format!("sha256:{}", "a".repeat(64)),
        )]),
    });
    assert_eq!(
        compile_for_preparation(&document, &snapshot, &pins).readiness,
        PlanReadiness::Dormant
    );
    snapshot.game_context.as_mut().unwrap().game_id = "dcs".into();
    assert_eq!(
        compile_for_preparation(&document, &snapshot, &pins).readiness,
        PlanReadiness::Ready
    );
    assert_eq!(
        compile(&document, &snapshot).readiness,
        PlanReadiness::Blocked
    );
}
#[test]
fn read_only_hardware_ties_restarts_and_retained_catalogues_cannot_prepare_outputs() {
    let mut registry = seeded_registry(false, false, false);
    let (_, snapshot, _) = capture(&registry, false);
    let id = &snapshot.assignments[0].source_id;
    let action = &snapshot.resources[0]
        .ports
        .iter()
        .find(|p| p.descriptor.id == "led.set")
        .unwrap();
    assert!(registry
        .prepare_port(id, "led.set", &action.contract_hash, Access::Invoke)
        .is_err());
    registry.reconcile(batch(2, None)).unwrap();
    assert!(registry
        .prepare_port(id, "led.set", &action.contract_hash, Access::Invoke)
        .is_err());
    let mut restored = Registry::restore(registry.saved()).unwrap();
    assert!(restored
        .prepare_port(
            id,
            "button:0",
            &snapshot.resources[0].ports[0].contract_hash,
            Access::Read
        )
        .is_err());
    let mut item = observation(false, false, true);
    item.ports = None;
    restored.reconcile(batch(1, Some(item))).unwrap();
    assert!(restored
        .prepare_port(id, "led.set", &action.contract_hash, Access::Invoke)
        .is_err());
    let mut first = observation(false, false, true);
    first.evidence = vec![IdentityEvidence {
        kind: EvidenceKind::VerifiedPhysical,
        value: "physical-one".into(),
    }];
    let mut second = first.clone();
    second.key = "panel-two".into();
    second.route.as_mut().unwrap().id = "route-two".into();
    let mut tied = Registry::new();
    tied.reconcile(ProviderBatch {
        scope: "provider".into(),
        session: "p".into(),
        revision: 1,
        complete: true,
        observations: vec![first, second],
    })
    .unwrap();
    let id = &tied.list(0, 1).unwrap().resources[0].id;
    assert!(tied
        .prepare_port(
            id,
            "button:0",
            &snapshot.resources[0].ports[0].contract_hash,
            Access::Read
        )
        .is_err());
}

#[test]
fn game_acquisition_selects_metadata_even_when_an_outgoing_reader_is_live() {
    let mut registry = Registry::new();
    let mut item = observation(true, false, false);
    item.key = "dcs".into();
    item.route.as_mut().unwrap().id = "game:dcs".into();
    item.evidence = vec![IdentityEvidence {
        kind: EvidenceKind::GameScope,
        value: "dcs".into(),
    }];
    registry
        .reconcile(ProviderBatch {
            scope: "game:dcs".into(),
            session: "catalogue".into(),
            revision: 1,
            complete: true,
            observations: vec![item.clone()],
        })
        .unwrap();
    let id = registry.list(0, 1).unwrap().resources[0].id.clone();
    let hash = item.ports.as_ref().unwrap().ports[0]
        .contract_hash()
        .unwrap();
    let mut active = item.clone();
    let route = active.route.as_mut().unwrap();
    route.id = "profile-game:dcs".into();
    route.priority = 100;
    route.baseline_ready = true;
    route.writable = true;
    registry
        .reconcile(ProviderBatch {
            scope: "profile-game:dcs".into(),
            session: "old-profile".into(),
            revision: 1,
            complete: true,
            observations: vec![active],
        })
        .unwrap();
    assert_eq!(
        registry
            .prepare_port(&id, "button:0", &hash, Access::Read)
            .unwrap()
            .resolved()
            .scope,
        "profile-game:dcs"
    );
    let pin = registry
        .prepare_game_metadata_port(&id, "button:0", &hash, Access::Read, "dcs")
        .unwrap();
    assert_eq!(pin.resolved().scope, "game:dcs");
    assert!(pin.requires_baseline());
    assert!(registry.validate_preparation(&pin).is_ok());
    registry.provider_failed("profile-game:dcs", "The outgoing reader stopped.");
    assert!(registry.validate_preparation(&pin).is_ok());
    let mut changed = item.clone();
    changed.route.as_mut().unwrap().provider_generation = "setup-2:catalogue-2".into();
    registry
        .reconcile(ProviderBatch {
            scope: "game:dcs".into(),
            session: "catalogue".into(),
            revision: 2,
            complete: true,
            observations: vec![changed],
        })
        .unwrap();
    assert!(registry.validate_preparation(&pin).is_err());
    assert!(registry
        .prepare_game_metadata_port(&id, "button:0", &hash, Access::Read, "wrong-game")
        .is_err());
    let hardware = seeded_registry(false, true, true);
    let id = hardware.list(0, 1).unwrap().resources[0].id.clone();
    assert!(hardware
        .prepare_game_metadata_port(&id, "button:0", &hash, Access::Read, "dcs")
        .is_err());
}

#[test]
fn aircraft_metadata_preparation_keeps_module_identity_and_cannot_select_another_game() {
    let mut registry=Registry::new();
    for module in ["F-14","F-86F Sabre"] {
        let key=format!("dcs:{module}");let scope=format!("game:{key}");
        let mut item=observation(true,false,false);item.key=key.clone();
        item.evidence=vec![IdentityEvidence{kind:EvidenceKind::GameScope,value:key.clone()}];
        item.route.as_mut().unwrap().id=scope.clone();
        let hash=item.ports.as_ref().unwrap().ports[0].contract_hash().unwrap();
        registry.reconcile(ProviderBatch{scope:scope.clone(),session:"catalogue".into(),revision:1,complete:true,observations:vec![item]}).unwrap();
        let id=registry.list(0,64).unwrap().resources.iter().find(|resource|resource.connections.iter().any(|connection|connection.scope==scope)).unwrap().id.clone();
        let pin=registry.prepare_game_metadata_port(&id,"button:0",&hash,Access::Read,"dcs").unwrap();
        assert_eq!(pin.resolved().scope,scope);assert!(registry.validate_preparation(&pin).is_ok());
        assert!(registry.prepare_game_metadata_port(&id,"button:0",&hash,Access::Read,"x-plane-12").is_err());
    }
    assert_eq!(registry.list(0,64).unwrap().resources.len(),2);
}
