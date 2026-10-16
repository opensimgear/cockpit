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
fn numeric(kind: ValueType) -> bool {
    matches!(kind, ValueType::Integer | ValueType::Number)
}
fn scalar(port: &Port, value: &PortValue) -> bool {
    match (port.value_type, value) {
        (ValueType::Boolean, PortValue::Boolean(_)) => true,
        (ValueType::Integer | ValueType::Number, PortValue::Number(value)) => {
            value.is_finite()
                && (port.value_type != ValueType::Integer || value.fract() == 0.0)
                && port
                    .range
                    .as_ref()
                    .is_some_and(|r| *value >= r.min && *value <= r.max)
        }
        (ValueType::Text, PortValue::Text(value)) => {
            value.chars().count() <= 256 && !value.chars().any(char::is_control)
        }
        (ValueType::Enum, PortValue::Text(value)) => port.enum_values.contains(value),
        _ => false,
    }
}
fn port(port: &Port) -> Result<(), String> {
    id(&port.id)?;
    label(&port.name)?;
    if let Some(unit) = &port.unit {
        check(
            !unit.is_empty()
                && unit.chars().count() <= 32
                && unit.trim() == unit
                && !unit.chars().any(char::is_control),
            "Invalid signal unit.",
        )?;
        check(
            numeric(port.value_type),
            "Only numeric ports can declare a unit.",
        )?;
    }
    if numeric(port.value_type) {
        let r = port
            .range
            .as_ref()
            .ok_or("Numeric ports require an explicit range.")?;
        check(
            r.min.is_finite()
                && r.max.is_finite()
                && r.min <= r.max
                && r.min.abs() <= MAX_REVISION as f64
                && r.max.abs() <= MAX_REVISION as f64,
            "Invalid numeric port range.",
        )?;
        if port.value_type == ValueType::Integer {
            check(
                r.min.fract() == 0.0 && r.max.fract() == 0.0,
                "Integer ranges require integer endpoints.",
            )?;
        }
    } else {
        check(
            port.range.is_none(),
            "Only numeric ports can declare a range.",
        )?;
    }
    if port.value_type == ValueType::Enum {
        check(
            !port.enum_values.is_empty() && port.enum_values.len() <= 64,
            "Enum ports require 1-64 values.",
        )?;
        let mut values = BTreeSet::new();
        for value in &port.enum_values {
            label(value)?;
            check(values.insert(value), "Duplicate enum value.")?;
        }
    } else {
        check(
            port.enum_values.is_empty(),
            "Only enum ports can declare enum values.",
        )?;
    }
    match port.delivery {
        Delivery::State => {
            check(
                port.freshness_ms
                    .is_some_and(|v| (1..=600_000).contains(&v)),
                "State ports require freshness of 1-600000 ms.",
            )?;
            let fallback = port
                .fallback
                .as_ref()
                .ok_or("State ports require an explicit fallback.")?;
            check(
                scalar(port, fallback),
                "Fallback does not match the port type or range.",
            )?;
        }
        Delivery::Event | Delivery::Action => {
            check(
                port.freshness_ms.is_none() && port.fallback.is_none(),
                "Events and actions cannot declare state freshness or fallback.",
            )?;
        }
    }
    Ok(())
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
fn same_signal(a: &Port, b: &Port) -> bool {
    a.value_type == b.value_type
        && a.unit == b.unit
        && a.range == b.range
        && a.enum_values.iter().collect::<BTreeSet<_>>()
            == b.enum_values.iter().collect::<BTreeSet<_>>()
}
fn operation(node: &ProcessingNode) -> Result<(), String> {
    check(
        node.inputs.len() == 1 && node.outputs.len() == 1,
        "Processing operations require exactly one input and one output.",
    )?;
    let input = &node.inputs[0];
    let output = &node.outputs[0];
    check(
        input.direction == Direction::Input && output.direction == Direction::Output,
        "Processing port direction does not match its input/output declaration.",
    )?;
    match node.operation {
        Operation::Identity {} => check(
            input.delivery == output.delivery && same_signal(input, output),
            "Identity must preserve the complete signal contract.",
        )?,
        Operation::Not {} => check(
            input.value_type == ValueType::Boolean
                && output.value_type == ValueType::Boolean
                && input.delivery == output.delivery,
            "Not requires matching boolean delivery contracts.",
        )?,
        Operation::Scale { factor, offset } => {
            check(
                factor.is_finite()
                    && offset.is_finite()
                    && numeric(input.value_type)
                    && input.value_type == output.value_type
                    && input.delivery == output.delivery,
                "Scale requires finite coefficients and matching numeric type and delivery.",
            )?;
            if input.value_type == ValueType::Integer {
                check(
                    factor.fract() == 0.0 && offset.fract() == 0.0,
                    "Integer scale coefficients must be integral.",
                )?;
            }
            let a = input.range.as_ref().unwrap();
            let b = output.range.as_ref().unwrap();
            let low = (a.min * factor + offset).min(a.max * factor + offset);
            let high = (a.min * factor + offset).max(a.max * factor + offset);
            check(
                low.is_finite() && high.is_finite() && low >= b.min && high <= b.max,
                "Scale results exceed the declared output range.",
            )?;
        }
        Operation::Threshold { threshold } => check(
            threshold.is_finite()
                && numeric(input.value_type)
                && output.value_type == ValueType::Boolean
                && input.delivery == Delivery::State
                && output.delivery == Delivery::State,
            "Threshold requires a numeric state input and boolean state output.",
        )?,
        Operation::Toggle {} => check(
            input.value_type == ValueType::Boolean
                && output.value_type == ValueType::Boolean
                && input.delivery == Delivery::Event
                && output.delivery == Delivery::State,
            "Toggle explicitly converts boolean events to boolean state.",
        )?,
        Operation::Trigger {} => check(
            input.delivery == Delivery::Event
                && output.delivery == Delivery::Action
                && same_signal(input, output),
            "Trigger explicitly converts an event into an action with the same value contract.",
        )?,
    }
    Ok(())
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
        document.schema_version == 1,
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
                .all(|p| p.delivery != Delivery::Action || p.direction == Direction::Input),
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
        check(
            !output.ports.is_empty()
                && output.ports.iter().all(|p| {
                    p.direction == Direction::Input
                        && p.delivery == Delivery::State
                        && matches!(
                            p.value_type,
                            ValueType::Boolean | ValueType::Integer | ValueType::Number
                        )
                }),
            "Virtual outputs require numeric or boolean state input ports.",
        )?;
        check(
            nodes.insert(&output.id, ports(&output.ports)?).is_none(),
            "Duplicate graph node identifier.",
        )?;
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
        id(&endpoint.port_id)?;
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
        check(from.delivery == to.delivery && from.value_type == to.value_type && from.unit == to.unit && from.enum_values.iter().collect::<BTreeSet<_>>() == to.enum_values.iter().collect::<BTreeSet<_>>(), "Binding requires an explicit converter for different type, delivery, unit or enum contracts.")?;
        if let (Some(a), Some(b)) = (&from.range, &to.range) {
            check(
                a.min >= b.min && a.max <= b.max,
                "Binding source range does not fit the consumer; add an explicit processor.",
            )?;
        }
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
        let target = device(&intent.resource_id)?;
        check(
            seen.insert(&intent.resource_id),
            "Duplicate runtime override.",
        )?;
        check(
            intent.enabled.is_some()
                || intent.debounce_ms.is_some()
                || !intent.invert_ports.is_empty(),
            "Runtime override is empty.",
        )?;
        check(
            intent.debounce_ms.is_none_or(|v| v <= 60_000)
                && intent.invert_ports.len() <= MAX_PORTS_PER_NODE,
            "Runtime override exceeds its limits.",
        )?;
        let mut inverted = BTreeSet::new();
        for key in &intent.invert_ports {
            check(inverted.insert(key), "Duplicate inverted port.")?;
            check(
                target.ports.iter().any(|p| {
                    &p.id == key
                        && p.direction == Direction::Output
                        && p.delivery == Delivery::State
                        && numeric(p.value_type)
                }),
                "Inversion requires an existing numeric device state output.",
            )?;
        }
    }
    seen.clear();
    for assignment in &document.companion_assignments {
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
            assignment
                .publication_hash
                .strip_prefix("sha256:")
                .is_some_and(|v| {
                    v.len() == 64
                        && v.bytes()
                            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                }),
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
    fn canonical(value: &mut serde_json::Value, field: &str) {
        match value {
            serde_json::Value::Object(object) => {
                object.remove("name");
                let mut sorted: BTreeMap<String, serde_json::Value> =
                    std::mem::take(object).into_iter().collect();
                for (key, value) in &mut sorted {
                    canonical(value, key);
                }
                object.extend(sorted);
            }
            serde_json::Value::Array(array) => {
                for value in array.iter_mut() {
                    canonical(value, "");
                }
                if matches!(
                    field,
                    "resources"
                        | "bindings"
                        | "processingNodes"
                        | "virtualOutputs"
                        | "ports"
                        | "inputs"
                        | "outputs"
                ) {
                    array.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
                } else if matches!(field, "visibility" | "overrides" | "companionAssignments") {
                    array.sort_by(|a, b| a["resourceId"].as_str().cmp(&b["resourceId"].as_str()));
                } else if matches!(field, "gameIds" | "enumValues" | "invertPorts") {
                    array.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
                }
            }
            _ => {}
        }
    }
    canonical(&mut value, "");
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&value).map_err(|e| e.to_string())?)
    ))
}

#[cfg(test)]
#[path = "validation_tests.rs"]
mod tests;
