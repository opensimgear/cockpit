use super::*;
use osg_profiles::{
    compiler_models::{SinkAdapter, SinkPlan, SourcePlan},
    resources::{IdentityEvidence, PortExpectation},
    resources_ports::{ActionSemantics, PortContract, PortDescriptor, PortUpdate, ScalarSchema},
    store::LocalAssignment,
    Direction, Endpoint, PortValue,
};
fn metadata(game: &str) -> Observation {
    Observation {
        key: game.into(),
        name: game.into(),
        resource_type: ResourceType::GameIntegration,
        evidence: vec![IdentityEvidence {
            kind: EvidenceKind::GameScope,
            value: game.into(),
        }],
        attributes: attributes([
            ("gameId", game.into()),
            ("providerSetupRevision", "setup-1".into()),
            ("catalogueRevision", "catalogue-1".into()),
            ("gameContextStatus", "waiting-profile".into()),
            ("processVerified", "true".into()),
        ]),
        diagnostics: vec![],
        route: Some(Route {
            id: format!("game:{game}"),
            transport: Transport::Integration,
            provider_generation: "setup-1:catalogue-1".into(),
            attributes: BTreeMap::new(),
            baseline_ready: false,
            writable: false,
            priority: 0,
        }),
        ports: Some(
            PortUpdate::complete(vec![
                PortDescriptor {
                    id: "altitude".into(),
                    name: "Altitude".into(),
                    direction: Direction::Output,
                    contract: PortContract::State {
                        schema: ScalarSchema::Integer {
                            min: 0,
                            max: 65535,
                            unit: Some("ft".into()),
                        },
                        freshness_ms: 1000,
                        fallback: Some(PortValue::Number(0.0)),
                    },
                },
                PortDescriptor {
                    id: "command".into(),
                    name: "Command".into(),
                    direction: Direction::Input,
                    contract: PortContract::Action {
                        parameters: vec![],
                        semantics: ActionSemantics::OneShot,
                        endpoint: "command".into(),
                        readable: false,
                        display: None,
                        ownership: None,
                    },
                },
            ])
            .unwrap(),
        ),
    }
}
fn delivery(game: &str, session: &str, revision: u64) -> ProviderBatch {
    let mut item = metadata(game);
    item.attributes.remove("gameContextStatus");
    item.attributes.remove("processVerified");
    item.attributes
        .insert("gamePreparationSession".into(), session.into());
    item.route = Some(Route {
        id: format!("profile-game:{game}"),
        transport: Transport::Integration,
        provider_generation: format!("{session}:sample-1"),
        attributes: item.attributes.clone(),
        baseline_ready: true,
        writable: false,
        priority: 100,
    });
    batch(
        &format!("profile-game:{game}"),
        session,
        revision,
        vec![item],
    )
}
fn runtime() -> Runtime {
    let runtime = Runtime::new(
        None,
        Arc::new(|_| panic!("native cached game delivery must not discover")),
        false,
    );
    Runtime::adopt(
        &runtime.state,
        vec![(
            "game:dcs".into(),
            Ok(batch("game:dcs", "metadata", 1, vec![metadata("dcs")])),
        )],
    );
    runtime
}
fn plan(runtime: &Runtime) -> ExecutionPlan {
    let state = runtime.state.lock().unwrap();
    let details = state
        .registry
        .list(0, 128)
        .unwrap()
        .resources
        .into_iter()
        .find(|r| r.attributes.get("gameId").map(String::as_str) == Some("dcs"))
        .unwrap();
    let observed = state.registry.inspect(&details.id, 0, 128).unwrap();
    let assignment = LocalAssignment {
        resource_id: "game".into(),
        source_id: details.id.clone(),
        ports: observed
            .ports
            .iter()
            .map(|p| PortExpectation {
                profile_port_id: p.descriptor.id.clone(),
                observed_port_id: p.descriptor.id.clone(),
                expected_contract_hash: p.contract_hash.clone(),
            })
            .collect(),
    };
    drop(state);
    let (_, pins) = runtime.snapshot_for_preparation(vec![assignment]).unwrap();
    let mut sources = vec![];
    let mut sinks = vec![];
    for pin in pins {
        let p = pin.resolved().port.clone();
        if p.direction == Direction::Output {
            sources.push(SourcePlan {
                endpoint: Endpoint {
                    node_id: "game".into(),
                    port_id: p.id.clone(),
                },
                inventory_resource_id: Some(details.id.clone()),
                observed_port_id: Some(p.id),
                contract: p.contract,
                token: None,
                route: Some(pin.resolved().route.clone()),
                preparation: Some(pin),
                readiness: PlanReadiness::Ready,
            });
        } else {
            sinks.push(SinkPlan {
                endpoint: Endpoint {
                    node_id: "game".into(),
                    port_id: p.id.clone(),
                },
                adapter: SinkAdapter::SimulatorCommand,
                inventory_resource_id: Some(details.id.clone()),
                observed_port_id: Some(p.id),
                contract: p.contract,
                token: None,
                route: Some(pin.resolved().route.clone()),
                preparation: Some(pin),
                readiness: PlanReadiness::Ready,
            });
        }
    }
    ExecutionPlan {
        program: osg_mapping::program::Program {
            sources: vec![],
            nodes: vec![],
            sinks: vec![],
            controllers: vec![],
        },
        topological_order: vec![],
        trace: vec![],
        sources,
        sinks,
        branches: vec![],
        controllers: vec![],
        visibility: vec![],
        runtime_overrides: vec![],
        companion_assignments: vec![],
        budgets: Default::default(),
    }
}
#[test]
fn fresh_game_overlay_binds_reads_without_granting_commands_or_reusing_outgoing_reader() {
    let runtime = runtime();
    let prepared = plan(&runtime);
    assert!(runtime.bind_delivery_reads(&prepared).is_err());
    runtime
        .publish_profile_game_delivery(delivery("dcs", "reader-one", 1))
        .unwrap();
    let bound = runtime.bind_delivery_reads(&prepared).unwrap();
    let token = bound.sources[0].token.as_ref().unwrap();
    assert_eq!(token.provider_session, "reader-one");
    assert_eq!(token.access, Access::Read);
    assert!(runtime.validate_delivery(token).unwrap().baseline_ready);
    assert!(bound.sinks.iter().all(|s| s.token.is_none()));
    assert_eq!(
        serde_json::to_value(&bound.program).unwrap(),
        serde_json::to_value(&prepared.program).unwrap()
    );
    assert!(prepared.sources[0].token.is_none());
    let next = plan(&runtime);
    assert_eq!(
        next.sources[0]
            .preparation
            .as_ref()
            .unwrap()
            .resolved()
            .scope,
        "game:dcs"
    );
    assert!(next.sources[0]
        .preparation
        .as_ref()
        .unwrap()
        .requires_baseline());
    assert_eq!(
        runtime
            .state
            .lock()
            .unwrap()
            .registry
            .list(0, 128)
            .unwrap()
            .total,
        1
    );
    Runtime::adopt(
        &runtime.state,
        vec![(
            "game:dcs".into(),
            Ok(batch("game:dcs", "metadata", 2, vec![metadata("dcs")])),
        )],
    );
    assert!(runtime.validate_delivery(token).is_ok());
}
#[test]
fn old_reader_withdrawal_cannot_delete_incoming_owner_and_retired_publications_fail() {
    let runtime = runtime();
    let prepared = plan(&runtime);
    runtime
        .publish_profile_game_delivery(delivery("dcs", "old-reader", 1))
        .unwrap();
    let old = runtime.bind_delivery_reads(&prepared).unwrap().sources[0]
        .token
        .clone()
        .unwrap();
    runtime
        .publish_profile_game_delivery(delivery("dcs", "new-reader", 1))
        .unwrap();
    let new = runtime.bind_delivery_reads(&prepared).unwrap().sources[0]
        .token
        .clone()
        .unwrap();
    assert!(runtime.validate_delivery(&old).is_err());
    runtime
        .withdraw_profile_game_delivery("dcs", "old-reader")
        .unwrap();
    assert!(runtime.validate_delivery(&new).is_ok());
    assert!(runtime
        .publish_profile_game_delivery(delivery("dcs", "old-reader", 2))
        .is_err());
    assert!(runtime.validate_delivery(&new).is_ok());
    runtime
        .withdraw_profile_game_delivery("dcs", "new-reader")
        .unwrap();
    assert!(runtime.validate_delivery(&new).is_err());
    assert!(runtime.bind_delivery_reads(&prepared).is_err());
}
#[test]
fn metadata_revision_process_and_catalogue_changes_revoke_overlay_before_token_validation() {
    for field in [
        "providerSetupRevision",
        "catalogueRevision",
        "processVerified",
        "gameContextStatus",
    ] {
        let runtime = runtime();
        let prepared = plan(&runtime);
        runtime
            .publish_profile_game_delivery(delivery("dcs", "reader", 1))
            .unwrap();
        let token = runtime.bind_delivery_reads(&prepared).unwrap().sources[0]
            .token
            .clone()
            .unwrap();
        let mut item = metadata("dcs");
        item.attributes.insert(field.into(), "changed".into());
        Runtime::adopt(
            &runtime.state,
            vec![(
                "game:dcs".into(),
                Ok(batch("game:dcs", "metadata", 2, vec![item])),
            )],
        );
        assert!(runtime.validate_delivery(&token).is_err(), "{field}");
        assert!(runtime
            .publish_profile_game_delivery(delivery("dcs", "reader", 2))
            .is_err());
    }
    for missing in [true, false] {
        let runtime = runtime();
        let prepared = plan(&runtime);
        runtime
            .publish_profile_game_delivery(delivery("dcs", "reader", 1))
            .unwrap();
        let token = runtime.bind_delivery_reads(&prepared).unwrap().sources[0]
            .token
            .clone()
            .unwrap();
        Runtime::adopt(
            &runtime.state,
            vec![(
                "game:dcs".into(),
                if missing {
                    Ok(batch("game:dcs", "metadata", 2, vec![]))
                } else {
                    Err("process inspection failed".into())
                },
            )],
        );
        assert!(runtime.validate_delivery(&token).is_err());
    }
}
#[test]
fn actual_loss_update_revokes_readiness_without_fabricating_an_empty_baseline() {
    let runtime = runtime();
    let prepared = plan(&runtime);
    runtime
        .publish_profile_game_delivery(delivery("dcs", "reader", 1))
        .unwrap();
    let token = runtime.bind_delivery_reads(&prepared).unwrap().sources[0]
        .token
        .clone()
        .unwrap();
    let mut loss = delivery("dcs", "reader", 2);
    let route = loss.observations[0].route.as_mut().unwrap();
    route.baseline_ready = false;
    route.writable = false;
    runtime.publish_profile_game_delivery(loss).unwrap();
    assert!(runtime.validate_delivery(&token).is_err());
    assert!(runtime.bind_delivery_reads(&prepared).is_err());
    let mut ready = delivery("dcs", "reader", 3);
    ready.observations[0].route.as_mut().unwrap().writable = true;
    runtime.publish_profile_game_delivery(ready).unwrap();
    let bound = runtime.bind_delivery_reads(&prepared).unwrap();
    assert!(bound.sinks[0].token.is_none());
}
#[test]
fn malformed_current_owner_publication_fails_closed_but_foreign_input_cannot_revoke_owner() {
    let runtime = runtime();
    let prepared = plan(&runtime);
    runtime
        .publish_profile_game_delivery(delivery("dcs", "reader", 1))
        .unwrap();
    let token = runtime.bind_delivery_reads(&prepared).unwrap().sources[0]
        .token
        .clone()
        .unwrap();
    let mut malformed = delivery("dcs", "foreign", 1);
    malformed.observations[0]
        .attributes
        .insert("catalogueRevision".into(), "wrong".into());
    assert!(runtime.publish_profile_game_delivery(malformed).is_err());
    assert!(runtime.validate_delivery(&token).is_ok());
    let mut malformed = delivery("dcs", "reader", 2);
    malformed.observations[0].ports.as_mut().unwrap().ports[0].contract = PortContract::Event {
        schema: ScalarSchema::Boolean {},
    };
    assert!(runtime.publish_profile_game_delivery(malformed).is_err());
    assert!(runtime.validate_delivery(&token).is_err());
    assert!(runtime
        .publish_profile_game_delivery(delivery("other", "reader", 2))
        .is_err());
}
#[test]
fn publication_rejects_wrong_scopes_identity_routes_and_invented_ports() {
    let metadata = metadata("dcs");
    for case in 0..12 {
        let mut bad = delivery("dcs", "reader", 1);
        match case {
            0 => bad.scope = "game:dcs".into(),
            1 => bad.complete = false,
            2 => bad.observations.clear(),
            3 => bad.observations[0].resource_type = ResourceType::OsgModule,
            4 => bad.observations[0]
                .attributes
                .remove("gamePreparationSession")
                .map(|_| ())
                .unwrap(),
            5 => bad.observations[0].route = None,
            6 => bad.observations[0].route.as_mut().unwrap().priority = 50,
            7 => bad.observations[0]
                .route
                .as_mut()
                .unwrap()
                .attributes
                .clear(),
            8 => {
                let r = bad.observations[0].route.as_mut().unwrap();
                r.baseline_ready = false;
                r.writable = true;
            }
            9 => bad.observations[0].ports = None,
            10 => {
                let p = &mut bad.observations[0].ports.as_mut().unwrap().ports[0];
                p.id = "invented".into();
            }
            _ => bad.observations[0].evidence.clear(),
        }
        assert!(
            validate_game_delivery(&bad, &metadata).is_err(),
            "case {case}"
        );
    }
    assert!(validate_game_delivery(&delivery("dcs", "reader", 0), &metadata).is_err());
}
#[test]
fn cached_read_binding_rejects_read_only_stop_missing_pins_or_changed_identity() {
    let runtime = runtime();
    let prepared = plan(&runtime);
    runtime
        .publish_profile_game_delivery(delivery("dcs", "reader", 1))
        .unwrap();
    for case in 0..4 {
        let mut bad = prepared.clone();
        match case {
            0 => bad.sources[0].preparation = None,
            1 => bad.sources[0].inventory_resource_id = None,
            2 => bad.sources[0].observed_port_id = None,
            _ => bad.sources[0].preparation = bad.sinks[0].preparation.clone(),
        };
        assert!(runtime.bind_delivery_reads(&bad).is_err());
    }
    let mut dormant = prepared.clone();
    dormant.sources[0].readiness = PlanReadiness::Dormant;
    assert!(runtime.bind_delivery_reads(&dormant).unwrap().sources[0]
        .token
        .is_none());
    runtime.state.lock().unwrap().read_only = true;
    assert!(runtime
        .publish_profile_game_delivery(delivery("dcs", "reader", 2))
        .is_err());
    assert!(runtime.bind_delivery_reads(&prepared).is_err());
    runtime.state.lock().unwrap().read_only = false;
    runtime.shutdown();
    assert!(runtime
        .publish_profile_game_delivery(delivery("dcs", "reader", 2))
        .is_err());
    assert!(runtime.bind_delivery_reads(&prepared).is_err());
    assert!(runtime
        .withdraw_profile_game_delivery("dcs", "reader")
        .is_err());
}

#[test]
fn hardware_baseline_binding_keeps_exact_route_session_catalogue_and_contract() {
    let runtime = runtime();
    let mut prepared = plan(&runtime);
    let mut hardware = metadata("hardware");
    hardware.resource_type = ResourceType::OsgModule;
    hardware.evidence = vec![IdentityEvidence {
        kind: EvidenceKind::VerifiedPhysical,
        value: "osg-mcu:00112233445566778899aabbccddeeff".into(),
    }];
    hardware.attributes.clear();
    hardware.route = Some(Route {
        id: "usb:hardware".into(),
        transport: Transport::Usb,
        provider_generation: "connection:boot:config".into(),
        attributes: attributes([("path", "COM42".into())]),
        baseline_ready: false,
        writable: false,
        priority: 100,
    });
    hardware.ports = Some(
        PortUpdate::complete(vec![hardware.ports.as_ref().unwrap().ports[0].clone()]).unwrap(),
    );
    Runtime::adopt(
        &runtime.state,
        vec![(
            "usb".into(),
            Ok(batch("usb", "usb-session", 1, vec![hardware.clone()])),
        )],
    );
    let pin = {
        let state = runtime.state.lock().unwrap();
        let id = state
            .registry
            .list(0, 128)
            .unwrap()
            .resources
            .iter()
            .find(|r| r.resource_type == ResourceType::OsgModule)
            .unwrap()
            .id
            .clone();
        let port = &hardware.ports.as_ref().unwrap().ports[0];
        state
            .registry
            .prepare_port(&id, &port.id, &port.contract_hash().unwrap(), Access::Read)
            .unwrap()
    };
    prepared.sources[0].inventory_resource_id = Some(pin.resolved().resource_id.clone());
    prepared.sources[0].observed_port_id = Some(pin.resolved().port.id.clone());
    prepared.sources[0].route = Some(pin.resolved().route.clone());
    prepared.sources[0].preparation = Some(pin);
    assert!(runtime.bind_delivery_reads(&prepared).is_err());
    hardware.route.as_mut().unwrap().baseline_ready = true;
    Runtime::adopt(
        &runtime.state,
        vec![(
            "usb".into(),
            Ok(batch("usb", "usb-session", 2, vec![hardware.clone()])),
        )],
    );
    let bound = runtime.bind_delivery_reads(&prepared).unwrap();
    assert_eq!(
        bound.sources[0].token.as_ref().unwrap().provider_session,
        "usb-session"
    );
    let other = Runtime::new(None, Arc::new(|_| vec![]), false);
    assert!(other.bind_delivery_reads(&prepared).is_err());
    Runtime::adopt(
        &runtime.state,
        vec![(
            "usb".into(),
            Ok(batch("usb", "new-usb-session", 1, vec![hardware.clone()])),
        )],
    );
    assert!(runtime.bind_delivery_reads(&prepared).is_err());
    let old_pin = prepared.sources[0].preparation.clone().unwrap();
    let fresh_pin = {
        let state = runtime.state.lock().unwrap();
        state
            .registry
            .prepare_port(
                &old_pin.resolved().resource_id,
                "altitude",
                &old_pin.resolved().port.contract_hash().unwrap(),
                Access::Read,
            )
            .unwrap()
    };
    prepared.sources[0].preparation = Some(fresh_pin);
    hardware.route.as_mut().unwrap().provider_generation = "new-connection:boot:config".into();
    Runtime::adopt(
        &runtime.state,
        vec![(
            "usb".into(),
            Ok(batch("usb", "new-usb-session", 2, vec![hardware.clone()])),
        )],
    );
    assert!(runtime.bind_delivery_reads(&prepared).is_err());
}
#[test]
fn metadata_failures_are_isolated_to_the_matching_game_and_scope_envelopes_are_strict() {
    let runtime = runtime();
    let prepared = plan(&runtime);
    runtime
        .publish_profile_game_delivery(delivery("dcs", "reader", 1))
        .unwrap();
    let token = runtime.bind_delivery_reads(&prepared).unwrap().sources[0]
        .token
        .clone()
        .unwrap();
    Runtime::adopt(
        &runtime.state,
        vec![("game:x-plane".into(), Err("closed".into()))],
    );
    assert!(runtime.validate_delivery(&token).is_ok());
    Runtime::adopt(
        &runtime.state,
        vec![(
            "game:dcs".into(),
            Ok(batch("wrong", "metadata", 2, vec![metadata("dcs")])),
        )],
    );
    assert!(runtime.validate_delivery(&token).is_err());
    let mut bad = metadata("dcs");
    bad.ports = None;
    assert!(!game_metadata_valid(&bad, "dcs"));
    bad = metadata("dcs");
    bad.route = None;
    assert!(!game_metadata_valid(&bad, "dcs"));
    bad = metadata("dcs");
    bad.evidence.clear();
    assert!(!game_metadata_valid(&bad, "dcs"));
}
