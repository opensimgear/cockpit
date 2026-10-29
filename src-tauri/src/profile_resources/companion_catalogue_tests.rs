use super::*;
use osg_profiles::{
    compiler_models::{BackendCapabilities, CapturedResource},
    resources::ResourceSummary,
    resources_ports::{PortContract, PortDescriptor, ScalarSchema},
    Direction,
};
use std::collections::BTreeMap;

#[test]
fn desired_contracts_are_inactive_capture_metadata_only_for_offline_companions() {
    let descriptor = PortDescriptor {
        id: "button".into(),
        name: "Button".into(),
        direction: Direction::Output,
        contract: PortContract::State {
            schema: ScalarSchema::Boolean {},
            freshness_ms: 1000,
            fallback: None,
        },
    };
    let catalogue = CompanionCatalogue {
        logical_resource_id: "panel".into(),
        inventory_resource_id: "native".into(),
        instance_id: "instance".into(),
        companion_id: "screen".into(),
        paired_identity_fingerprint: None,
        publication_hash: format!("sha256:{}", "a".repeat(64)),
        capability_fingerprint: format!("sha256:{}", "b".repeat(64)),
        ports: vec![descriptor.clone()],
    };
    let mut snapshot = CompileSnapshot {
        inventory_session: "fixture".into(),
        inventory_revision: "1".into(),
        assignments: vec![],
        game_context: None,
        backend: BackendCapabilities::default(),
        inventory_diagnostics: vec![],
        read_only: false,
        resources: vec![CapturedResource {
            summary: ResourceSummary {
                id: "native".into(),
                name: "Screen".into(),
                resource_type: ResourceType::Companion,
                availability: Availability::Offline,
                generation: "1".into(),
                capability_revision: "empty".into(),
                port_count: 0,
                cached_port_count: 0,
                connections: vec![],
                attributes: BTreeMap::new(),
                diagnostics: vec![],
            },
            ports: vec![],
            tokens: vec![],
            provider_setup_revision: None,
            game_id: None,
        }],
    };
    for (kind, availability) in [
        (ResourceType::HostController, Availability::Offline),
        (ResourceType::Companion, Availability::Online),
    ] {
        snapshot.resources[0].summary.resource_type = kind;
        snapshot.resources[0].summary.availability = availability;
        capture_offline(&mut snapshot, std::slice::from_ref(&catalogue)).unwrap();
        assert!(
            snapshot.resources[0].ports.is_empty(),
            "Live or unrelated resources cannot inherit desired contracts"
        );
    }
    snapshot.resources[0].summary.availability = Availability::Offline;
    capture_offline(&mut snapshot, std::slice::from_ref(&catalogue)).unwrap();
    capture_offline(&mut snapshot, std::slice::from_ref(&catalogue)).unwrap();
    let resource = &snapshot.resources[0];
    assert_eq!(resource.ports.len(), 1);
    assert_eq!(resource.ports[0].descriptor, descriptor);
    assert!(!resource.ports[0].current && resource.ports[0].route_ids.is_empty());
    assert!(resource.tokens.is_empty() && resource.summary.connections.is_empty());
    assert_eq!(resource.summary.port_count, 0);
    snapshot.resources[0]
        .summary
        .attributes
        .insert("companionId".into(), "another-screen".into());
    assert!(capture_offline(&mut snapshot, std::slice::from_ref(&catalogue)).is_err());
    snapshot.resources[0]
        .summary
        .attributes
        .insert("companionId".into(), "screen".into());
    snapshot.resources[0]
        .summary
        .attributes
        .insert("componentKind".into(), "paired-companion".into());
    snapshot.resources[0]
        .summary
        .attributes
        .insert("pairedIdentityFingerprint".into(), "changed-pairing".into());
    let mut sealed = catalogue;
    sealed.paired_identity_fingerprint = Some("original-pairing".into());
    assert!(capture_offline(&mut snapshot, &[sealed]).is_err());
}
