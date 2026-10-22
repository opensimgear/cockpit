//! Shared immutable widget sources for the desktop designer and authenticated MCP.
use osg_companion_service::{validate_widget_bundle, BUNDLE_BYTES};
use serde_json::{json, Value};
use std::{fs, io::Read, path::PathBuf, sync::Mutex};

pub(crate) struct Runtime {
    path: Option<PathBuf>,
    bundle: Mutex<Option<Value>>,
}
fn builtins() -> Value {
    serde_json::from_str(include_str!(
        "../../examples/companion/builtin-widgets.widgets.json"
    ))
    .expect("generated widget catalogue")
}
pub(crate) fn guide() -> Value {
    let bundle = builtins();
    json!({"formatVersion":1,"workflow":["Call widgets_list, then widget_get for a design resembling the requested widget. The returned bundle is editable portable source, not HTML or executable code.","For a new design, use a fresh stable definition.id (ASCII letter/digit first, up to 64 letters/digits/underscore/hyphen), version:1 and expectedVersion:null. For a revision keep its id, increment version and provide the exact previous expectedVersion.","Edit title, size, properties, typed inputs/outputs, interaction, hitArea and source layers. Geometry uses normalized 0..1 bounds; size uses pixels. Layers may reference named typed properties with {property:name}. State overrides are normal, active, pressed, warning, stale, unknown, disabled and disconnected.","Call widget_save with the complete one-definition bundle and a fresh operationId. Retry identical arguments with the same operationId to recover the result. Version conflicts require reading latest and reconciling the edit.","The saved widget appears in the running Cockpit Widgets section and panel palette. Existing panel instances keep their pinned versions. Saving a widget never publishes a panel or starts device outputs."],"limits":{"definitions":64,"layersPerDefinition":32,"properties":24,"portsPerDirection":8,"definitionBytes":65536,"libraryBytes":1048576,"mcpRequestBytes":MAX_MCP_BYTES},"layerKinds":["rectangle","ellipse","line","text","image","indicator","gauge"],"interactionKinds":["none","momentary","toggle","selector","navigate"],"feedback":"Active is confirmed simulator/game feedback, never a local toggle request. Use state.confirmedPort referencing an indicator output; unknown/stale/disconnected remain explicit.","assets":"manifest contains {id,sha256,sizeBytes,mime}; assets contains {assetId,base64}. Images must have verified canonical base64, SHA256, supported PNG/JPEG/WebP bytes and bounded decoded dimensions. No filesystem paths, URLs, HTML or scripts.","example":{"bundleVersion":1,"library":{"formatVersion":1,"definitions":[bundle["library"]["definitions"].as_array().unwrap().iter().find(|d|d["id"]=="osg-capsule-toggle-horizontal").unwrap()]},"manifest":[],"assets":[]}})
}
const MAX_MCP_BYTES: usize = 2 * 1024 * 1024;
fn references(value: &Value, id: &str) -> bool {
    match value {
        Value::String(s) => s == id,
        Value::Array(a) => a.iter().any(|v| references(v, id)),
        Value::Object(o) => o.values().any(|v| references(v, id)),
        _ => false,
    }
}
fn merge(previous: &Value, incoming: &Value) -> Result<Value, String> {
    validate_widget_bundle(incoming)?;
    let mut next = previous.clone();
    for (field, key) in [
        ("definitions", "id"),
        ("manifest", "id"),
        ("assets", "assetId"),
    ] {
        let rows = if field == "definitions" {
            &incoming["library"][field]
        } else {
            &incoming[field]
        };
        let target = if field == "definitions" {
            &mut next["library"][field]
        } else {
            &mut next[field]
        };
        let target = target
            .as_array_mut()
            .ok_or("Invalid stored widget library.")?;
        for row in rows.as_array().ok_or("Invalid widget bundle.")? {
            let old = target.iter().find(|old| {
                old[key] == row[key] && (field != "definitions" || old["version"] == row["version"])
            });
            match old {Some(old) if old!=row=>return Err("Widget revisions and asset identities are immutable. Read the latest version before saving.".into()),None=>target.push(row.clone()),_=>{}}
        }
    }
    validate_widget_bundle(&next)?;
    Ok(next)
}
impl Runtime {
    pub(crate) fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            bundle: Mutex::new(None),
        }
    }
    pub(crate) fn request(&self, request: &Value) -> Result<Value, String> {
        let command = request["command"]
            .as_str()
            .ok_or("Missing widget operation.")?;
        let allowed: &[&str] = match command {
            "widget_library_get" => &[],
            "widget_library_merge" => &["bundle"],
            "widgets_list" => &["offset", "limit"],
            "widget_get" => &["id", "version"],
            "widget_save" => &["bundle", "expectedVersion"],
            _ => return Err("Unknown widget operation.".into()),
        };
        if request.to_string().len() > BUNDLE_BYTES + 1024
            || request.as_object().is_none_or(|r| {
                r.keys()
                    .any(|k| k != "command" && k != "clientId" && !allowed.contains(&k.as_str()))
            })
        {
            return Err("Invalid widget request fields or budget.".into());
        }
        let mut guard = self.bundle.lock().unwrap_or_else(|p| p.into_inner());
        if guard.is_none() {
            let loaded = if let Some(path) = &self.path {
                match fs::File::open(path) {
                    Ok(file) => {
                        let mut bytes = vec![];
                        file.take((BUNDLE_BYTES + 1) as u64)
                            .read_to_end(&mut bytes)
                            .map_err(|e| e.to_string())?;
                        if bytes.len() > BUNDLE_BYTES {
                            return Err("Stored widget library exceeds budget.".into());
                        }
                        let saved: Value = serde_json::from_slice(&bytes)
                            .map_err(|_| "Invalid saved widget library.")?;
                        merge(&builtins(), &saved)?
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => builtins(),
                    Err(e) => return Err(e.to_string()),
                }
            } else {
                builtins()
            };
            *guard = Some(loaded);
        }
        let current = guard.as_ref().unwrap();
        let definitions = current["library"]["definitions"]
            .as_array()
            .ok_or("Missing definitions.")?;
        match command {
            "widget_library_get" => Ok(current.clone()),
            "widgets_list" => {
                let offset = request
                    .get("offset")
                    .map(|v| v.as_u64().filter(|n| *n <= 262144).ok_or("Invalid offset."))
                    .transpose()?
                    .unwrap_or(0) as usize;
                let limit = request
                    .get("limit")
                    .map(|v| {
                        v.as_u64()
                            .filter(|n| *n >= 1 && *n <= 64)
                            .ok_or("Invalid limit.")
                    })
                    .transpose()?
                    .unwrap_or(32) as usize;
                let mut latest = std::collections::BTreeMap::<String, &Value>::new();
                for d in definitions {
                    let id = d["id"].as_str().unwrap().to_owned();
                    if latest
                        .get(&id)
                        .is_none_or(|old| old["version"].as_u64() < d["version"].as_u64())
                    {
                        latest.insert(id, d);
                    }
                }
                let widgets=latest.values().skip(offset).take(limit).map(|d|json!({"id":d["id"],"version":d["version"],"title":d["title"],"size":d["size"],"interaction":d["interaction"]})).collect::<Vec<_>>();
                Ok(
                    json!({"widgets":widgets,"total":latest.len(),"offset":offset,"nextOffset":if offset+widgets.len()<latest.len(){Some(offset+widgets.len())}else{None}}),
                )
            }
            "widget_get" => {
                let id = request["id"].as_str().ok_or("Choose a widget ID.")?;
                let version = request
                    .get("version")
                    .map(|v| {
                        v.as_u64()
                            .filter(|n| *n >= 1 && *n <= 65535)
                            .ok_or("Invalid widget version.")
                    })
                    .transpose()?;
                let d = definitions
                    .iter()
                    .filter(|d| d["id"] == id && version.is_none_or(|v| d["version"] == v))
                    .max_by_key(|d| d["version"].as_u64())
                    .ok_or("Widget revision not found.")?;
                let manifest = current["manifest"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|a| references(d, a["id"].as_str().unwrap()))
                    .collect::<Vec<_>>();
                let assets = current["assets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|a| manifest.iter().any(|m| m["id"] == a["assetId"]))
                    .collect::<Vec<_>>();
                Ok(
                    json!({"bundleVersion":1,"library":{"formatVersion":1,"definitions":[d]},"manifest":manifest,"assets":assets}),
                )
            }
            _ => {
                let incoming = &request["bundle"];
                validate_widget_bundle(incoming)?;
                if command == "widget_save" {
                    let rows = incoming["library"]["definitions"].as_array().unwrap();
                    if rows.len() != 1 {
                        return Err("Save exactly one widget definition.".into());
                    }
                    let d = &rows[0];
                    let latest = definitions
                        .iter()
                        .filter(|old| old["id"] == d["id"])
                        .filter_map(|d| d["version"].as_u64())
                        .max();
                    let expected = request
                        .get("expectedVersion")
                        .ok_or("Provide expectedVersion, or null for a new widget.")?;
                    if !((expected.is_null() && latest.is_none())
                        || (expected.as_u64().is_some() && expected.as_u64() == latest))
                        || d["version"].as_u64() != Some(latest.unwrap_or(0) + 1)
                    {
                        return Err("Widget version conflict. Read widget_get and save the next version with its exact expectedVersion.".into());
                    }
                }
                let next = merge(current, incoming)?;
                if let Some(path) = &self.path {
                    let parent = path.parent().ok_or("Missing widget storage directory.")?;
                    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    let bytes = serde_json::to_vec(&next).map_err(|e| e.to_string())?;
                    use std::io::Write;
                    let mut file =
                        tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
                    file.write_all(&bytes)
                        .and_then(|_| file.as_file().sync_all())
                        .map_err(|e| e.to_string())?;
                    file.persist(path).map_err(|e| e.to_string())?;
                }
                *guard = Some(next.clone());
                if command == "widget_save" {
                    let d = &incoming["library"]["definitions"][0];
                    Ok(json!({"id":d["id"],"version":d["version"],"title":d["title"],"saved":true}))
                } else {
                    Ok(next)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bundle(id: &str, version: u64) -> Value {
        let mut b = builtins();
        let mut d = b["library"]["definitions"][0].clone();
        d["id"] = json!(id);
        d["version"] = json!(version);
        b["library"]["definitions"] = json!([d]);
        b
    }
    fn save(b: Value, expected: Value) -> Value {
        json!({"command":"widget_save","bundle":b,"expectedVersion":expected})
    }
    #[test]
    fn discover_create_revise_and_restore_shared_library() {
        let dir = crate::test_directory();
        let path = dir.path().join("widgets.json");
        let r = Runtime::new(Some(path.clone()));
        assert_eq!(
            r.request(&json!({"command":"widgets_list","limit":2}))
                .unwrap()["widgets"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let b = bundle("mcp-test", 1);
        assert_eq!(
            r.request(&save(b.clone(), Value::Null)).unwrap()["version"],
            1
        );
        assert!(r
            .request(&save(b.clone(), Value::Null))
            .unwrap_err()
            .contains("conflict"));
        let mut next = bundle("mcp-test", 2);
        next["library"]["definitions"][0]["title"] = json!("Revised");
        assert_eq!(r.request(&save(next, json!(1))).unwrap()["version"], 2);
        assert_eq!(
            r.request(&json!({"command":"widget_get","id":"mcp-test"}))
                .unwrap()["library"]["definitions"][0]["title"],
            "Revised"
        );
        assert_eq!(
            r.request(&json!({"command":"widget_get","id":"mcp-test","version":1}))
                .unwrap(),
            b
        );
        let restored = Runtime::new(Some(path));
        assert_eq!(
            restored
                .request(&json!({"command":"widgets_list"}))
                .unwrap()["total"],
            15
        );
        assert_eq!(
            restored
                .request(&json!({"command":"widget_get","id":"mcp-test"}))
                .unwrap()["library"]["definitions"][0]["version"],
            2
        );
    }
    #[test]
    fn invalid_and_stale_writes_preserve_previous_commit() {
        let r = Runtime::new(None);
        let b = bundle("invalid-test", 1);
        for change in [
            json!({"bad":true}),
            json!({"kind":"script","code":"alert(1)"}),
        ] {
            let mut bad = b.clone();
            bad["library"]["definitions"][0]["layers"][0] = change;
            assert!(r.request(&save(bad, Value::Null)).is_err());
        }
        assert!(r
            .request(&save(bundle("invalid-test", 2), Value::Null))
            .is_err());
        let mut bad = save(b.clone(), Value::Null);
        bad.as_object_mut().unwrap().remove("expectedVersion");
        assert!(r.request(&bad).is_err());
        r.request(&save(b.clone(), Value::Null)).unwrap();
        let mut conflict = b.clone();
        conflict["library"]["definitions"][0]["title"] = json!("Overwrite");
        assert!(r
            .request(&json!({"command":"widget_library_merge","bundle":conflict}))
            .is_err());
        assert_eq!(
            r.request(&json!({"command":"widget_get","id":"invalid-test"}))
                .unwrap(),
            b
        );
    }
    #[test]
    fn migration_merges_without_replacing_revisions_and_pages_latest() {
        let r = Runtime::new(None);
        let b = bundle("imported", 1);
        for _ in 0..2 {
            r.request(&json!({"command":"widget_library_merge","bundle":b}))
                .unwrap();
        }
        r.request(&save(bundle("imported", 2), json!(1))).unwrap();
        let page = r
            .request(&json!({"command":"widgets_list","offset":0,"limit":1}))
            .unwrap();
        assert_eq!(page["total"], 15);
        assert_eq!(page["nextOffset"], 1);
        assert_eq!(page["widgets"][0]["version"], 2);
        assert!(r
            .request(&json!({"command":"widgets_list","offset":100}))
            .unwrap()["widgets"]
            .as_array()
            .unwrap()
            .is_empty());
        for request in [
            json!({"command":"widgets_list","limit":0}),
            json!({"command":"widgets_list","offset":-1}),
            json!({"command":"widget_get","id":"missing"}),
            json!({"command":"widget_get","id":"imported","version":0}),
            json!({"command":"widget_library_get","path":"secret"}),
            json!({"command":"unknown"}),
        ] {
            assert!(r.request(&request).is_err());
        }
    }
    #[test]
    fn corrupted_storage_and_failed_write_do_not_claim_success() {
        let dir = crate::test_directory();
        let path = dir.path().join("widgets.json");
        fs::write(&path, b"bad").unwrap();
        assert!(Runtime::new(Some(path.clone()))
            .request(&json!({"command":"widget_library_get"}))
            .is_err());
        fs::write(&path, " ".repeat(BUNDLE_BYTES + 1)).unwrap();
        assert!(Runtime::new(Some(path.clone()))
            .request(&json!({"command":"widget_library_get"}))
            .is_err());
        fs::remove_file(&path).unwrap();
        let r = Runtime::new(Some(path.clone()));
        r.request(&json!({"command":"widget_library_get"})).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(r.request(&save(bundle("failed", 1), Value::Null)).is_err());
        assert!(r
            .request(&json!({"command":"widget_get","id":"failed"}))
            .is_err());
    }
    #[test]
    fn guide_contains_valid_portable_example_and_invalid_bundles_fail() {
        let guide = guide();
        validate_widget_bundle(&guide["example"]).unwrap();
        let mut b = bundle("bad", 1);
        b["manifest"] =
            json!([{"id":"image","sha256":"0".repeat(64),"sizeBytes":1,"mime":"image/png"}]);
        assert!(validate_widget_bundle(&b).is_err());
        b["assets"] = json!([{"assetId":"image","base64":"AA=="}]);
        assert!(validate_widget_bundle(&b).is_err());
        b = bundle("bad", 1);
        b["library"]["definitions"][0]["layers"][0]["bounds"]["width"] = json!(4);
        assert!(validate_widget_bundle(&b).is_err());
    }
    #[test]
    fn verified_assets_roundtrip_without_leaking_unrelated_images() {
        use base64::{engine::general_purpose::STANDARD, Engine};
        use sha2::{Digest, Sha256};
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(2, 2)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let bytes = bytes.into_inner();
        let mut b = bundle("artwork", 1);
        b["manifest"] = json!([{"id":"artwork-asset","sha256":format!("{:x}",Sha256::digest(&bytes)),"sizeBytes":bytes.len(),"mime":"image/png"}]);
        b["assets"] = json!([{"assetId":"artwork-asset","base64":STANDARD.encode(&bytes)}]);
        b["library"]["definitions"][0]["layers"].as_array_mut().unwrap().push(json!({"id":"picture","label":"Artwork","kind":"image","bounds":{"x":0,"y":0,"width":1,"height":1},"assetId":"artwork-asset"}));
        let r = Runtime::new(None);
        r.request(&save(b.clone(), Value::Null)).unwrap();
        assert_eq!(
            r.request(&json!({"command":"widget_get","id":"artwork"}))
                .unwrap(),
            b
        );
        assert!(r
            .request(&json!({"command":"widget_get","id":"osg-capsule-toggle-horizontal"}))
            .unwrap()["assets"]
            .as_array()
            .unwrap()
            .is_empty());
        let mut invalid = b.clone();
        invalid["assets"][0]["base64"] = json!("broken");
        assert!(validate_widget_bundle(&invalid).is_err());
        invalid = b.clone();
        invalid["manifest"][0]["sha256"] = json!("0".repeat(64));
        assert!(validate_widget_bundle(&invalid).is_err());
        invalid = b.clone();
        invalid["assets"]
            .as_array_mut()
            .unwrap()
            .push(b["assets"][0].clone());
        assert!(validate_widget_bundle(&invalid).is_err());
        invalid = b.clone();
        invalid["library"]["definitions"][0]["version"] = json!(2);
        invalid["manifest"][0]["sizeBytes"] = json!(0);
        assert!(r.request(&save(invalid, json!(1))).is_err());
        assert!(references(
            &json!([true,3,{"property":"artwork-asset"}]),
            "artwork-asset"
        ));
    }
    #[test]
    fn concurrent_creations_commit_once_and_oversized_or_multiple_sources_fail() {
        let r = std::sync::Arc::new(Runtime::new(None));
        let b = bundle("concurrent", 1);
        let threads = (0..4)
            .map(|_| {
                let r = r.clone();
                let b = b.clone();
                std::thread::spawn(move || r.request(&save(b, Value::Null)).is_ok())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            threads
                .into_iter()
                .map(|t| t.join().unwrap() as usize)
                .sum::<usize>(),
            1
        );
        let mut multiple = b.clone();
        multiple["library"]["definitions"]
            .as_array_mut()
            .unwrap()
            .push(bundle("second", 1)["library"]["definitions"][0].clone());
        assert!(r.request(&save(multiple, Value::Null)).is_err());
        assert!(r.request(&json!({"command":"widget_library_merge","bundle":{"extra":"x".repeat(BUNDLE_BYTES+1)}})).is_err());
        assert!(r.request(&json!({})).is_err());
    }
}
