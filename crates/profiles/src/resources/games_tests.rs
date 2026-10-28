use super::*;
use crate::resources_ports::{PortContract, PortDescriptor, PortUpdate, ScalarSchema};

fn game_observation(key: &str, game_id: &str, integration: Option<&str>) -> Observation {
    let scope = format!("game:{key}");
    let mut attributes = BTreeMap::from([("gameId".into(), game_id.into())]);
    if let Some(integration) = integration {
        attributes.insert("integrationKey".into(), integration.into());
    }
    if let Some(module) = key.strip_prefix("dcs:") {
        attributes.insert("aircraftModule".into(), module.into());
    }
    Observation {
        key: key.into(),
        name: key.into(),
        resource_type: ResourceType::GameIntegration,
        evidence: vec![IdentityEvidence {
            kind: EvidenceKind::GameScope,
            value: key.into(),
        }],
        attributes,
        diagnostics: vec![],
        route: Some(Route {
            id: scope,
            transport: Transport::Integration,
            provider_generation: "setup:catalogue".into(),
            attributes: BTreeMap::new(),
            baseline_ready: true,
            writable: false,
            priority: 0,
        }),
        ports: Some(
            PortUpdate::complete(vec![PortDescriptor {
                id: "value".into(),
                name: "Value".into(),
                direction: crate::Direction::Output,
                contract: PortContract::State {
                    schema: ScalarSchema::Boolean {},
                    freshness_ms: 1000,
                    fallback: None,
                },
            }])
            .unwrap(),
        ),
    }
}

fn physical_observation() -> Observation {
    Observation {
        key: "controller-one".into(),
        name: "Controller".into(),
        resource_type: ResourceType::OsgModule,
        evidence: vec![IdentityEvidence {
            kind: EvidenceKind::VerifiedPhysical,
            value: "fixture-serial".into(),
        }],
        attributes: BTreeMap::new(),
        diagnostics: vec![],
        route: Some(Route {
            id: "usb:controller-one".into(),
            transport: Transport::Usb,
            provider_generation: "boot-1".into(),
            attributes: BTreeMap::new(),
            baseline_ready: true,
            writable: true,
            priority: 10,
        }),
        ports: Some(
            PortUpdate::complete(vec![PortDescriptor {
                id: "button:0".into(),
                name: "Button".into(),
                direction: crate::Direction::Output,
                contract: PortContract::State {
                    schema: ScalarSchema::Boolean {},
                    freshness_ms: 1000,
                    fallback: None,
                },
            }])
            .unwrap(),
        ),
    }
}

fn companion_observation() -> Observation {
    let mut item = physical_observation();
    item.key = "paired:one".into();
    item.name = "Paired companion".into();
    item.resource_type = ResourceType::Companion;
    item.evidence = vec![IdentityEvidence {
        kind: EvidenceKind::CompanionInstance,
        value: "paired:one".into(),
    }];
    item.route.as_mut().unwrap().transport = Transport::Network;
    item
}

fn observe(registry: &mut Registry, scope: &str, revision: u64, item: Observation) {
    registry
        .reconcile(ProviderBatch {
            scope: scope.into(),
            session: "game-retention-test".into(),
            revision,
            complete: true,
            observations: vec![item],
        })
        .unwrap();
}

fn seed(registry: &mut Registry) -> Vec<Observation> {
    let items = vec![
        game_observation("diablo", "diablo", None),
        game_observation("diablo:Vehicle", "diablo", None),
        game_observation("dcs:CommonData", "dcs", Some("dcs")),
        game_observation("dcs:A-10C", "dcs", Some("dcs")),
    ];
    for item in &items {
        observe(registry, &format!("game:{}", item.key), 1, item.clone());
    }
    observe(registry, "host-controllers", 1, physical_observation());
    observe(registry, "companions", 1, companion_observation());
    items
}

fn named_id(registry: &Registry, name: &str) -> String {
    registry
        .list(0, 32)
        .unwrap()
        .resources
        .into_iter()
        .find(|resource| resource.name == name)
        .unwrap()
        .id
}

#[test]
fn retention_after_restore_removes_unsupported_game_and_revokes_authority() {
    let mut initial = Registry::new();
    let items = seed(&mut initial);
    let diablo_id = named_id(&initial, "diablo");
    let diablo_module_id = named_id(&initial, "diablo:Vehicle");
    let dcs_base = named_id(&initial, "dcs:CommonData");
    let aircraft = named_id(&initial, "dcs:A-10C");
    let mut offline = Registry::restore(initial.saved()).unwrap();
    assert!(offline
        .retain_game_integrations(&BTreeSet::from(["dcs".into()]))
        .unwrap());
    assert_eq!(
        offline
            .inspect(&dcs_base, 0, 1)
            .unwrap()
            .resource
            .availability,
        Availability::Offline
    );
    assert_eq!(
        offline
            .inspect(&aircraft, 0, 1)
            .unwrap()
            .resource
            .availability,
        Availability::Offline
    );
    assert!(offline.inspect(&diablo_id, 0, 1).is_err());
    assert!(offline.inspect(&diablo_module_id, 0, 1).is_err());
    assert_eq!(
        Registry::restore(offline.saved())
            .unwrap()
            .list(0, 32)
            .unwrap()
            .total,
        4
    );

    let mut saved = initial.saved();
    let alias = uuid::Uuid::new_v4().to_string();
    saved.redirects.insert(alias.clone(), diablo_id.clone());
    let mut registry = Registry::restore(saved).unwrap();

    for item in &items {
        observe(
            &mut registry,
            &format!("game:{}", item.key),
            1,
            item.clone(),
        );
    }
    for game_id in ["diablo", "dcs"] {
        registry
            .reconcile(ProviderBatch {
                scope: format!("profile-game:{game_id}"),
                session: "game-retention-test".into(),
                revision: 1,
                complete: true,
                observations: vec![],
            })
            .unwrap();
    }
    observe(&mut registry, "host-controllers", 1, physical_observation());
    observe(&mut registry, "companions", 1, companion_observation());
    let token = registry.token(&diablo_id, "value", Access::Read).unwrap();
    let hash = registry.inspect(&diablo_id, 0, 1).unwrap().ports[0]
        .contract_hash
        .clone();
    let preparation = registry
        .prepare_game_metadata_port(&diablo_id, "value", &hash, Access::Read, "diablo")
        .unwrap();
    registry.provider_failed("profile-game:diablo", "Removed game route");
    registry.provider_failed("profile-game:diablo:Vehicle", "Removed aircraft route");
    registry.provider_failed("unrelated:device", "Unrelated failure");
    let revision = registry.revision();

    assert!(registry
        .retain_game_integrations(&BTreeSet::from(["dcs".into()]))
        .unwrap());
    assert_eq!(
        registry.revision().parse::<u64>().unwrap(),
        revision.parse::<u64>().unwrap() + 1
    );
    assert!(registry.inspect(&diablo_id, 0, 1).is_err());
    assert!(registry.inspect(&diablo_module_id, 0, 1).is_err());
    assert!(registry.inspect(&alias, 0, 1).is_err());
    assert!(registry
        .validate_token(&token, "value", Access::Read)
        .is_err());
    assert!(registry.validate_preparation(&preparation).is_err());
    assert!(registry
        .prepare_game_metadata_port(&diablo_id, "value", &hash, Access::Read, "diablo")
        .is_err());
    assert!(registry.inspect(&dcs_base, 0, 1).is_ok());
    assert!(registry.inspect(&aircraft, 0, 1).is_ok());
    assert!(registry
        .inspect(&named_id(&registry, "Controller"), 0, 1)
        .is_ok());
    assert!(registry
        .inspect(&named_id(&registry, "Paired companion"), 0, 1)
        .is_ok());
    assert!(registry.saved().redirects.is_empty());
    assert!(!registry.scopes.contains_key("game:diablo"));
    assert!(!registry.scopes.contains_key("profile-game:diablo"));
    assert!(registry.scopes.contains_key("game:dcs:CommonData"));
    assert!(registry.scopes.contains_key("profile-game:dcs"));
    assert!(!registry.failures.contains_key("profile-game:diablo"));
    assert!(!registry
        .failures
        .contains_key("profile-game:diablo:Vehicle"));
    assert_eq!(
        registry.failures.get("unrelated:device").unwrap(),
        "Unrelated failure"
    );
    registry.saved().validate().unwrap();

    let saved = registry.saved();
    assert!(!serde_json::to_string(&saved).unwrap().contains("diablo"));
    let restored = Registry::restore(saved.clone()).unwrap();
    assert_eq!(restored.saved().resources.len(), 4);
    assert_eq!(restored.saved().redirects, saved.redirects);
    let revision = registry.revision();
    let retained_dcs_entry = registry.entries[&dcs_base].clone();
    assert!(!registry
        .retain_game_integrations(&BTreeSet::from(["dcs".into()]))
        .unwrap());
    assert_eq!(registry.revision(), revision);
    assert!(Arc::ptr_eq(
        &retained_dcs_entry,
        &registry.entries[&dcs_base]
    ));
}

#[test]
fn empty_authoritative_catalogue_removes_only_game_resources() {
    let mut registry = Registry::new();
    seed(&mut registry);
    let before = registry.revision();
    assert!(registry.retain_game_integrations(&BTreeSet::new()).unwrap());
    let page = registry.list(0, 32).unwrap();
    assert_eq!(page.total, 2);
    assert!(page.resources.iter().all(|resource| {
        matches!(
            resource.resource_type,
            ResourceType::OsgModule | ResourceType::Companion
        )
    }));
    assert_eq!(
        registry.revision().parse::<u64>().unwrap(),
        before.parse::<u64>().unwrap() + 1
    );
    assert!(!registry.retain_game_integrations(&BTreeSet::new()).unwrap());
    registry.saved().validate().unwrap();
}

#[test]
fn scope_only_retirement_clears_game_sessions_and_failures_for_readdition() {
    let mut registry = Registry::new();
    for scope in ["game:diablo", "profile-game:diablo"] {
        registry
            .reconcile(ProviderBatch {
                scope: scope.into(),
                session: "previous-session".into(),
                revision: 5,
                complete: true,
                observations: vec![],
            })
            .unwrap();
    }
    registry.provider_failed("profile-game:diablo", "stale delivery failure");
    let revision = registry.revision();
    assert!(registry.retain_game_integrations(&BTreeSet::new()).unwrap());
    assert_eq!(
        registry.revision().parse::<u64>().unwrap(),
        revision.parse::<u64>().unwrap() + 1
    );
    assert!(registry.scopes.is_empty());
    assert!(registry.failures.is_empty());
    assert!(!registry.retain_game_integrations(&BTreeSet::new()).unwrap());

    for scope in ["game:diablo", "profile-game:diablo"] {
        registry
            .reconcile(ProviderBatch {
                scope: scope.into(),
                session: "previous-session".into(),
                revision: 1,
                complete: true,
                observations: vec![],
            })
            .unwrap();
    }
    assert_eq!(registry.scopes.len(), 2);
}

#[test]
fn retiring_profile_delivery_removes_overlay_ports_but_keeps_base_catalogue_offline() {
    let mut registry = Registry::new();
    let base = game_observation("dcs", "dcs", Some("dcs"));
    observe(&mut registry, "game:dcs", 1, base);

    let mut delivery = game_observation("dcs", "dcs", Some("dcs"));
    delivery.route.as_mut().unwrap().id = "profile-game:dcs".into();
    delivery.ports = Some(
        PortUpdate::complete(vec![
            PortDescriptor {
                id: "value".into(),
                name: "Value".into(),
                direction: crate::Direction::Output,
                contract: PortContract::State {
                    schema: ScalarSchema::Boolean {},
                    freshness_ms: 1000,
                    fallback: None,
                },
            },
            PortDescriptor {
                id: "legacy-overlay".into(),
                name: "Legacy overlay".into(),
                direction: crate::Direction::Output,
                contract: PortContract::State {
                    schema: ScalarSchema::Boolean {},
                    freshness_ms: 1000,
                    fallback: None,
                },
            },
        ])
        .unwrap(),
    );
    observe(&mut registry, "profile-game:dcs", 1, delivery);

    let id = named_id(&registry, "dcs");
    let mut restarted = Registry::restore(registry.saved()).unwrap();
    let restarted_id = named_id(&restarted, "dcs");
    assert_eq!(restarted_id, id);
    assert!(restarted
        .inspect(&restarted_id, 0, 8)
        .unwrap()
        .ports
        .iter()
        .any(|port| port.descriptor.id == "legacy-overlay"));
    assert!(restarted.retire_restored_game_deliveries().unwrap());
    assert!(restarted
        .inspect(&restarted_id, 0, 8)
        .unwrap()
        .ports
        .iter()
        .all(|port| port.descriptor.id != "legacy-overlay"));
    assert!(restarted
        .inspect(&restarted_id, 0, 8)
        .unwrap()
        .ports
        .iter()
        .any(|port| port.descriptor.id == "value"));
    assert!(!restarted.retire_restored_game_deliveries().unwrap());

    let details = registry.inspect(&id, 0, 8).unwrap();
    let legacy = details
        .ports
        .iter()
        .find(|port| port.descriptor.id == "legacy-overlay")
        .unwrap();
    let token = registry.token(&id, "legacy-overlay", Access::Read).unwrap();
    let preparation = registry
        .prepare_port(&id, "legacy-overlay", &legacy.contract_hash, Access::Read)
        .unwrap();
    assert!(registry
        .validate_token(&token, "legacy-overlay", Access::Read)
        .is_ok());
    assert!(registry.validate_preparation(&preparation).is_ok());

    registry
        .failures
        .insert("profile-game:dcs".into(), "stopped".into());
    let revision = registry.revision();
    assert!(registry.retire_game_delivery("profile-game:dcs").unwrap());
    assert_eq!(
        registry.revision().parse::<u64>().unwrap(),
        revision.parse::<u64>().unwrap() + 1
    );
    assert!(registry
        .validate_token(&token, "legacy-overlay", Access::Read)
        .is_err());
    assert!(registry.validate_preparation(&preparation).is_err());
    assert!(registry
        .inspect(&id, 0, 8)
        .unwrap()
        .ports
        .iter()
        .all(|port| port.descriptor.id != "legacy-overlay"));
    assert!(!registry.scopes.contains_key("profile-game:dcs"));
    assert!(!registry.failures.contains_key("profile-game:dcs"));
    assert!(registry.scopes.contains_key("game:dcs"));

    let mut offline_base = game_observation("dcs", "dcs", Some("dcs"));
    offline_base.route = None;
    offline_base.ports = None;
    observe(&mut registry, "game:dcs", 2, offline_base);
    let offline = registry.inspect(&id, 0, 8).unwrap();
    assert_eq!(offline.resource.availability, Availability::Offline);
    assert_eq!(offline.ports.len(), 1);
    assert_eq!(offline.ports[0].descriptor.id, "value");
    let saved = registry.saved();
    assert!(!serde_json::to_string(&saved)
        .unwrap()
        .contains("legacy-overlay"));
    assert_eq!(
        Registry::restore(saved).unwrap().list(0, 8).unwrap().total,
        1
    );
    let revision = registry.revision();
    let retained_entry = registry.entries[&id].clone();
    assert!(!registry.retire_game_delivery("profile-game:dcs").unwrap());
    assert_eq!(registry.revision(), revision);
    assert!(Arc::ptr_eq(&retained_entry, &registry.entries[&id]));
}
