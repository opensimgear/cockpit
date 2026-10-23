//! Independent native admission of authoring data. The lowered runtime contract
//! is validated here; raster compilation remains on the host compiler.
use super::*;

fn rows(value: &Value, maximum: usize) -> Result<&Vec<Value>, String> {
    let rows = value.as_array().ok_or("Expected design list")?;
    ensure(rows.len() <= maximum, "Design list exceeds bounds")?;
    Ok(rows)
}
fn map(value: &Value) -> Result<&serde_json::Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| "Expected design object".into())
}
fn expression(value: &Value) -> Value {
    if value.is_object() {
        value.clone()
    } else {
        json!({"literal":value})
    }
}
fn expand(
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
fn condition(regions: &Value) -> Result<Value, String> {
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
fn get_layer<'a>(scene: &'a Value, id: &str) -> Option<&'a Value> {
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
fn layer_mut<'a>(scene: &'a mut Value, id: &str) -> Option<&'a mut Value> {
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
fn property(node: &Value, key: &str) -> Value {
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
fn valid_property(node: &Value, key: &str) -> bool {
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
fn includes(a: &Value, b: &Value) -> bool {
    b.as_object()
        .is_some_and(|b| b.iter().all(|(k, v)| a.get(k) == Some(v)))
}
fn motion_expression(value: &Value, parameters: &Value, region: &Value) -> Result<Value, String> {
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

pub(super) fn lower(input: &Value) -> Result<Value, String> {
    object(
        input,
        &[
            "id",
            "title",
            "size",
            "parameters",
            "behavior",
            "visualDomains",
            "inputs",
            "outputs",
            "hitRegions",
            "scene",
            "design",
        ],
        &["motions", "sounds", "assets"],
    )?;
    let design = &input["design"];
    object(design, &["nodes", "owners", "appearances", "effects"], &[])?;
    let signals = rows(&input["behavior"]["signals"], 32)?;
    let regions = rows(&input["behavior"]["regions"], 8)?;
    let mut nodes = BTreeMap::new();
    for node in rows(&design["nodes"], 64)? {
        object(node, &["id", "label", "domain", "expression"], &[])?;
        let id = stable(&node["id"])?;
        string(&node["label"], 120)?;
        ensure(
            !signals.iter().any(|s| s["id"] == id)
                && !rows(&input["parameters"], 32)?
                    .iter()
                    .any(|p| p["id"] == id)
                && !nodes.contains_key(id),
            "Duplicate graph ID",
        )?;
        let optional = match node["domain"]["type"].as_str() {
            Some("number") => vec!["min", "max"],
            Some("enum") => vec!["values"],
            _ => vec![],
        };
        object(&node["domain"], &["type"], &optional)?;
        nodes.insert(id.to_string(), node.clone());
    }
    for node in nodes.values() {
        let expanded = expand(&node["expression"], &nodes, &mut BTreeSet::new(), &mut 0, 0)?;
        let mut probe = input["behavior"].clone();
        probe["events"] = json!([{"id":"probe"}]);
        probe
            .as_object_mut()
            .ok_or("Invalid behavior")?
            .remove("timers");
        let mut output = node["domain"].clone();
        output["id"] = json!("probe");
        output["neutral"] = match output["type"].as_str() {
            Some("boolean") => json!(false),
            Some("number") => output["min"].clone(),
            _ => output["values"][0].clone(),
        };
        probe["outputs"] = json!([output]);
        for region in probe["regions"].as_array_mut().ok_or("Invalid regions")? {
            region["transitions"] = json!([]);
        }
        if probe["regions"].as_array().unwrap().is_empty() {
            probe["regions"] =
                json!([{"id":"graph-probe","initial":"ready","states":["ready"],"transitions":[]}]);
        }
        probe["regions"][0]["transitions"] = json!([{"id":"probe","from":probe["regions"][0]["initial"],"event":"probe","actions":[{"type":"emit","port":"probe","value":expanded}]}]);
        compiled::validate_behavior(&probe)?;
    }
    let mut result = input.clone();
    result.as_object_mut().unwrap().remove("design");
    result["behavior"] = expand(&input["behavior"], &nodes, &mut BTreeSet::new(), &mut 0, 0)?;
    result["scene"] = expand(&input["scene"], &nodes, &mut BTreeSet::new(), &mut 0, 0)?;
    let mut owned_signals = BTreeSet::new();
    let mut owned_regions = BTreeSet::new();
    let mut owned_ports = BTreeSet::new();
    for owner in rows(&design["owners"], 8)? {
        object(owner, &["region", "signal", "values"], &["port"])?;
        let region = regions
            .iter()
            .find(|r| r["id"] == owner["region"])
            .ok_or("Unknown owner region")?;
        let signal = signals
            .iter()
            .find(|s| s["id"] == owner["signal"])
            .ok_or("Unknown owner signal")?;
        ensure(
            signal["source"] == "local"
                && owned_signals.insert(stable(&owner["signal"])?)
                && owned_regions.insert(stable(&owner["region"])?),
            "Duplicate or invalid state owner",
        )?;
        let states = rows(&region["states"], 32)?;
        let state_keys = states
            .iter()
            .map(|v| v.as_str().ok_or("Invalid state"))
            .collect::<Result<Vec<_>, _>>()?;
        object(&owner["values"], &state_keys, &[])?;
        for value in map(&owner["values"])?.values() {
            ensure(accepts(signal, value), "Owner value outside domain")?;
        }
        ensure(
            owner["values"][region["initial"].as_str().ok_or("Invalid initial")?]
                == signal["initial"],
            "Owner initial mismatch",
        )?;
        for r in rows(&result["behavior"]["regions"], 8)? {
            for t in rows(&r["transitions"], 128)? {
                if let Some(actions) = t.get("actions") {
                    for a in rows(actions, 8)? {
                        ensure(
                            !(a["type"] == "set" && a["signal"] == owner["signal"])
                                && !(owner.get("port").is_some()
                                    && a["type"] == "emit"
                                    && a["port"] == owner["port"]),
                            "State owner has a second writer",
                        )?;
                    }
                }
            }
        }
        if let Some(port) = owner.get("port") {
            ensure(
                owned_ports.insert(stable(port)?)
                    && rows(&input["inputs"], 16)?
                        .iter()
                        .any(|p| p["id"] == *port && p["signal"] == owner["signal"]),
                "Owner port mismatch",
            )?;
        }
        let target = result["behavior"]["regions"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r["id"] == owner["region"])
            .unwrap();
        for transition in target["transitions"]
            .as_array_mut()
            .ok_or("Invalid transitions")?
        {
            if let Some(to) = transition
                .get("to")
                .and_then(Value::as_str)
                .map(str::to_string)
            {
                let value = owner["values"][to].clone();
                if transition.get("actions").is_none() {
                    transition["actions"] = json!([]);
                }
                let actions = transition["actions"]
                    .as_array_mut()
                    .ok_or("Invalid actions")?;
                actions
                    .push(json!({"type":"set","signal":owner["signal"],"value":{"literal":value}}));
                if let Some(port) = owner.get("port") {
                    actions.push(json!({"type":"emit","port":port,"value":{"literal":value}}));
                }
            }
        }
        let last = states
            .last()
            .and_then(Value::as_str)
            .ok_or("Owner needs states")?;
        let mut owned_expression = json!({"literal":owner["values"][last]});
        for state in states[..states.len() - 1].iter().rev() {
            let id = state.as_str().ok_or("Invalid state")?;
            owned_expression = json!({"op":"if","condition":{"op":"eq","left":{"state":owner["region"]},"right":{"literal":state}},"then":{"literal":owner["values"][id]},"else":owned_expression});
        }
        let replacements = BTreeMap::from([(
            stable(&owner["signal"])?.to_string(),
            json!({"expression":owned_expression}),
        )]);
        result["scene"] = expand(
            &result["scene"],
            &replacements,
            &mut BTreeSet::new(),
            &mut 0,
            0,
        )?;
    }
    let mut appearances = rows(&design["appearances"], 64)?.clone();
    appearances.sort_by_key(|a| {
        (
            a["regions"].as_object().map_or(0, |v| v.len()),
            a["id"].as_str().unwrap_or("").to_string(),
        )
    });
    let mut ids = BTreeSet::new();
    for appearance in &appearances {
        object(appearance, &["id", "regions", "layers"], &[])?;
        ensure(
            ids.insert(stable(&appearance["id"])?),
            "Duplicate appearance ID",
        )?;
        for (id, state) in map(&appearance["regions"])? {
            ensure(
                regions.iter().any(|r| {
                    r["id"] == *id && r["states"].as_array().is_some_and(|v| v.contains(state))
                }),
                "Unknown appearance state",
            )?;
        }
        let selector = condition(&appearance["regions"])?;
        ensure(
            map(&appearance["layers"])?.len() <= 128,
            "Too many appearance layers",
        )?;
        for (id, patch) in map(&appearance["layers"])? {
            let node = layer_mut(&mut result["scene"], id).ok_or("Missing appearance layer")?;
            for (key, value) in map(patch)? {
                ensure(valid_property(node, key), "Invalid appearance property")?;
                for other in &appearances {
                    if other["id"] == appearance["id"] || other["layers"][id].get(key).is_none() {
                        continue;
                    }
                    let overlap = map(&appearance["regions"])?
                        .iter()
                        .all(|(k, v)| other["regions"].get(k).is_none_or(|o| o == v));
                    ensure(
                        !overlap || appearance["regions"] != other["regions"],
                        "Duplicate appearance selector writer",
                    )?;
                    if overlap
                        && !includes(&appearance["regions"], &other["regions"])
                        && !includes(&other["regions"], &appearance["regions"])
                    {
                        let mut combination = appearance["regions"].clone();
                        for (k, v) in map(&other["regions"])? {
                            combination[k] = v.clone();
                        }
                        ensure(
                            appearances.iter().any(|c| {
                                includes(&c["regions"], &combination)
                                    && c["layers"][id].get(key).is_some()
                            }),
                            "Conflicting appearances require combination override",
                        )?;
                    }
                }
                let changed = json!({"op":"if","condition":selector,"then":expand(&expression(value),&nodes,&mut BTreeSet::new(),&mut 0,0)?,"else":expression(&property(node,key))});
                if let Some(part) = key.strip_prefix("transform.") {
                    if node.get("transform").is_none() {
                        node["transform"] = json!({});
                    }
                    node["transform"][part] = changed;
                } else {
                    node[key] = changed;
                }
            }
        }
    }
    let mut effects = BTreeSet::new();
    for effect in rows(&design["effects"], 32)? {
        object(
            effect,
            &["region", "transition", "durationMs", "fps", "properties"],
            &["easing", "interruption", "cue"],
        )?;
        let region = regions
            .iter()
            .find(|r| r["id"] == effect["region"])
            .ok_or("Unknown effect region")?;
        let transition = rows(&region["transitions"], 128)?
            .iter()
            .find(|t| t["id"] == effect["transition"])
            .ok_or("Unknown effect transition")?;
        ensure(
            transition.get("to").is_some() && transition["from"] != "*",
            "Effect requires explicit endpoints",
        )?;
        let id = format!(
            "{}-{}",
            stable(&effect["region"])?,
            stable(&effect["transition"])?
        );
        ensure(effects.insert(id.clone()), "Duplicate transition effect")?;
        integer(&effect["durationMs"], 1, 60000)?;
        integer(&effect["fps"], 1, 60)?;
        ensure(
            effect.get("easing").is_none_or(|v| {
                ["linear", "ease-in", "ease-out", "ease-in-out"]
                    .iter()
                    .any(|s| v == s)
            }) && effect
                .get("interruption")
                .is_none_or(|v| ["reverse", "restart", "snap"].iter().any(|s| v == s)),
            "Invalid effect playback rule",
        )?;
        let mut tracks = Vec::new();
        let mut track_ids = BTreeSet::new();
        for track in rows(&effect["properties"], 64)? {
            object(track, &["node", "property"], &[])?;
            let node = get_layer(&result["scene"], stable(&track["node"])?)
                .ok_or("Missing motion layer")?;
            let key = string(&track["property"], 64)?;
            ensure(
                valid_property(node, key)
                    && !["visible", "fill", "stroke", "text", "d"].contains(&key),
                "Motion requires numeric property",
            )?;
            ensure(
                track_ids.insert(format!("{}.{}", track["node"], key)),
                "Duplicate automatic track",
            )?;
            let region_id = stable(&effect["region"])?;
            ensure(
                !result["motions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|motion| {
                        motion["tracks"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .any(|other| {
                                other["node"] == track["node"]
                                    && other["property"] == track["property"]
                            })
                            && motion["fromRegions"]
                                .get(region_id)
                                .is_none_or(|value| value == &transition["from"])
                            && motion["toRegions"]
                                .get(region_id)
                                .is_none_or(|value| value == &transition["to"])
                    }),
                "Automatic and explicit motion tracks conflict",
            )?;
            let value = motion_expression(
                &expression(&property(node, key)),
                &input["parameters"],
                &effect["region"],
            )?;
            let mut probe = json!({"version":1,"signals":[],"events":[{"id":"probe"}],"outputs":[{"id":"probe","type":"number","min":-1000000,"max":1000000,"neutral":0}],"regions":[{"id":effect["region"],"initial":region["initial"],"states":region["states"],"transitions":[]}]});
            probe["regions"][0]["transitions"] = json!([{"id":"probe","from":region["initial"],"event":"probe","actions":[{"type":"emit","port":"probe","value":value}]}]);
            compiled::validate_behavior(&probe)?;
            let mut t = json!({"node":track["node"],"property":key,"keyframes":[{"at":0,"value":0},{"at":1,"value":1}]});
            if let Some(easing) = effect.get("easing") {
                t["easing"] = easing.clone();
            }
            tracks.push(t);
        }
        if !tracks.is_empty() {
            let mut motion = json!({"id":id,"from":{},"to":{},"fromRegions":{stable(&effect["region"])?:transition["from"]},"toRegions":{stable(&effect["region"])?:transition["to"]},"durationMs":effect["durationMs"],"fps":effect["fps"],"tracks":tracks});
            if let Some(rule) = effect.get("interruption") {
                motion["interruption"] = rule.clone();
            }
            if result.get("motions").is_none() {
                result["motions"] = json!([]);
            }
            result["motions"]
                .as_array_mut()
                .ok_or("Invalid motions")?
                .push(motion);
        }
        if let Some(cue) = effect.get("cue") {
            stable(cue)?;
            let target = result["behavior"]["regions"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|r| r["id"] == effect["region"])
                .unwrap()["transitions"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|t| t["id"] == effect["transition"])
                .unwrap();
            if target.get("actions").is_none() {
                target["actions"] = json!([]);
            }
            target["actions"]
                .as_array_mut()
                .ok_or("Invalid actions")?
                .push(json!({"type":"cue","cue":cue}));
        }
    }
    Ok(result)
}
