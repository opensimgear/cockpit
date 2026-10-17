use super::*;
use crate::resources::ResourceType;

pub(super) fn check(
    document: &ProfileDocument,
    snapshot: &CompileSnapshot,
    sources: &[SourcePlan],
    sinks: &[SinkPlan],
    diagnostics: &mut Vec<CompileDiagnostic>,
) {
    let bindings = document.bindings.iter().collect::<Vec<_>>();
    let mut physical = BTreeMap::<&str, Vec<&SourcePlan>>::new();
    for source in sources {
        if let Some(id) = source.inventory_resource_id.as_deref() {
            physical.entry(id).or_default().push(source);
        }
        if let Some(resource) = capture::captured(&source.endpoint.node_id, snapshot) {
            let attributes = &resource.summary.attributes;
            let known_virtual = ["virtual", "isVirtual", "virtualOutput"]
                .iter()
                .any(|key| attributes.get(*key).is_some_and(|value| value == "true"))
                || attributes.get("backend").is_some_and(|value| {
                    matches!(
                        value.to_ascii_lowercase().as_str(),
                        "vjoy" | "virtual-controller"
                    )
                });
            if known_virtual {
                error(
                    diagnostics,
                    "virtualOutputFeedback",
                    "A known virtual output cannot be recaptured as a physical input source.",
                    location(source, &bindings),
                );
            }
        }
    }
    for group in physical.values() {
        let logical = group
            .iter()
            .map(|source| &source.endpoint.node_id)
            .collect::<BTreeSet<_>>();
        if logical.len() > 1 {
            for source in group {
                error(diagnostics, "duplicatePhysicalSource", "Several logical input resources resolve to the same proven physical source; keep one capture assignment.", location(source, &bindings));
            }
        }
        let routes = group
            .iter()
            .filter_map(|source| source.route.as_ref().map(|route| &route.id))
            .collect::<BTreeSet<_>>();
        if routes.len() > 1 {
            for source in group {
                error(diagnostics, "duplicatePhysicalInputRoutes", "One physical resource uses several independent HID/USB/CAN capture routes; select one authoritative input route for its controls.", location(source, &bindings));
            }
        }
    }

    let mut endpoints = BTreeMap::<(&str, &str), Vec<&SinkPlan>>::new();
    for sink in sinks {
        if let Some(id) = sink.inventory_resource_id.as_deref() {
            let endpoint = match &sink.contract {
                PortContract::Action { endpoint, .. } => Some(endpoint.as_str()),
                _ => sink.observed_port_id.as_deref(),
            };
            if let Some(endpoint) = endpoint {
                endpoints.entry((id, endpoint)).or_default().push(sink);
            }
        }
    }
    for group in endpoints.values().filter(|group| group.len() > 1) {
        let mut edges = group
            .iter()
            .flat_map(|sink| related_edges(&sink.endpoint, &bindings))
            .collect::<Vec<_>>();
        edges.sort();
        edges.dedup();
        for sink in group {
            error(diagnostics, "conflictingEndpointWriters", "Several graph sinks write the same physical endpoint; combine them through one explicit arbitration/action node and one endpoint writer.", GraphLocation {
                resource_id: Some(sink.endpoint.node_id.clone()), port_id: Some(sink.endpoint.port_id.clone()), edge_ids: edges.clone(), ..Default::default()
            });
        }
    }

    // Signal direction is independent of physical transport. Only the native
    // OSG button state/event pair is a proven alias of one physical control.
    let branch_plans = branches(document, &bindings, sources, sinks);
    let mut domains =
        BTreeMap::<(String, String), (BTreeSet<&str>, Vec<&SourcePlan>, BTreeSet<String>)>::new();
    for branch in &branch_plans {
        let sink = sinks
            .iter()
            .find(|sink| sink.endpoint == branch.sink)
            .unwrap();
        let domain = match sink.adapter {
            SinkAdapter::VirtualController => "virtual",
            SinkAdapter::SimulatorCommand => "simulator",
            _ => continue,
        };
        for endpoint in &branch.source_ports {
            let source = sources
                .iter()
                .find(|source| source.endpoint == *endpoint)
                .unwrap();
            let Some(resource) = capture::captured(&source.endpoint.node_id, snapshot) else {
                continue;
            };
            if !matches!(
                resource.summary.resource_type,
                ResourceType::HostController | ResourceType::OsgModule
            ) {
                continue;
            }
            let Some(observed) = source.observed_port_id.as_deref() else {
                continue;
            };
            let native_osg = resource.summary.resource_type == ResourceType::OsgModule
                && resource.summary.connections.iter().any(|connection| {
                    source
                        .route
                        .as_ref()
                        .is_some_and(|route| route.id == connection.route.id)
                        && (connection.scope == "module-usb"
                            || connection.scope.starts_with("gateway:"))
                });
            let control = if native_osg {
                canonical_osg_button(observed)
            } else {
                observed.to_owned()
            };
            let group = domains
                .entry((resource.summary.id.clone(), control))
                .or_default();
            group.0.insert(domain);
            group.1.push(source);
            group.2.extend(branch.edge_ids.clone());
        }
    }
    for (domains, sources, edges) in domains.values() {
        if domains.contains("virtual") && domains.contains("simulator") {
            let mut seen = BTreeSet::new();
            for source in sources {
                if seen.insert(key(&source.endpoint)) {
                    error(diagnostics, "virtualDirectSimulatorConflict", "The same physical control feeds a virtual controller and a direct simulator command; choose one game-visible command route.", GraphLocation {
                        resource_id: Some(source.endpoint.node_id.clone()), port_id: Some(source.endpoint.port_id.clone()), edge_ids: edges.iter().cloned().collect(), ..Default::default()
                    });
                }
            }
        }
    }
}

fn location(source: &SourcePlan, bindings: &[&Binding]) -> GraphLocation {
    GraphLocation {
        resource_id: Some(source.endpoint.node_id.clone()),
        port_id: Some(source.endpoint.port_id.clone()),
        edge_ids: related_edges(&source.endpoint, bindings),
        ..Default::default()
    }
}

fn canonical_osg_button(port: &str) -> String {
    for prefix in ["button-event:", "button:"] {
        if let Some(index) = port.strip_prefix(prefix).filter(|index| {
            index
                .parse::<u16>()
                .is_ok_and(|number| number.to_string() == *index)
        }) {
            return format!("button:{index}");
        }
    }
    port.to_owned()
}
