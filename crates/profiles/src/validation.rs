//! Bounded structural and signal-contract checks; this module executes no graph.
use crate::model::*;
use crate::{MAX_PROFILE_BYTES, MAX_REVISION};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const MAX_RESOURCES: usize = 64;
pub const MAX_PORTS: usize = 1024;
pub const MAX_PORTS_PER_NODE: usize = 128;
pub const MAX_BINDINGS: usize = 512;
pub const MAX_PROCESSING_NODES: usize = 256;
pub const MAX_VIRTUAL_OUTPUTS: usize = 16;
const MAX_COORDINATE: f64 = 1_000_000.0;

pub fn identifier(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id.as_bytes()[0].is_ascii_alphabetic()
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
}
/// Panel identities use the companion protocol grammar, including UUIDs whose
/// first character may be a digit. They are distinct from logical node IDs.
pub fn companion_panel_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}
pub fn companion_publication_hash(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}
/// Port identities are protocol names, not node IDs or machine assignments.
/// Keep every legal native action argument representable without renaming it.
pub fn port_identifier(value: &str) -> bool {
    crate::resources_ports::text(value, 80).is_ok()
}
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= 80
        && name.trim() == name
        && !name.chars().any(char::is_control)
}
fn check(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
fn id(id: &str) -> Result<(), String> {
    check(identifier(id), "Use a logical identifier of 1-80 ASCII letters, digits, dot, underscore or hyphen, starting with a letter.")
}
fn label(name: &str) -> Result<(), String> {
    check(
        valid_name(name),
        "Use a trimmed name of 1-80 characters without control characters.",
    )
}
fn port(port: &Port) -> Result<(), String> {
    check(
        port_identifier(&port.id),
        "Graph port IDs require 1-80 UTF-8 bytes without control characters.",
    )?;
    label(&port.name)?;
    port.contract.validate_graph(port.direction)
}
fn ports<'a>(values: impl IntoIterator<Item = &'a Port>) -> Result<Vec<&'a Port>, String> {
    let values: Vec<_> = values.into_iter().collect();
    check(
        values.len() <= MAX_PORTS_PER_NODE,
        "A node exceeds 128 ports.",
    )?;
    let mut ids = BTreeSet::new();
    for value in &values {
        port(value)?;
        check(
            ids.insert(&value.id),
            "Duplicate port identifier on a node.",
        )?;
    }
    Ok(values)
}
fn operation(node: &ProcessingNode) -> Result<(), String> {
    check(
        node.inputs.iter().all(|p| p.direction == Direction::Input)
            && node
                .outputs
                .iter()
                .all(|p| p.direction == Direction::Output),
        "Processing port direction does not match its declaration.",
    )?;
    osg_mapping::program::validate_operation(
        &node.operation,
        &node.inputs.iter().map(Port::descriptor).collect::<Vec<_>>(),
        &node
            .outputs
            .iter()
            .map(Port::descriptor)
            .collect::<Vec<_>>(),
    )
}
fn endpoint_key(node: &str, port: &str) -> String {
    format!("{node}/{port}")
}
fn acyclic(edges: &BTreeMap<String, BTreeSet<String>>) -> Result<(), String> {
    let mut degree: BTreeMap<&str, usize> = edges.keys().map(|key| (key.as_str(), 0)).collect();
    for targets in edges.values() {
        for target in targets {
            *degree.entry(target).or_default() += 1;
        }
    }
    let mut queue: VecDeque<_> = degree
        .iter()
        .filter_map(|(key, degree)| (*degree == 0).then_some(*key))
        .collect();
    let mut visited = 0;
    while let Some(key) = queue.pop_front() {
        visited += 1;
        if let Some(targets) = edges.get(key) {
            for target in targets {
                let count = degree.get_mut(target.as_str()).unwrap();
                *count -= 1;
                if *count == 0 {
                    queue.push_back(target);
                }
            }
        }
    }
    check(
        visited == degree.len(),
        "Profile contains a processing cycle.",
    )
}

pub fn validate_document(document: &ProfileDocument) -> Result<(), String> {
    check(
        document.schema_version == crate::PROFILE_SCHEMA_VERSION,
        "Unsupported unified profile version.",
    )?;
    let uuid = uuid::Uuid::parse_str(&document.id)
        .map_err(|_| "Profile identity must be a canonical UUID.")?;
    check(
        uuid.to_string() == document.id,
        "Profile identity must be a canonical lowercase UUID.",
    )?;
    check(
        document.revision >= 1 && document.revision <= MAX_REVISION,
        "Profile revision must be a positive safe integer.",
    )?;
    label(&document.name)?;
    check(
        serde_json::to_vec(document)
            .map_err(|e| e.to_string())?
            .len()
            <= MAX_PROFILE_BYTES,
        "Profile exceeds 512 KiB.",
    )?;
    check(
        document.resources.len() <= MAX_RESOURCES
            && document.processing_nodes.len() <= MAX_PROCESSING_NODES
            && document.virtual_outputs.len() <= MAX_VIRTUAL_OUTPUTS
            && document.desktop_outputs.len() <= 1
            && document.processing_nodes.len() + document.desktop_outputs.iter().map(|output| output.actions.len()).sum::<usize>() <= 256
            && document.bindings.len() <= MAX_BINDINGS,
        "Profile exceeds resource, processing, output or binding limits.",
    )?;
    check(
        document.eligibility.game_ids.len() <= 32,
        "Too many game eligibility entries.",
    )?;
    let mut games = BTreeSet::new();
    for game in &document.eligibility.game_ids {
        id(game)?;
        check(games.insert(game), "Duplicate game eligibility entry.")?;
    }
    let mut nodes = BTreeMap::<&str, Vec<&Port>>::new();
    for resource in &document.resources {
        id(&resource.id)?;
        label(&resource.name)?;
        check(
            nodes
                .insert(&resource.id, ports(&resource.ports)?)
                .is_none(),
            "Duplicate graph node identifier.",
        )?;
        check(
            resource
                .ports
                .iter()
                .all(|p| !matches!(p.contract, PortContract::Invocation { .. })),
            "Resource action ports must consume actions.",
        )?;
    }
    for node in &document.processing_nodes {
        id(&node.id)?;
        label(&node.name)?;
        let all = ports(node.inputs.iter().chain(&node.outputs))?;
        operation(node)?;
        check(
            nodes.insert(&node.id, all).is_none(),
            "Duplicate graph node identifier.",
        )?;
    }
    for output in &document.virtual_outputs {
        id(&output.id)?;
        label(&output.name)?;
        output.layout.validate()?;
        let declared = output
            .ports
            .iter()
            .map(|p| p.id.as_str())
            .collect::<BTreeSet<_>>();
        let layout_ports = output
            .layout
            .buttons
            .iter()
            .map(String::as_str)
            .chain(output.layout.axes.iter().map(|a| a.port_id.as_str()))
            .chain(output.layout.hats.iter().map(String::as_str))
            .collect::<BTreeSet<_>>();
        check(
            declared == layout_ports,
            "Virtual controller layout must reference every exact logical port once.",
        )?;
        for p in &output.ports {
            let expected = output
                .layout
                .virtual_port_contract(&p.id)
                .ok_or("Unknown virtual controller port.")?;
            let matches = match (&p.contract, &expected) {
                (
                    PortContract::State {
                        schema: a,
                        fallback: x,
                        ..
                    },
                    PortContract::State {
                        schema: b,
                        fallback: y,
                        ..
                    },
                ) => a == b && x == y,
                _ => false,
            };
            check(
                p.direction == Direction::Input && matches,
                "Virtual controller port does not match its declared control layout.",
            )?;
        }
        check(
            nodes.insert(&output.id, ports(&output.ports)?).is_none(),
            "Duplicate graph node identifier.",
        )?;
    }
    for output in &document.desktop_outputs {
        id(&output.id)?;
        label(&output.name)?;
        if let Some(DesktopScope::Application { application_reference: Some(reference) }) = &output.scope {
            check(uuid::Uuid::parse_str(reference).ok().map(|id| id.to_string()).as_deref() == Some(reference.as_str()),
                "Choose a native captured application reference.")?;
        }
        let declared = ports(&output.ports)?;
        check(!declared.is_empty() && output.actions.len() == declared.len(), "A desktop endpoint needs one exact action definition per typed input port.")?;
        let mut actions = BTreeSet::new();
        for definition in &output.actions {
            check(actions.insert(&definition.port_id), "Duplicate desktop action port.")?;
            definition.action.validate()?;
            let port = declared.iter().find(|port| port.id == definition.port_id).ok_or("Desktop action references a missing exact port.")?;
            check(port.direction == Direction::Input && port.contract == definition.action.contract(), "Desktop input contract does not match its explicit action.")?;
        }
        check(nodes.insert(&output.id, declared).is_none(), "Duplicate graph node identifier.")?;
    }
    check(
        nodes.values().map(Vec::len).sum::<usize>() <= MAX_PORTS,
        "Profile exceeds 1024 ports.",
    )?;
    let mut edges: BTreeMap<String, BTreeSet<String>> = nodes
        .iter()
        .flat_map(|(node, ports)| {
            ports
                .iter()
                .map(move |port| (endpoint_key(node, &port.id), BTreeSet::new()))
        })
        .collect();
    // Only processors create an internal dependency. A duplex device/game does
    // not implicitly feed its action input back into its independent output.
    for node in &document.processing_nodes {
        for input in &node.inputs {
            for output in &node.outputs {
                edges
                    .get_mut(&endpoint_key(&node.id, &input.id))
                    .unwrap()
                    .insert(endpoint_key(&node.id, &output.id));
            }
        }
    }
    let resolve = |endpoint: &Endpoint| -> Result<&Port, String> {
        id(&endpoint.node_id)?;
        check(
            port_identifier(&endpoint.port_id),
            "Invalid graph port endpoint identity.",
        )?;
        nodes
            .get(endpoint.node_id.as_str())
            .and_then(|ports| ports.iter().find(|p| p.id == endpoint.port_id).copied())
            .ok_or_else(|| "Binding references a missing node or port.".into())
    };
    let mut bindings = BTreeSet::new();
    let mut targets = BTreeSet::new();
    for binding in &document.bindings {
        id(&binding.id)?;
        check(
            bindings.insert(&binding.id),
            "Duplicate binding identifier.",
        )?;
        let from = resolve(&binding.from)?;
        let to = resolve(&binding.to)?;
        check(
            from.direction == Direction::Output && to.direction == Direction::Input,
            "Bindings must connect an output to an input.",
        )?;
        check(from.contract.can_bind_to(&to.contract), "Binding requires an explicit converter for different type, delivery, unit, range or action argument contracts.")?;
        let target = endpoint_key(&binding.to.node_id, &binding.to.port_id);
        check(
            targets.insert(target.clone()),
            "An input has multiple writers; use an explicit processing node.",
        )?;
        edges
            .get_mut(&endpoint_key(&binding.from.node_id, &binding.from.port_id))
            .unwrap()
            .insert(target);
    }
    acyclic(&edges)?;
    let resources: BTreeMap<_, _> = document
        .resources
        .iter()
        .map(|r| (r.id.as_str(), r))
        .collect();
    check(
        document.visibility.len() <= MAX_RESOURCES
            && document.overrides.len() <= MAX_RESOURCES
            && document.companion_assignments.len() <= MAX_RESOURCES,
        "Too many visibility, override or companion declarations.",
    )?;
    let device = |resource: &str| -> Result<&Resource, String> {
        resources
            .get(resource)
            .copied()
            .filter(|r| r.kind == ResourceKind::Device)
            .ok_or_else(|| "Device intent references a missing or non-device resource.".into())
    };
    let mut seen = BTreeSet::new();
    for intent in &document.visibility {
        device(&intent.resource_id)?;
        check(
            seen.insert(&intent.resource_id),
            "Duplicate visibility intent.",
        )?;
    }
    seen.clear();
    for intent in &document.overrides {
        device(&intent.resource_id)?;
        check(
            seen.insert(&intent.resource_id),
            "Duplicate runtime override.",
        )?;
    }
    seen.clear();
    for assignment in &document.companion_assignments {
        check(
            assignment.panel_id.as_deref().is_none_or(companion_panel_identifier),
            "Invalid portable companion panel identity.",
        )?;
        check(
            resources
                .get(assignment.resource_id.as_str())
                .is_some_and(|r| r.kind == ResourceKind::Companion),
            "Companion assignment references a missing or non-companion resource.",
        )?;
        check(
            seen.insert(&assignment.resource_id),
            "Duplicate companion assignment.",
        )?;
        check(
            companion_publication_hash(&assignment.publication_hash),
            "Invalid portable companion publication hash.",
        )?;
    }
    check(
        document.editor.positions.len()
            <= MAX_RESOURCES + MAX_PROCESSING_NODES + MAX_VIRTUAL_OUTPUTS,
        "Too many editor positions.",
    )?;
    let coordinate = |value: f64| value.is_finite() && value.abs() <= MAX_COORDINATE;
    let mut positions = BTreeSet::new();
    for position in &document.editor.positions {
        check(
            nodes.contains_key(position.node_id.as_str()) && positions.insert(&position.node_id),
            "Editor position references an unknown or duplicate node.",
        )?;
        check(
            coordinate(position.x) && coordinate(position.y),
            "Editor coordinates are invalid.",
        )?;
    }
    check(document.editor.groups.len() <= 32, "A canvas supports at most 32 editor groups.")?;
    let mut group_ids = BTreeSet::new();
    let mut grouped = BTreeSet::new();
    for group in &document.editor.groups {
        id(&group.id)?;
        label(&group.name)?;
        check(group_ids.insert(&group.id), "Duplicate editor group identifier.")?;
        check(group.members.len() <= MAX_RESOURCES + MAX_PROCESSING_NODES + MAX_VIRTUAL_OUTPUTS,
            "An editor group supports at most 336 known member nodes.")?;
        if let Some(bounds) = &group.bounds {
            check(coordinate(bounds.x) && coordinate(bounds.y) && bounds.width.is_finite() && bounds.height.is_finite()
                && (328.0..=2_001_000.0).contains(&bounds.width) && (300.0..=2_001_000.0).contains(&bounds.height),
                "Editor group frame is invalid.")?;
        }
        for member in &group.members {
            check(nodes.contains_key(member.as_str()) && grouped.insert(member),
                "Editor group member is unknown or belongs to more than one group.")?;
        }
    }
    let frames: Vec<_> = document.editor.groups.iter().filter_map(|group|group.bounds.as_ref()).collect();
    for (index, bounds) in frames.iter().enumerate() {
        check(!frames.iter().skip(index+1).any(|other|bounds.x < other.x+other.width && bounds.x+bounds.width > other.x
            && bounds.y < other.y+other.height && bounds.y+bounds.height > other.y), "Editor groups cannot overlap.")?;
    }
    check(document.editor.collapsed_port_groups.len() <= 3 * (MAX_RESOURCES + MAX_PROCESSING_NODES + MAX_VIRTUAL_OUTPUTS),
        "Too many collapsed editor port groups.")?;
    let mut collapsed = BTreeSet::new();
    for category in &document.editor.collapsed_port_groups {
        check(nodes.contains_key(category.node_id.as_str()) && collapsed.insert(category),
            "Collapsed editor port group is unknown or duplicated.")?;
    }
    let v = &document.editor.viewport;
    check(
        coordinate(v.x) && coordinate(v.y) && v.zoom.is_finite() && (0.1..=4.0).contains(&v.zoom),
        "Editor viewport is invalid.",
    )?;
    Ok(())
}

/// Content identity ignores names, local document identity, revision and editor
/// layout. Unordered declarations are canonicalized before hashing.
pub fn semantic_hash(document: &ProfileDocument) -> Result<String, String> {
    validate_document(document)?;
    let mut value = serde_json::to_value(document).map_err(|e| e.to_string())?;
    let object = value.as_object_mut().unwrap();
    for field in ["id", "name", "revision", "editor"] {
        object.remove(field);
    }
    fn canonical(value: &mut serde_json::Value, field: &str, declaration: bool) {
        match value {
            serde_json::Value::Object(object) => {
                if declaration {
                    object.remove("name");
                }
                let mut sorted: BTreeMap<String, serde_json::Value> =
                    std::mem::take(object).into_iter().collect();
                for (key, value) in &mut sorted {
                    canonical(value, key, false);
                }
                object.extend(sorted);
            }
            serde_json::Value::Array(array) => {
                for value in array.iter_mut() {
                    canonical(
                        value,
                        field,
                        matches!(
                            field,
                            "resources"
                                | "processingNodes"
                                | "virtualOutputs"
                                | "desktopOutputs"
                                | "ports"
                                | "inputs"
                                | "outputs"
                                | "parameters"
                        ),
                    );
                }
                if matches!(
                    field,
                    "resources"
                        | "bindings"
                        | "processingNodes"
                        | "virtualOutputs"
                        | "desktopOutputs"
                        | "ports"
                        | "inputs"
                        | "outputs"
                ) {
                    array.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
                } else if matches!(field, "visibility" | "overrides" | "companionAssignments") {
                    array.sort_by(|a, b| a["resourceId"].as_str().cmp(&b["resourceId"].as_str()));
                } else if field == "actions" {
                    array.sort_by(|a, b| a["portId"].as_str().cmp(&b["portId"].as_str()));
                } else if matches!(field, "gameIds") {
                    array.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
                }
            }
            _ => {}
        }
    }
    canonical(&mut value, "", false);
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&value).map_err(|e| e.to_string())?)
    ))
}

#[cfg(test)]
#[path = "validation_tests.rs"]
mod tests;
