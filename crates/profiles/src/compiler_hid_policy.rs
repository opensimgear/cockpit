//! Native publication evidence is acquisition metadata, never profile authority.
use super::*;
use crate::resources::{Connection, ResourceSummary, ResourceType, Transport};
use serde_json::{json, Value};

pub fn attribute(route: &str) -> String {
    // Resource metadata keys are bounded to 80 bytes, including long USB paths.
    format!("dcsHid:{:x}", Sha256::digest(route.as_bytes()))
}

pub fn evidence(route: &Route, status: &str) -> String {
    json!({"schema":1,"routeId":route.id,"providerGeneration":route.provider_generation,"status":status}).to_string()
}

pub fn status(resource: &ResourceSummary, connection: &Connection) -> Option<String> {
    let proof: Value =
        serde_json::from_str(resource.attributes.get(&attribute(&connection.route.id))?).ok()?;
    (proof["schema"] == 1
        && proof["routeId"] == connection.route.id
        && proof["providerGeneration"] == connection.route.provider_generation)
        .then(|| proof["status"].as_str().map(str::to_owned))
        .flatten()
}

fn dcs_sink(sink: &SinkPlan, snapshot: &CompileSnapshot) -> bool {
    sink.adapter == SinkAdapter::SimulatorCommand
        && sink.readiness != PlanReadiness::Dormant
        && capture::captured(&sink.endpoint.node_id, snapshot)
            .is_some_and(|r| r.game_id.as_deref() == Some("dcs"))
}

fn disable_capability(
    document: &ProfileDocument,
    logical: &str,
    resource: &ResourceSummary,
) -> bool {
    if !document
        .overrides
        .iter()
        .any(|intent| intent.resource_id == logical && !intent.enabled)
        || resource.connections.len() != 1
    {
        return false;
    }
    let connection = &resource.connections[0];
    let metadata: Value = resource
        .attributes
        .get("controllerPublicationOverride")
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(Value::Null);
    connection.scope.starts_with("gateway:")
        && connection.route.transport == Transport::Can
        && connection.route.writable
        && metadata["schema"] == 1
        && metadata["revision"].as_u64().is_some_and(|n| n > 0)
        && resource
            .attributes
            .get("moduleId")
            .is_some_and(|id| metadata["module"]["id"] == *id)
        && resource.attributes.get("gatewayId").is_some_and(|gateway| {
            connection.route.id
                == format!(
                    "can:{gateway}:{}",
                    metadata["module"]["id"].as_str().unwrap_or("")
                )
        })
}

pub(super) fn apply(
    document: &ProfileDocument,
    snapshot: &CompileSnapshot,
    sources: &[SourcePlan],
    sinks: &mut [SinkPlan],
    acquisition: bool,
    diagnostics: &mut Vec<CompileDiagnostic>,
) {
    let bindings = document.bindings.iter().collect::<Vec<_>>();
    let branch_plans = branches(document, &bindings, sources, sinks);
    for sink in sinks.iter_mut().filter(|sink| dcs_sink(sink, snapshot)) {
        let branch = branch_plans
            .iter()
            .find(|branch| branch.sink == sink.endpoint)
            .unwrap();
        // An absent input branch cannot issue simulator commands and acquires
        // no HID publication lease. Recheck publication if that input returns.
        if branch.readiness == PlanReadiness::Unavailable { continue; }
        for endpoint in &branch.source_ports {
            let Some(resource) = capture::captured(&endpoint.node_id, snapshot)
                .filter(|r| r.summary.resource_type == ResourceType::OsgModule)
            else {
                continue;
            };
            let capable = disable_capability(document, &endpoint.node_id, &resource.summary);
            let safe = !document
                .overrides
                .iter()
                .any(|intent| intent.resource_id == endpoint.node_id && intent.enabled)
                && !resource.summary.connections.is_empty()
                && resource.summary.connections.iter().all(|connection| {
                    let native = connection.scope == "module-usb"
                        || connection.scope.starts_with("gateway:");
                    if !native {
                        return false;
                    }
                    match status(&resource.summary, connection).as_deref() {
                        Some("disabled") => true,
                        Some("lease-disabled") => capable,
                        Some("published") => acquisition && capable,
                        _ => false,
                    }
                });
            if !safe {
                sink.readiness = PlanReadiness::Blocked;
                diagnostics.push(CompileDiagnostic {
                    code: "dcsHidPublicationRequired".into(), severity: DiagnosticSeverity::Warning,
                    message: "Direct DCS commands require every native USB and gateway HID publisher of their OSG input to be disabled. Keep configured or unknown publication cannot authorize this branch; a supported temporary Disable still requires an owned lease and fresh post-publication baseline.".into(),
                    location: GraphLocation { resource_id: Some(endpoint.node_id.clone()),
                        port_id: Some(endpoint.port_id.clone()), edge_ids: branch.edge_ids.clone(), ..Default::default() },
                });
            }
        }
    }
}
