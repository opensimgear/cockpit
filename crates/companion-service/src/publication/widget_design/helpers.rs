//! Bounded expression expansion and artwork property access for authoring admission.
use super::*;

pub(super) fn rows(value: &Value, maximum: usize) -> Result<&Vec<Value>, String> {
    let rows = value.as_array().ok_or("Expected design list")?;
    ensure(rows.len() <= maximum, "Design list exceeds bounds")?;
    Ok(rows)
}
pub(super) fn map(value: &Value) -> Result<&serde_json::Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| "Expected design object".into())
}
pub(super) fn expression(value: &Value) -> Value {
    if value.is_object() {
        value.clone()
    } else {
        json!({"literal":value})
    }
}
pub(super) fn expand(
    value: &Value,
    nodes: &BTreeMap<String, Value>,
    visiting: &mut BTreeSet<String>,
    budget: &mut usize,
    depth: usize,
) -> Result<Value, String> {
    *budget += 1;
    ensure(
        *budget <= 32768 && depth < 32,
        "Graph expansion exceeds work bounds",
    )?;
    if let Some(id) = value.get("ref").and_then(Value::as_str) {
        if value.as_object().is_some_and(|v| v.len() == 1) {
            if let Some(node) = nodes.get(id) {
                ensure(visiting.insert(id.into()), "Signal graph contains a cycle")?;
                let result = expand(&node["expression"], nodes, visiting, budget, depth + 1)?;
                visiting.remove(id);
                return Ok(result);
            }
        }
    }
    Ok(match value {
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|v| expand(v, nodes, visiting, budget, depth + 1))
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(k, v)| Ok((k.clone(), expand(v, nodes, visiting, budget, depth + 1)?)))
                .collect::<Result<_, String>>()?,
        ),
        _ => value.clone(),
    })
}
pub(super) fn condition(regions: &Value) -> Result<Value, String> {
    let values = map(regions)?
        .iter()
        .map(|(id, state)| json!({"op":"eq","left":{"state":id},"right":{"literal":state}}))
        .collect::<Vec<_>>();
    ensure(!values.is_empty(), "Appearance needs a selector")?;
    Ok(if values.len() == 1 {
        values[0].clone()
    } else {
        json!({"op":"and","values":values})
    })
}
pub(super) fn get_layer<'a>(scene: &'a Value, id: &str) -> Option<&'a Value> {
    for node in scene.as_array()? {
        if node["id"] == id {
            return Some(node);
        }
        if node["kind"] == "group" {
            if let Some(found) = get_layer(&node["children"], id) {
                return Some(found);
            }
        }
    }
    None
}
pub(super) fn layer_mut<'a>(scene: &'a mut Value, id: &str) -> Option<&'a mut Value> {
    for node in scene.as_array_mut()? {
        if node["id"] == id {
            return Some(node);
        }
        if node["kind"] == "group" {
            if let Some(found) = layer_mut(&mut node["children"], id) {
                return Some(found);
            }
        }
    }
    None
}
pub(super) fn property(node: &Value, key: &str) -> Value {
    let value = if let Some(part) = key.strip_prefix("transform.") {
        &node["transform"][part]
    } else {
        &node[key]
    };
    if !value.is_null() {
        value.clone()
    } else if key == "visible" {
        json!(true)
    } else if key == "opacity" || key.starts_with("transform.scale") {
        json!(1)
    } else if key == "fill" || key == "stroke" {
        json!("none")
    } else {
        json!(0)
    }
}
pub(super) fn valid_property(node: &Value, key: &str) -> bool {
    if let Some(part) = key.strip_prefix("transform.") {
        return [
            "translateX",
            "translateY",
            "rotate",
            "scaleX",
            "scaleY",
            "originX",
            "originY",
        ]
        .contains(&part);
    }
    ["visible", "opacity", "fill", "stroke", "strokeWidth"].contains(&key)
        || ([
            "x", "y", "x2", "y2", "width", "height", "radius", "text", "d", "fontSize",
        ]
        .contains(&key)
            && (node.get(key).is_some() || key == "radius" && node["kind"] == "rectangle"))
}
pub(super) fn includes(a: &Value, b: &Value) -> bool {
    b.as_object()
        .is_some_and(|b| b.iter().all(|(k, v)| a.get(k) == Some(v)))
}
pub(super) fn motion_expression(value: &Value, parameters: &Value, region: &Value) -> Result<Value, String> {
    if let Some(id) = value.get("ref") {
        let parameter = rows(parameters, 32)?
            .iter()
            .find(|p| p["id"] == *id)
            .ok_or("Automatic motion depends on live signal")?;
        return Ok(json!({"literal":parameter["default"]}));
    }
    if let Some(id) = value.get("state") {
        ensure(id == region, "Automatic motion depends on another region")?;
    }
    Ok(match value {
        Value::Array(v) => Value::Array(
            v.iter()
                .map(|v| motion_expression(v, parameters, region))
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(v) => Value::Object(
            v.iter()
                .map(|(k, v)| Ok((k.clone(), motion_expression(v, parameters, region)?)))
                .collect::<Result<_, String>>()?,
        ),
        _ => value.clone(),
    })
}
