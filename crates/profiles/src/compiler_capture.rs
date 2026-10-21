use super::*;
use crate::resources::{Access, Availability, ResourceType};
use crate::resources_ports::{graph_compatible, text};
use crate::{Direction, Port, Resource, ResourceKind};

#[derive(Clone)]
pub(super) struct ResolvedPort {
    pub inventory_id: Option<String>,
    pub observed_id: Option<String>,
    pub token: Option<ResourceToken>,
    pub preparation: Option<PortPreparation>,
    pub route: Option<Route>,
    pub readiness: PlanReadiness,
}

pub(super) fn validate_snapshot(snapshot: &CompileSnapshot) -> Result<(), String> {
    text(&snapshot.inventory_session, 256)?;
    decimal(&snapshot.inventory_revision)?;
    if snapshot.assignments.len() > 64
        || snapshot.resources.len() > 64
        || snapshot
            .resources
            .iter()
            .map(|r| r.ports.len())
            .sum::<usize>()
            > 1024
        || snapshot
            .resources
            .iter()
            .map(|r| r.tokens.len())
            .sum::<usize>()
            > 1024
    {
        return Err("Compile capture exceeds 64 resources or 1024 exact ports/tokens.".into());
    }
    let mut assignments = BTreeSet::new();
    for assignment in &snapshot.assignments {
        if !assignments.insert(&assignment.resource_id) || assignment.ports.len() > 128 {
            return Err(
                "Compile capture contains duplicate or oversized local assignments.".into(),
            );
        }
        let mut expected = BTreeSet::new();
        for port in &assignment.ports {
            if !expected.insert(&port.profile_port_id) {
                return Err("Compile capture contains duplicate logical port expectations.".into());
            }
        }
    }
    let mut resources = BTreeSet::new();
    for resource in &snapshot.resources {
        text(&resource.summary.id, 1024)?;
        decimal(&resource.summary.generation)?;
        if !resources.insert(&resource.summary.id) || resource.summary.connections.len() > 64 {
            return Err("Compile capture contains duplicate resources or too many routes.".into());
        }
        let mut ports = BTreeSet::new();
        for port in &resource.ports {
            port.descriptor.validate()?;
            if matches!(port.descriptor.contract, PortContract::Invocation { .. })
                || port.contract_hash != port.descriptor.contract_hash()?
                || !ports.insert((&port.descriptor.id, &port.contract_hash))
            {
                return Err(
                    "Compile capture contains an invalid provider port variant or contract hash."
                        .into(),
                );
            }
        }
    }
    Ok(())
}

fn decimal(value: &str) -> Result<(), String> {
    if value.parse::<u64>().is_ok_and(|n| n.to_string() == value) {
        Ok(())
    } else {
        Err("Compile capture counters must be canonical unsigned decimal strings.".into())
    }
}

pub(super) fn fingerprint(snapshot: &CompileSnapshot) -> Result<String, String> {
    validate_snapshot(snapshot)?;
    let mut snapshot = snapshot.clone();
    snapshot
        .assignments
        .sort_by(|a, b| a.resource_id.cmp(&b.resource_id));
    for assignment in &mut snapshot.assignments {
        assignment
            .ports
            .sort_by(|a, b| a.profile_port_id.cmp(&b.profile_port_id));
    }
    snapshot
        .resources
        .sort_by(|a, b| a.summary.id.cmp(&b.summary.id));
    snapshot.inventory_diagnostics.sort();
    for resource in &mut snapshot.resources {
        resource
            .summary
            .connections
            .sort_by(|a, b| (&a.scope, &a.key, &a.route.id).cmp(&(&b.scope, &b.key, &b.route.id)));
        resource.summary.diagnostics.sort();
        resource.ports.sort_by(|a, b| {
            (&a.descriptor.id, &a.contract_hash).cmp(&(&b.descriptor.id, &b.contract_hash))
        });
        for port in &mut resource.ports {
            port.route_ids.sort();
        }
        resource.tokens.sort_by_key(|token| {
            serde_json::to_string(token)
                .expect("resource tokens contain only bounded strings and an enum")
        });
    }
    hash(&snapshot)
}

pub(super) fn captured<'a>(
    logical: &str,
    snapshot: &'a CompileSnapshot,
) -> Option<&'a CapturedResource> {
    let assignment = snapshot
        .assignments
        .iter()
        .find(|a| a.resource_id == logical)?;
    snapshot
        .resources
        .iter()
        .find(|r| r.summary.id == assignment.source_id)
}

pub(super) fn resolve(
    logical: &Resource,
    port: &Port,
    snapshot: &CompileSnapshot,
    diagnostics: &mut Vec<CompileDiagnostic>,
    edge_ids: Vec<String>,
    preparations: Option<&[PortPreparation]>,
    companion_publication: Option<&str>,
) -> ResolvedPort {
    let location = GraphLocation {
        resource_id: Some(logical.id.clone()),
        port_id: Some(port.id.clone()),
        edge_ids,
        ..Default::default()
    };
    let mut result = ResolvedPort {
        inventory_id: None,
        observed_id: None,
        token: None,
        preparation: None,
        route: None,
        readiness: PlanReadiness::Blocked,
    };
    let game_dormant = logical.kind == ResourceKind::Game && snapshot.game_context.is_none();
    if game_dormant {
        result.readiness = PlanReadiness::Dormant;
    }
    let mut warn = |code: &str, message: &str| {
        diagnostics.push(CompileDiagnostic {
            code: code.into(),
            severity: DiagnosticSeverity::Warning,
            message: message.into(),
            location: location.clone(),
        });
    };
    let Some(assignment) = snapshot
        .assignments
        .iter()
        .find(|a| a.resource_id == logical.id)
    else {
        warn(
            "resourceUnassigned",
            "Assign this logical resource before this branch can run.",
        );
        return result;
    };
    result.inventory_id = Some(assignment.source_id.clone());
    let expected = assignment.ports.iter().find(|p| p.profile_port_id == port.id);
    let Some(resource) = captured(&logical.id, snapshot) else {
        if matches!(logical.kind, ResourceKind::Device | ResourceKind::Companion) {
            if let Some(expected) = expected {
                result.observed_id = Some(expected.observed_port_id.clone());
                result.readiness = PlanReadiness::Unavailable;
            }
        }
        warn(
            "resourceMissing",
            "The assigned persistent resource is missing from captured inventory.",
        );
        return result;
    };
    let kind_matches = matches!(
        (logical.kind, resource.summary.resource_type),
        (
            ResourceKind::Device,
            ResourceType::HostController | ResourceType::OsgModule
        ) | (ResourceKind::Companion, ResourceType::Companion)
            | (ResourceKind::Game, ResourceType::GameIntegration)
    );
    if !kind_matches {
        warn(
            "resourceKindChanged",
            "The assigned resource kind differs from this logical block.",
        );
        return result;
    }
    let Some(expected) = expected else {
        warn(
            "portUnassigned",
            "Assign this exact logical port to an observed port and contract hash.",
        );
        return result;
    };
    result.observed_id = Some(expected.observed_port_id.clone());
    // A sealed native preparation may describe a retained desired publication
    // which differs from the currently assigned one. It is acquisition-only;
    // the ordinary delivery compiler never consults these prospective ports.
    let prospective = preparations.and_then(|pins| pins.iter().find_map(|pin| {
        let port = pin.prospective_companion_port(&snapshot.inventory_session, &resource.summary, companion_publication)?;
        (port.descriptor.id == expected.observed_port_id && port.contract_hash == expected.expected_contract_hash).then_some(port)
    }));
    let Some(observed) = prospective.as_ref().or_else(|| resource.ports.iter().find(|p| {
        p.descriptor.id == expected.observed_port_id
            && p.contract_hash == expected.expected_contract_hash
    })) else {
        warn("portContractChanged", "The selected observed port is missing or its complete contract changed; reconcile this binding.");
        return result;
    };
    if !graph_compatible(&observed.descriptor, port) {
        warn(
            "portContractChanged",
            "The portable port differs from its exact captured provider contract.",
        );
        return result;
    }
    // Only an explicitly offline physical peripheral with a retained, matching
    // exact contract is skippable. Unavailable backends and stale online ports
    // retain their normal readiness gates.
    let physical_offline = matches!(logical.kind, ResourceKind::Device | ResourceKind::Companion)
        && resource.summary.availability == Availability::Offline;
    let mut readiness = if physical_offline { PlanReadiness::Unavailable } else { PlanReadiness::Ready };
    if logical.kind == ResourceKind::Game {
        let context = snapshot.game_context.as_ref();
        let matching = context
            .is_some_and(|context| resource.game_id.as_deref() == Some(context.game_id.as_str()));
        let setup_matches = context.is_some_and(|context| {
            resource
                .provider_setup_revision
                .as_ref()
                .is_some_and(|revision| {
                    context.provider_setup_revisions.get(&resource.summary.id) == Some(revision)
                })
        });
        if !matching || !setup_matches {
            warn("gameContextDormant", "This integration branch awaits its matching verified game and captured game-details provider setup revision.");
            readiness = PlanReadiness::Dormant;
        }
    }
    if resource.summary.availability != Availability::Online || (!observed.current && prospective.is_none()) {
        warn("resourceUnavailable", "This exact port is offline, unavailable or awaiting current-route capability verification.");
        if readiness != PlanReadiness::Dormant && !physical_offline {
            readiness = PlanReadiness::Blocked;
        }
    }
    let access = if port.direction == Direction::Output {
        Access::Read
    } else if matches!(port.contract, PortContract::Action { .. }) {
        Access::Invoke
    } else {
        Access::Write
    };
    if let Some(preparations) = preparations {
        let mut selected = preparations
            .iter()
            .filter(|preparation| {
                preparation.companion_publication().is_none_or(|(_, publication, _)| Some(publication) == companion_publication)
                && preparation.matches_capture(
                    &snapshot.inventory_session,
                    &resource.summary,
                    observed,
                    access,
                )
            })
            .collect::<Vec<_>>();
        selected.sort_by(|left, right| {
            right
                .resolved()
                .route
                .priority
                .cmp(&left.resolved().route.priority)
                .then(left.resolved().route.id.cmp(&right.resolved().route.id))
        });
        if selected.len() == 1 {
            if readiness != PlanReadiness::Unavailable {
                result.preparation = Some(selected[0].clone());
                result.route = Some(selected[0].resolved().route.clone());
            }
        } else if selected.len() > 1 || readiness != PlanReadiness::Unavailable {
            warn("routeNotPrepared","This exact port lacks one current unambiguous native acquisition pin; setup, identity, capabilities and route conflicts must be resolved before takeover.");
            if readiness != PlanReadiness::Dormant {
                readiness = PlanReadiness::Blocked;
            }
        }
        result.readiness = readiness;
        return result;
    }
    let mut tokens = resource
        .tokens
        .iter()
        .filter_map(|token| {
            let connection = resource
                .summary
                .connections
                .iter()
                .find(|c| c.route.id == token.route_id)?;
            let route = &connection.route;
            let allowed = if access == Access::Read {
                route.baseline_ready
            } else {
                route.writable
            };
            (allowed
                && token.inventory_session == snapshot.inventory_session
                && token.resource_id == resource.summary.id
                && token.generation == resource.summary.generation
                && token.capability_revision == resource.summary.capability_revision
                && token.port_id == observed.descriptor.id
                && token.contract_hash == observed.contract_hash
                && token.access == access
                && !token.provider_session.is_empty()
                && token.provider_generation == route.provider_generation
                && observed.route_ids.contains(&route.id))
            .then_some((token, route))
        })
        .collect::<Vec<_>>();
    tokens.sort_by(|a, b| b.1.priority.cmp(&a.1.priority).then(a.1.id.cmp(&b.1.id)));
    if tokens
        .get(1)
        .is_some_and(|next| next.1.priority == tokens[0].1.priority)
    {
        warn("routeAmbiguous", "Several current routes have equal authority; select one authoritative input/output route.");
        if readiness != PlanReadiness::Dormant {
            readiness = PlanReadiness::Blocked;
        }
    } else if let Some((token, route)) = tokens.first() {
        if readiness != PlanReadiness::Unavailable {
            result.token = Some((*token).clone());
            result.route = Some((*route).clone());
        }
    } else if readiness != PlanReadiness::Unavailable {
        warn("routeNotReady", "No exact current route token has the required baseline or output capability; no ownership was acquired.");
        if readiness != PlanReadiness::Dormant {
            readiness = PlanReadiness::Blocked;
        }
    }
    result.readiness = readiness;
    result
}
