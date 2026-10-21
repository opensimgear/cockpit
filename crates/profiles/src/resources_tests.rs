use super::*;
use crate::resources_ports::*;

fn state(id: &str) -> PortDescriptor {
    PortDescriptor {
        id: id.into(),
        name: "Button".into(),
        direction: crate::Direction::Output,
        contract: PortContract::State {
            schema: ScalarSchema::Boolean {},
            freshness_ms: 1000,
            fallback: Some(PortValue::Boolean(false)),
        },
    }
}
fn action(id: &str) -> PortDescriptor {
    PortDescriptor {
        id: id.into(),
        name: "Set LED".into(),
        direction: crate::Direction::Input,
        contract: PortContract::Action {
            parameters: vec![ActionParameter {
                id: "on".into(),
                name: "On".into(),
                schema: ScalarSchema::Boolean {},
            }],
            semantics: ActionSemantics::Setter,
            endpoint: "led".into(),
            readable: true,
            display: None,
            ownership: None,
        },
    }
}
fn observation(key: &str, proof: Option<&str>, transport: Transport, priority: i32) -> Observation {
    Observation {
        key: key.into(),
        name: "Panel".into(),
        resource_type: ResourceType::OsgModule,
        evidence: proof
            .map(|value| {
                vec![IdentityEvidence {
                    kind: EvidenceKind::VerifiedPhysical,
                    value: value.into(),
                }]
            })
            .unwrap_or_default(),
        attributes: BTreeMap::from([("context".into(), "test".into())]),
        diagnostics: vec![],
        route: Some(Route {
            id: format!("{key}-route"),
            transport,
            provider_generation: "1".into(),
            attributes: BTreeMap::new(),
            baseline_ready: true,
            writable: true,
            priority,
        }),
        ports: Some(PortUpdate::complete(vec![state("button:0"), action("led.set")]).unwrap()),
    }
}
fn batch(scope: &str, revision: u64, observations: Vec<Observation>) -> ProviderBatch {
    ProviderBatch {
        scope: scope.into(),
        session: "provider-1".into(),
        revision,
        complete: true,
        observations,
    }
}
fn only_id(registry: &Registry) -> String {
    registry.list(0, 128).unwrap().resources[0].id.clone()
}

fn paired_companion(key: &str) -> Observation {
    let mut item = observation(key, None, Transport::Network, 0);
    item.name = key.into();
    item.resource_type = ResourceType::Companion;
    item.evidence = vec![IdentityEvidence { kind: EvidenceKind::CompanionInstance, value: key.into() }];
    item
}

#[test]
fn complete_companion_membership_removes_unpaired_cache_after_restart_but_preserves_offline_pairs() {
    let mut registry = Registry::new();
    registry.reconcile(batch("companions", 1, vec![paired_companion("removed"), paired_companion("paired")])).unwrap();
    let ids = registry.list(0,128).unwrap().resources.into_iter().map(|item|(item.name,item.id)).collect::<BTreeMap<_,_>>();
    let mut restored = Registry::restore(registry.saved()).unwrap();
    restored.provider_failed("companions", "Temporary protected store failure");
    assert_eq!(restored.list(0,128).unwrap().total,2);
    let mut partial = batch("companions",1,vec![]);partial.complete=false;
    restored.reconcile(partial).unwrap();
    assert_eq!(restored.list(0,128).unwrap().total,2);
    let mut offline = paired_companion("paired");offline.route=None;offline.ports=None;
    let report = restored.reconcile(batch("companions",2,vec![offline])).unwrap();
    assert!(report.changed.contains(&ids["removed"]));
    assert!(restored.inspect(&ids["removed"],0,128).is_err());
    let retained = restored.inspect(&ids["paired"],0,128).unwrap();
    assert_eq!(retained.resource.availability,Availability::Offline);
    assert_eq!(retained.ports.len(),2);
    let again = Registry::restore(restored.saved()).unwrap();
    assert_eq!(again.list(0,128).unwrap().total,1);
    assert_eq!(only_id(&again),ids["paired"]);
}

#[test]
fn removal_revokes_tokens_cleans_redirects_and_repairing_gets_a_new_identity() {
    let mut registry = Registry::new();
    registry.reconcile(batch("companions",1,vec![paired_companion("panel")])).unwrap();
    let id=only_id(&registry);
    let alias=uuid::Uuid::new_v4().to_string();
    let mut saved=registry.saved();saved.redirects.insert(alias.clone(),id.clone());
    let mut registry=Registry::restore(saved).unwrap();
    registry.reconcile(batch("companions",1,vec![paired_companion("panel")])).unwrap();
    assert_eq!(registry.inspect(&alias,0,128).unwrap().resource.id,id);
    let token=registry.token(&id,"button:0",Access::Read).unwrap();
    registry.reconcile(batch("companions",2,vec![])).unwrap();
    assert!(registry.validate_token(&token,"button:0",Access::Read).is_err());
    assert!(registry.inspect(&alias,0,128).is_err());
    assert!(registry.saved().redirects.is_empty());
    registry.saved().validate().unwrap();
    registry.reconcile(batch("companions",3,vec![paired_companion("panel")])).unwrap();
    assert_ne!(only_id(&registry),id);
    assert!(registry.inspect(&id,0,128).is_err());
}

#[test]
fn authoritative_forget_removes_only_its_sighting_and_usb_unplug_stays_cached() {
    let mut registry=Registry::new();
    let item=observation("module",Some("physical-module"),Transport::Hid,0);
    registry.reconcile(batch("host-controllers",1,vec![item.clone()])).unwrap();
    registry.reconcile(batch("standalone-usb",1,vec![item])).unwrap();
    let id=only_id(&registry);
    registry.reconcile(batch("host-controllers",2,vec![])).unwrap();
    assert_eq!(only_id(&registry),id);
    assert!(registry.saved().resources[0].sightings.iter().all(|s|s.scope=="standalone-usb"));
    registry.reconcile(batch("standalone-usb",2,vec![])).unwrap();
    assert_eq!(registry.inspect(&id,0,128).unwrap().resource.availability,Availability::Offline);
    let mut can=observation("forgotten-can",None,Transport::Can,0);can.name="forgotten-can".into();
    registry.reconcile(batch("gateway:one",1,vec![can])).unwrap();
    registry.reconcile(batch("gateway:one",2,vec![])).unwrap();
    assert_eq!(registry.list(0,128).unwrap().total,1);
}

#[test]
fn persistent_identity_offline_refs_and_baselines_survive_without_replaying() {
    let mut registry = Registry::new();
    registry
        .reconcile(batch(
            "gateway-a",
            1,
            vec![observation("module-a", None, Transport::Can, 10)],
        ))
        .unwrap();
    let id = only_id(&registry);
    let token = registry.token(&id, "button:0", Access::Read).unwrap();
    registry
        .reconcile(batch(
            "gateway-a",
            2,
            vec![observation("module-a", None, Transport::Can, 10)],
        ))
        .unwrap();
    registry
        .validate_token(&token, "button:0", Access::Read)
        .unwrap();
    let saved = registry.saved();
    let mut restored = Registry::restore(saved).unwrap();
    let details = restored.inspect(&id, 0, 128).unwrap();
    assert_eq!(details.resource.availability, Availability::Offline);
    assert_eq!(details.ports.len(), 2);
    assert!(details
        .ports
        .iter()
        .all(|p| !p.current && p.route_ids.is_empty()));
    assert!(restored
        .validate_token(&token, "button:0", Access::Read)
        .is_err());
    let mut reconnect = observation("module-a", None, Transport::Can, 10);
    reconnect.route.as_mut().unwrap().baseline_ready = false;
    restored
        .reconcile(batch("gateway-a", 1, vec![reconnect.clone()]))
        .unwrap();
    assert_eq!(only_id(&restored), id);
    assert!(restored.token(&id, "button:0", Access::Read).is_err());
    reconnect.route.as_mut().unwrap().baseline_ready = true;
    restored
        .reconcile(batch("gateway-a", 2, vec![reconnect]))
        .unwrap();
    assert!(restored.token(&id, "button:0", Access::Read).is_ok());
    let current = restored.token(&id, "button:0", Access::Read).unwrap();
    restored.reconcile(batch("gateway-a", 3, vec![])).unwrap();
    assert!(restored
        .validate_token(&current, "button:0", Access::Read)
        .is_err());
    assert_eq!(
        restored.inspect(&id, 0, 128).unwrap().resource.availability,
        Availability::Offline
    );
}

#[test]
fn equal_module_ids_and_serials_do_not_merge_without_verified_physical_evidence() {
    let mut registry = Registry::new();
    let mut a = observation("same-module", None, Transport::Can, 10);
    a.evidence.push(IdentityEvidence {
        kind: EvidenceKind::Serial,
        value: "same-serial".into(),
    });
    registry
        .reconcile(batch("gateway-a", 1, vec![a.clone()]))
        .unwrap();
    registry.reconcile(batch("gateway-b", 1, vec![a])).unwrap();
    assert_eq!(registry.list(0, 128).unwrap().total, 2);
    let ids = registry
        .list(0, 128)
        .unwrap()
        .resources
        .into_iter()
        .map(|r| r.id)
        .collect::<Vec<_>>();
    registry
        .reconcile(batch(
            "gateway-a",
            2,
            vec![observation(
                "same-module",
                Some("osg:verified-uid"),
                Transport::Can,
                10,
            )],
        ))
        .unwrap();
    registry
        .reconcile(batch(
            "gateway-b",
            2,
            vec![observation(
                "same-module",
                Some("osg:verified-uid"),
                Transport::Usb,
                20,
            )],
        ))
        .unwrap();
    assert_eq!(registry.list(0, 128).unwrap().total, 1);
    for id in &ids {
        assert!(registry.inspect(id, 0, 128).is_ok());
    }
    let assignments = ids
        .iter()
        .enumerate()
        .map(|(i, id)| LocalAssignment {
            resource_id: format!("role-{i}"),
            source_id: id.clone(),
        ports: vec![],
        })
        .collect::<Vec<_>>();
    assert_eq!(registry.duplicate_sources(&assignments).len(), 2);
    assert_eq!(
        registry
            .token(&ids[0], "led.set", Access::Invoke)
            .unwrap()
            .route_id,
        "same-module-route"
    );
    Registry::restore(registry.saved()).unwrap();
}

#[test]
fn route_takeover_and_delayed_responses_cannot_reuse_prior_authority() {
    let mut registry = Registry::new();
    registry
        .reconcile(batch(
            "can",
            1,
            vec![observation("can", Some("osg:uid"), Transport::Can, 10)],
        ))
        .unwrap();
    let id = only_id(&registry);
    let old = registry.token(&id, "led.set", Access::Invoke).unwrap();
    registry
        .reconcile(batch(
            "usb",
            1,
            vec![observation("usb", Some("osg:uid"), Transport::Usb, 20)],
        ))
        .unwrap();
    assert!(registry
        .validate_invocation(&old, "led.set", &[PortValue::Boolean(true)])
        .is_err());
    let current = registry.token(&id, "led.set", Access::Invoke).unwrap();
    assert_eq!(current.route_id, "usb-route");
    let invocation = registry
        .validate_invocation(&current, "led.set", &[PortValue::Boolean(true)])
        .unwrap();
    assert_eq!(invocation.arguments(), &[PortValue::Boolean(true)]);
    assert_eq!(invocation.token(), &current);
    assert_eq!(invocation.resolved().scope, "usb");
    let mut next = batch(
        "usb",
        1,
        vec![observation("usb", Some("osg:uid"), Transport::Usb, 20)],
    );
    assert!(registry.reconcile(next.clone()).is_err());
    next.session = "provider-2".into();
    registry.reconcile(next).unwrap();
    assert!(registry
        .validate_token(&current, "led.set", Access::Invoke)
        .is_err());
    assert!(registry
        .reconcile(batch(
            "usb",
            99,
            vec![observation("usb", Some("osg:uid"), Transport::Usb, 20)]
        ))
        .is_err());
}

#[test]
fn partial_scans_and_pages_never_mean_absence_and_keep_current_tokens_stable() {
    let mut registry = Registry::new();
    let mut a = observation("a", None, Transport::Hid, 0);
    a.ports = Some(PortUpdate {
        revision: "catalogue-v1".into(),
        total: 20_000,
        offset: 0,
        complete: false,
        ports: vec![state("known")],
    });
    registry
        .reconcile(batch(
            "controllers",
            1,
            vec![a.clone(), observation("b", None, Transport::Hid, 0)],
        ))
        .unwrap();
    let id = registry
        .saved()
        .resources
        .iter()
        .find(|r| r.sightings[0].key == "a")
        .unwrap()
        .id
        .clone();
    let token = registry.token(&id, "known", Access::Read).unwrap();
    let mut partial = batch("controllers", 2, vec![a]);
    partial.complete = false;
    partial.observations[0].ports.as_mut().unwrap().offset = 1;
    partial.observations[0].ports.as_mut().unwrap().ports = vec![state("next")];
    registry.reconcile(partial).unwrap();
    registry
        .validate_token(&token, "known", Access::Read)
        .unwrap();
    let details = registry.inspect(&id, 0, 1).unwrap();
    assert_eq!(details.resource.port_count, 20_000);
    assert_eq!(details.total, 2);
    assert_eq!(
        registry
            .list(0, 128)
            .unwrap()
            .resources
            .iter()
            .filter(|r| r.availability == Availability::Online)
            .count(),
        2
    );
    assert_eq!(registry.inspect(&id, 1, 1).unwrap().ports.len(), 1);
    assert!(registry.inspect(&id, 3, 1).is_err());
    assert!(registry.list(0, 0).is_err());
    assert!(registry.list(0, 129).is_err());
}

#[test]
fn provider_failure_invalidates_tokens_and_preserves_explainable_descriptors() {
    let mut registry = Registry::new();
    registry
        .reconcile(batch(
            "controllers",
            1,
            vec![observation("a", None, Transport::Hid, 0)],
        ))
        .unwrap();
    let id = only_id(&registry);
    let token = registry.token(&id, "button:0", Access::Read).unwrap();
    registry.provider_failed("controllers", "Permission denied");
    assert!(registry
        .validate_token(&token, "button:0", Access::Read)
        .is_err());
    let details = registry.inspect(&id, 0, 128).unwrap();
    assert_eq!(details.resource.availability, Availability::Unavailable);
    assert!(details
        .resource
        .diagnostics
        .contains(&"Permission denied".into()));
    assert_eq!(details.ports.len(), 2);
    let revision = registry.revision();
    registry.provider_failed("controllers", "Permission denied");
    assert_eq!(revision, registry.revision());
    registry
        .reconcile(batch(
            "controllers",
            2,
            vec![observation("a", None, Transport::Hid, 0)],
        ))
        .unwrap();
    assert_eq!(
        registry.inspect(&id, 0, 128).unwrap().resource.availability,
        Availability::Online
    );
}

#[test]
fn route_specific_ports_do_not_borrow_capabilities_from_another_route() {
    let mut registry = Registry::new();
    let can = observation("can", Some("osg:uid"), Transport::Can, 10);
    let mut hid = observation("hid", Some("osg:uid"), Transport::Hid, 30);
    hid.ports = Some(PortUpdate::complete(vec![state("hid-button")]).unwrap());
    registry
        .reconcile(batch("can", 1, vec![can.clone()]))
        .unwrap();
    registry.reconcile(batch("hid", 1, vec![hid])).unwrap();
    let id = only_id(&registry);
    assert_eq!(
        registry
            .token(&id, "led.set", Access::Invoke)
            .unwrap()
            .route_id,
        "can-route"
    );
    assert!(registry.token(&id, "hid-button", Access::Invoke).is_err());
    assert!(registry.token(&id, "led.set", Access::Read).is_err());
    let mut other = can;
    other.key = "other".into();
    other.route.as_mut().unwrap().id = "other-route".into();
    registry.reconcile(batch("other", 1, vec![other])).unwrap();
    assert!(registry
        .token(&id, "led.set", Access::Invoke)
        .unwrap_err()
        .contains("Ambiguous"));
}

#[test]
fn changed_or_missing_capabilities_and_read_only_routes_cannot_accept_actions() {
    let mut registry = Registry::new();
    let mut o = observation("a", None, Transport::Usb, 0);
    registry
        .reconcile(batch("usb", 1, vec![o.clone()]))
        .unwrap();
    let id = only_id(&registry);
    let token = registry.token(&id, "led.set", Access::Invoke).unwrap();
    o.ports = Some(PortUpdate::complete(vec![state("button:0")]).unwrap());
    registry
        .reconcile(batch("usb", 2, vec![o.clone()]))
        .unwrap();
    assert!(registry
        .validate_token(&token, "led.set", Access::Invoke)
        .is_err());
    o.ports = Some(PortUpdate::complete(vec![action("led.set")]).unwrap());
    o.route.as_mut().unwrap().writable = false;
    registry.reconcile(batch("usb", 3, vec![o])).unwrap();
    assert!(registry.token(&id, "led.set", Access::Invoke).is_err());
    let mut forged = token;
    forged.inventory_session = "other".into();
    assert!(registry
        .validate_token(&forged, "led.set", Access::Invoke)
        .is_err());
}

#[test]
fn assignment_expectations_pin_actual_schema_and_diagnose_retained_offline_bindings() {
    let mut registry = Registry::new();
    registry
        .reconcile(batch(
            "usb",
            1,
            vec![observation("a", None, Transport::Usb, 0)],
        ))
        .unwrap();
    let id = only_id(&registry);
    let mut logical = Resource {
        id: "panel".into(),
        name: "Panel".into(),
        kind: crate::ResourceKind::Device,
        required: true,
        ports: vec![crate::Port {
            id: "button".into(),
            name: "Button".into(),
            direction: crate::Direction::Output,
            contract:state("button:0").contract,

        }],
    };
    let mut assignment = LocalAssignment {
        resource_id: logical.id.clone(),
        source_id: id.clone(),
    ports: vec![],
    };
    let mut expected = PortExpectation {
        profile_port_id: "button".into(),
        observed_port_id: "button:0".into(),
        expected_contract_hash: state("button:0").contract_hash().unwrap(),
    };
    assert!(
        registry
            .check_assignment(&logical, &assignment, &[expected.clone()])
            .valid
    );
    expected.expected_contract_hash = "stale".into();
    assert!(
        !registry
            .check_assignment(&logical, &assignment, &[expected.clone()])
            .valid
    );
    expected.expected_contract_hash = state("button:0").contract_hash().unwrap();
    logical.ports[0].contract = PortContract::Event {schema:ScalarSchema::Text {max_bytes:8,encoding:TextEncoding::Utf8}};
    assert!(
        !registry
            .check_assignment(&logical, &assignment, &[expected.clone()])
            .valid
    );
    logical.ports[0].contract = state("button:0").contract;
    assert!(!registry.check_assignment(&logical, &assignment, &[]).valid);
    assert!(
        !registry
            .check_assignment(&logical, &assignment, &[expected.clone(), expected.clone()])
            .valid
    );
    logical.kind = crate::ResourceKind::Companion;
    assert!(
        !registry
            .check_assignment(&logical, &assignment, &[expected.clone()])
            .valid
    );
    logical.kind = crate::ResourceKind::Device;
    assignment.resource_id = "wrong".into();
    assert!(
        !registry
            .check_assignment(&logical, &assignment, &[expected.clone()])
            .valid
    );
    assignment.resource_id = "panel".into();
    registry.reconcile(batch("usb", 2, vec![])).unwrap();
    let report = registry.check_assignment(&logical, &assignment, &[expected]);
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.code == "resourceOffline"));
    assert!(report.diagnostics.iter().any(|d| d.code == "portOffline"));
    assignment.source_id = uuid::Uuid::new_v4().to_string();
    assert!(!registry.check_assignment(&logical, &assignment, &[]).valid);
}

#[test]
fn malformed_batches_are_atomic_and_tampered_saved_claims_are_rejected() {
    let mut registry = Registry::new();
    registry
        .reconcile(batch(
            "usb",
            1,
            vec![observation("a", Some("osg:one"), Transport::Usb, 0)],
        ))
        .unwrap();
    let before = serde_json::to_value(registry.saved()).unwrap();
    let changed = batch(
        "usb",
        2,
        vec![observation("a", Some("osg:other"), Transport::Usb, 0)],
    );
    assert!(registry.reconcile(changed).is_err());
    assert_eq!(serde_json::to_value(registry.saved()).unwrap(), before);
    let mut saved = registry.saved();
    saved.resources[0].sightings[0]
        .catalogue
        .as_mut()
        .unwrap()
        .hash = "tampered".into();
    assert!(Registry::restore(saved).is_err());
    let mut saved = registry.saved();
    saved.redirects.insert(
        uuid::Uuid::new_v4().to_string(),
        uuid::Uuid::new_v4().to_string(),
    );
    assert!(Registry::restore(saved).is_err());
    let mut saved = registry.saved();
    let mut duplicate = saved.resources[0].clone();
    duplicate.id = uuid::Uuid::new_v4().to_string();
    saved.resources.push(duplicate);
    assert!(saved.validate().is_err());
    let mut saved = registry.saved();
    saved.schema_version = 2;
    assert!(saved.validate().is_err());
    let mut invalid = batch("usb", 2, vec![observation("b", None, Transport::Usb, 0)]);
    invalid.observations.push(invalid.observations[0].clone());
    assert!(registry.reconcile(invalid).is_err());
    let mut invalid = batch("usb", 0, vec![]);
    assert!(registry.reconcile(invalid.clone()).is_err());
    invalid.revision = 2;
    invalid.scope = "".into();
    assert!(registry.reconcile(invalid).is_err());
    let mut invalid = batch("usb", 2, vec![observation("b", None, Transport::Usb, 0)]);
    invalid.observations[0].resource_type = ResourceType::Companion;
    invalid.observations[0].evidence = vec![IdentityEvidence {
        kind: EvidenceKind::VerifiedPhysical,
        value: "osg:bad".into(),
    }];
    assert!(registry.reconcile(invalid).is_err());
}

#[test]
fn reused_catalogue_revision_with_changed_contract_never_overwrites_known_ports() {
    let mut registry = Registry::new();
    let mut o = observation("a", None, Transport::Usb, 0);
    o.ports.as_mut().unwrap().revision = "fixed-revision".into();
    registry
        .reconcile(batch("usb", 1, vec![o.clone()]))
        .unwrap();
    let id = only_id(&registry);
    let token = registry.token(&id, "button:0", Access::Read).unwrap();
    o.ports.as_mut().unwrap().ports[0].contract = PortContract::Event {
        schema: ScalarSchema::Boolean {},
    };
    assert!(registry.reconcile(batch("usb", 2, vec![o])).is_err());
    registry
        .validate_token(&token, "button:0", Access::Read)
        .unwrap();
    registry.provider_failed("usb", "Invalid descriptor revision reuse");
    assert!(registry
        .validate_token(&token, "button:0", Access::Read)
        .is_err());
}

#[test]
fn physical_identity_conflicts_block_writes_regardless_of_route_priority() {
    let mut registry = Registry::new();
    let mut can = observation("can", Some("osg:uid"), Transport::Can, 50);
    can.route
        .as_mut()
        .unwrap()
        .attributes
        .insert("boot".into(), "10".into());
    registry
        .reconcile(batch("can-a", 1, vec![can.clone()]))
        .unwrap();
    let id = only_id(&registry);
    let mut usb = observation("usb", Some("osg:uid"), Transport::Usb, 100);
    usb.route
        .as_mut()
        .unwrap()
        .attributes
        .insert("boot".into(), "10".into());
    registry
        .reconcile(batch("usb", 1, vec![usb.clone()]))
        .unwrap();
    let token = registry.token(&id, "led.set", Access::Invoke).unwrap();
    assert_eq!(token.route_id, "usb-route");
    usb.route
        .as_mut()
        .unwrap()
        .attributes
        .insert("boot".into(), "11".into());
    registry
        .reconcile(batch("usb", 2, vec![usb.clone()]))
        .unwrap();
    assert!(registry
        .validate_token(&token, "led.set", Access::Invoke)
        .is_err());
    assert!(registry
        .token(&id, "led.set", Access::Invoke)
        .unwrap_err()
        .contains("boot"));
    assert!(registry.token(&id, "button:0", Access::Read).is_ok());
    assert!(registry
        .inspect(&id, 0, 128)
        .unwrap()
        .resource
        .diagnostics
        .iter()
        .any(|d| d.contains("Conflicting")));
    usb.route
        .as_mut()
        .unwrap()
        .attributes
        .insert("boot".into(), "10".into());
    registry.reconcile(batch("usb", 3, vec![usb])).unwrap();
    can.key = "second-can".into();
    can.route.as_mut().unwrap().id = "second-can-route".into();
    can.route.as_mut().unwrap().priority = 1;
    registry.reconcile(batch("can-b", 1, vec![can])).unwrap();
    assert!(registry
        .token(&id, "led.set", Access::Invoke)
        .unwrap_err()
        .contains("multiple live CAN"));
}

#[test]
fn remembered_catalogues_cannot_authorize_a_new_unverified_connection() {
    let mut registry = Registry::new();
    let mut o = observation("a", None, Transport::Usb, 0);
    registry
        .reconcile(batch("usb", 1, vec![o.clone()]))
        .unwrap();
    let id = only_id(&registry);
    let token = registry.token(&id, "led.set", Access::Invoke).unwrap();
    o.ports = None;
    registry
        .reconcile(batch("usb", 2, vec![o.clone()]))
        .unwrap();
    registry
        .validate_token(&token, "led.set", Access::Invoke)
        .unwrap();
    o.route.as_mut().unwrap().provider_generation = "2".into();
    registry
        .reconcile(batch("usb", 3, vec![o.clone()]))
        .unwrap();
    assert!(registry.token(&id, "led.set", Access::Invoke).is_err());
    assert!(registry
        .inspect(&id, 0, 128)
        .unwrap()
        .ports
        .iter()
        .all(|p| !p.current));
    let mut restored = Registry::restore(registry.saved()).unwrap();
    restored
        .reconcile(batch("usb", 1, vec![o.clone()]))
        .unwrap();
    assert!(restored.token(&id, "led.set", Access::Invoke).is_err());
    o.ports = Some(PortUpdate::complete(vec![state("button:0"), action("led.set")]).unwrap());
    restored.reconcile(batch("usb", 2, vec![o])).unwrap();
    assert!(restored.token(&id, "led.set", Access::Invoke).is_ok());
}

#[test]
fn a_full_twenty_thousand_port_catalogue_is_paged_without_truncation() {
    let mut registry = Registry::new();
    let ports = (0..20_000)
        .map(|index| state(&format!("channel-{index:05}")))
        .collect();
    let mut o = observation("game", None, Transport::Integration, 0);
    o.resource_type = ResourceType::GameIntegration;
    o.route.as_mut().unwrap().writable = false;
    o.ports = Some(PortUpdate::complete(ports).unwrap());
    registry
        .reconcile(batch("integration", 1, vec![o]))
        .unwrap();
    let id = only_id(&registry);
    let last = registry.inspect(&id, 19_999, 1).unwrap();
    assert_eq!(last.total, 20_000);
    assert_eq!(last.resource.port_count, 20_000);
    assert_eq!(last.ports[0].descriptor.id, "channel-19999");
    assert!(registry.token(&id, "channel-19999", Access::Read).is_ok());
    assert!(registry.inspect(&id, 20_000, 128).unwrap().ports.is_empty());
    Registry::restore(registry.saved()).unwrap();
}

#[test]
fn serialized_envelope_limit_is_enforced_without_an_unbounded_output_buffer() {
    use std::io::Write;
    let mut writer = InventoryLimit {
        written: crate::resources_store::MAX_INVENTORY_BYTES - 1,
    };
    assert_eq!(writer.write(b"x").unwrap(), 1);
    assert!(writer.write(b"x").is_err());
    writer.flush().unwrap();
    let mut saved = SavedRegistry::default();
    saved.resources.push(SavedResource {
        id: uuid::Uuid::new_v4().to_string(),
        name: "x".repeat(crate::resources_store::MAX_INVENTORY_BYTES),
        resource_type: ResourceType::HostController,
        sightings: vec![],
    });
    assert!(saved.validate().unwrap_err().contains("128 MiB"));
}
