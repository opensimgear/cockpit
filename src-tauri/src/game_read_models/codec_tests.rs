use super::*;
use crate::game_read_models::pages::{Limits, Pages};
use std::sync::Arc;

fn raw(pages: &Pages, node: &Value) -> String {
    let id = node["$gamePage"]["id"].as_str().unwrap();
    let mut text = String::new();
    let mut offset = 0;
    loop {
        let result = pages.get(id, offset).unwrap();
        text.push_str(result["chunk"].as_str().unwrap());
        match result["nextOffset"].as_u64() { Some(next) => offset = next, None => return text }
    }
}

fn expand(pages: &Pages, value: &Value) -> Value {
    if let Some(reference) = value.get("$gamePage") {
        let text = raw(pages, value);
        return match reference["kind"].as_str().unwrap() {
            "text" | "asset" => Value::String(text),
            kind => {
                assert!(text.len() <= PAGE_BYTES);
                let payload: Value = serde_json::from_str(&text).unwrap();
                if kind == "array" {
                    let nodes = if payload["type"] == "branch" { "chunks" } else { "items" };
                    let mut restored = Vec::new();
                    for node in payload[nodes].as_array().unwrap() {
                        let decoded = expand(pages, node);
                        if nodes == "chunks" { restored.extend(decoded.as_array().unwrap().iter().cloned()); }
                        else { restored.push(decoded); }
                    }
                    Value::Array(restored)
                } else {
                    let mut restored = Map::new();
                    if payload["type"] == "branch" {
                        for node in payload["chunks"].as_array().unwrap() {
                            restored.extend(expand(pages, node).as_object().unwrap().clone());
                        }
                    } else {
                        for entry in payload["entries"].as_array().unwrap() {
                            restored.insert(entry[0].as_str().unwrap().into(), expand(pages, &entry[1]));
                        }
                    }
                    Value::Object(restored)
                }
            }
        };
    }
    match value {
        Value::Array(items) => Value::Array(items.iter().map(|item| expand(pages, item)).collect()),
        Value::Object(entries) => Value::Object(entries.iter().map(|(key, value)| (key.clone(), expand(pages, value))).collect()),
        _ => value.clone(),
    }
}

#[test]
fn valid_profile_over_three_mib_and_collection_over_1024_rows_round_trip() {
    let pages = Pages::default();
    let rows: Vec<_> = (0..1300).map(|i| json!({"id":i,"configuration":"x".repeat(2700),
        "nested":[[i],{"binding":[true,null,"axis"]}]})).collect();
    let value = json!({"profile":{"name":"large valid profile","mappings":rows}});
    assert!(size(&value).unwrap() > 3 << 20);
    let mut projection = pages.begin();
    let node = projection.encode(&value).unwrap();
    assert!(size(&node).unwrap() <= INLINE_BYTES);
    projection.commit("detail").unwrap();
    assert_eq!(expand(&pages, &node), value);
}

#[test]
fn nested_arrays_and_objects_preserve_the_original_collection_boundaries() {
    let pages = Pages::default();
    let entries: Map<_, _> = (0..1800).map(|i| (format!("key-{i}"), json!([i,"z".repeat(90)]))).collect();
    let value = json!([[Value::Object(entries)],[],{"nested":[[1,2],[3],{"empty":{}}]}]);
    let mut projection = pages.begin();
    let node = projection.encode(&value).unwrap();
    projection.commit("state").unwrap();
    assert_eq!(expand(&pages, &node), value);
}

#[test]
fn long_text_and_explicit_image_asset_deduplicate_and_round_trip() {
    let pages = Pages::default();
    let image = format!("data:image/png;base64,{}", "A".repeat(175_000));
    let mut projection = pages.begin();
    let image_node = projection.asset(Arc::from(image.as_str())).unwrap();
    let original = json!({"image":image,"caption":"ü".repeat(5000),"literal":"small"});
    let source = json!({"image":image_node,"caption":original["caption"],"literal":"small"});
    let node = projection.encode(&source).unwrap();
    projection.commit("detail").unwrap();
    assert_eq!(expand(&pages, &node), original);
    // Data URLs receive text treatment unless the caller explicitly marks an asset.
    let mut projection = pages.begin();
    let text_node = projection.encode(&json!(image)).unwrap();
    assert_eq!(text_node["$gamePage"]["kind"], "text");
    assert_eq!(text_node["$gamePage"]["id"], image_node["$gamePage"]["id"]);
    projection.commit("other").unwrap();
}

#[test]
fn escaped_text_and_maximum_keys_still_fit_each_leaf() {
    let pages = Pages::default();
    let value = Value::Object(Map::from_iter([(String::from("\u{1}").repeat(KEY_BYTES),
        json!("\u{1}".repeat(TEXT_BYTES)))]));
    let mut projection = pages.begin();
    let node = projection.encode(&value).unwrap();
    assert!(size(&node).unwrap() <= INLINE_BYTES);
    projection.commit("state").unwrap();
    assert_eq!(expand(&pages, &node), value);
}

#[test]
fn reused_issued_collection_retains_transitive_children_after_scope_replacement() {
    let pages = Pages::with_limits(Limits { bytes: 300_000, blobs: 100, blob_bytes: 200_000, scopes: 2 });
    let value = json!((0..300).map(|i| json!({"id":i,"text":"w".repeat(5000)})).collect::<Vec<_>>());
    let mut projection = pages.begin();
    let node = projection.encode(&value).unwrap();
    projection.commit("state").unwrap();
    let mut projection = pages.begin();
    assert_eq!(projection.encode(&node).unwrap(), node);
    projection.commit("state").unwrap();
    {
        let mut projection = pages.begin();
        assert!(projection.asset(Arc::from("a".repeat(200_000))).is_ok());
        assert!(projection.asset(Arc::from("b".repeat(200_000))).is_err());
    }
    assert_eq!(expand(&pages, &node), value);
}

#[test]
fn malformed_references_invalid_keys_and_excessive_depth_are_rejected() {
    let pages = Pages::default();
    let mut projection = pages.begin();
    assert!(projection.encode(&json!({"$gamePage":{}})).is_err());
    let mut node = projection.asset(Arc::from("asset")).unwrap();
    node["$gamePage"]["bytes"] = json!(6);
    assert!(projection.encode(&node).is_err());
    let unknown = json!({"$gamePage":{"id":format!("sha256:{}","f".repeat(64)),"bytes":1,"kind":"asset"}});
    assert!(projection.encode(&unknown).is_err());
    let invalid_key = Value::Object(Map::from_iter([("k".repeat(KEY_BYTES + 1), json!(1))]));
    assert!(projection.encode(&invalid_key).is_err());
    let mut nested = json!(1);
    for _ in 0..DEPTH + 1 { nested = json!([nested]); }
    assert!(projection.encode(&nested).is_err());
}

#[test]
fn recursive_encoding_capacity_failure_leaves_current_baseline_readable() {
    let pages = Pages::with_limits(Limits { bytes: 12_000, blobs: 20, blob_bytes: 12_000, scopes: 1 });
    let mut projection = pages.begin();
    let baseline = projection.encode(&json!("baseline".repeat(800))).unwrap();
    projection.commit("state").unwrap();
    {
        let mut projection = pages.begin();
        let value = json!(["a".repeat(5000),"b".repeat(5000)]);
        assert!(projection.encode(&value).is_err());
    }
    assert_eq!(expand(&pages, &baseline), json!("baseline".repeat(800)));
}
