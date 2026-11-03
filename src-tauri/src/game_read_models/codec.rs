use super::pages::Projection;
use serde_json::{json, Map, Value};

const INLINE_BYTES: usize = 8192;
const TEXT_BYTES: usize = 4096;
const PAGE_BYTES: usize = 65536;
const KEY_BYTES: usize = 4096;
const DEPTH: usize = 64;

pub(super) fn encode(projection: &mut Projection<'_>, value: &Value) -> Result<Value, String> {
    encode_at(projection, value, 0)
}

fn encode_at(projection: &mut Projection<'_>, value: &Value, depth: usize) -> Result<Value, String> {
    if depth > DEPTH { return Err("Game JSON exceeds the nesting limit".into()); }
    if projection.retain_node(value)? { return Ok(value.clone()); }
    match value {
        Value::String(text) if text.len() > TEXT_BYTES || size(value)? > INLINE_BYTES => projection.text(text),
        Value::Array(items) => {
            let encoded = items.iter().map(|item| encode_at(projection, item, depth + 1))
                .collect::<Result<Vec<_>, _>>()?;
            let inline = Value::Array(encoded);
            if size(&inline)? <= INLINE_BYTES { return Ok(inline); }
            let Value::Array(encoded) = inline else { unreachable!() };
            collection(projection, "array", encoded)
        }
        Value::Object(entries) => {
            let mut encoded = Map::new();
            for (key, value) in entries {
                if key.len() > KEY_BYTES { return Err("Game JSON key exceeds the size limit".into()); }
                encoded.insert(key.clone(), encode_at(projection, value, depth + 1)?);
            }
            let inline = Value::Object(encoded);
            if size(&inline)? <= INLINE_BYTES { return Ok(inline); }
            let Value::Object(encoded) = inline else { unreachable!() };
            let entries = encoded.into_iter().map(|(key, value)| json!([key, value])).collect();
            collection(projection, "object", entries)
        }
        _ => Ok(value.clone()),
    }
}

fn size(value: &Value) -> Result<usize, String> {
    serde_json::to_vec(value).map(|bytes| bytes.len()).map_err(|e| e.to_string())
}

fn payload(kind: &str, branch: bool, nodes: Vec<Value>) -> Value {
    if branch { json!({"type":"branch","chunks":nodes}) }
    else if kind == "array" { json!({"type":"leaf","items":nodes}) }
    else { json!({"type":"leaf","entries":nodes}) }
}

fn collection(projection: &mut Projection<'_>, kind: &'static str, nodes: Vec<Value>) -> Result<Value, String> {
    let mut pages = chunks(projection, kind, false, nodes)?;
    while pages.len() > 1 { pages = chunks(projection, kind, true, pages)?; }
    pages.pop().ok_or_else(|| "Cannot page an empty game collection".into())
}

fn chunks(projection: &mut Projection<'_>, kind: &'static str, branch: bool, nodes: Vec<Value>) -> Result<Vec<Value>, String> {
    let overhead = size(&payload(kind, branch, Vec::new()))?;
    let mut used = overhead;
    let mut current = Vec::new();
    let mut pages = Vec::new();
    for node in nodes {
        let node_bytes = size(&node)?;
        if overhead + node_bytes > PAGE_BYTES { return Err("Game collection entry exceeds page limit".into()); }
        let separator = usize::from(!current.is_empty());
        if used + separator + node_bytes > PAGE_BYTES {
            pages.push(projection.page(kind, payload(kind, branch, std::mem::take(&mut current)))?);
            used = overhead;
        }
        used += usize::from(!current.is_empty()) + node_bytes;
        current.push(node);
    }
    if !current.is_empty() { pages.push(projection.page(kind, payload(kind, branch, current))?); }
    Ok(pages)
}

#[cfg(test)]
#[path = "codec_tests.rs"]
mod tests;
