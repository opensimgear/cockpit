//! Immutable generic widget sources shared by the designer and authenticated MCP.
use osg_companion_service::{validate_widget_bundle, BUNDLE_BYTES};
use serde_json::{json, Value};
use std::{fs, io::Read, path::PathBuf, sync::Mutex};

pub(crate) struct Runtime {
    path: Option<PathBuf>,
    bundle: Mutex<Option<Value>>,
}
const MAX_MCP_BYTES: usize = 2 * 1024 * 1024;
fn builtins() -> Value {
    let toggle: Value = serde_json::from_str(include_str!(
        "../../examples/companion/generic-toggle.source.json"
    ))
    .expect("generated generic Toggle source");
    json!({"bundleVersion":2,"library":{"formatVersion":2,"definitions":[toggle]}})
}
pub(crate) fn guide() -> Value {
    json!({"formatVersion":2,"workflow":[
        "Call widgets_list, then widget_get. The sole built-in seed is Toggle (toggle-v2). Returned definitions are editable generic sources; legacy widgets and imports are retired.",
        "New sources use formatVersion:2, a fresh stable id and revision:1. Send expectedVersion:null to widget_save. To revise, increment source.revision and provide the previous exact revision as expectedVersion. MCP version arguments/results identify source.revision.",
        "Design a pixel-space scene using rectangle, ellipse, line, path, text, image and group nodes. Bind properties using bounded data expressions: literal, ref, state, not, and, or, comparisons, arithmetic, clamp and if. Parameters and typed signals have distinct IDs. No JavaScript, URLs or executable markup is accepted.",
        "Configure typed signals, declared events, named independent state-machine regions, ordered guards and bounded actions. Inputs declare emit outputs with matching type/domain/neutral. Feedback quality stays separate from locally requested controls.",
        "Configure finite numeric visualDomains with range/samples, travelPixels and accepted maxErrorPixels. Optional motions bake property/transform tracks. Optional sounds reference bounded inline PCM16 WAV assets and semantic cues.",
        "Call widget_save with {bundleVersion:2,library:{formatVersion:2,definitions:[source]}} and a fresh operationId. Reuse identical operationId/arguments only to recover an uncertain write. Conflicts require reading latest first.",
        "Saving stores authoring source. Cockpit compiles for resolved parameters, exact size and target capabilities before Try, publishing or assignment. Companions receive finished pixel tiles and behavior tables; source layers never deploy. ESP32 is outside this feature."
    ],"limits":{"definitionsAndRevisions":256,"sceneNodes":128,"groupDepth":8,"parameters":32,"portsPerDirection":16,"sourceBytes":1048576,"libraryBytes":8388608,"mcpRequestBytes":MAX_MCP_BYTES},"primitives":["rectangle","ellipse","line","path","text","image","group"],"assets":"Assets are inline per source: {id,mime,base64}. Images: bounded PNG/JPEG/WebP. Audio: PCM16 WAV, 8-48 kHz, mono/stereo, at most 10 seconds. No filesystem paths or network URLs.","example":builtins()})
}
fn merge(previous: &Value, incoming: &Value) -> Result<Value, String> {
    validate_widget_bundle(incoming)?;
    let mut next = previous.clone();
    let target = next["library"]["definitions"]
        .as_array_mut()
        .ok_or("Invalid stored generic library.")?;
    for source in incoming["library"]["definitions"].as_array().unwrap() {
        match target
            .iter()
            .find(|old| old["id"] == source["id"] && old["revision"] == source["revision"])
        {
            Some(old) if old != source => {
                return Err(
                    "Widget revisions are immutable. Read the latest revision before saving."
                        .into(),
                )
            }
            None => target.push(source.clone()),
            _ => {}
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
    fn persist(&self, bundle: &Value) -> Result<(), String> {
        if let Some(path) = &self.path {
            let parent = path.parent().ok_or("Missing widget storage directory.")?;
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            use std::io::Write;
            let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
            file.write_all(&serde_json::to_vec(bundle).map_err(|e| e.to_string())?)
                .and_then(|_| file.as_file().sync_all())
                .map_err(|e| e.to_string())?;
            file.persist(path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    fn load(&self) -> Result<Value, String> {
        let Some(path) = &self.path else {
            return Ok(builtins());
        };
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(builtins()),
            Err(e) => return Err(e.to_string()),
        };
        let mut bytes = vec![];
        file.take((BUNDLE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > BUNDLE_BYTES {
            return Err("Stored widget library exceeds budget.".into());
        }
        let saved: Value =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid saved widget library.")?;
        if saved["bundleVersion"] == 1 || saved["library"]["formatVersion"] == 1 {
            let seed = builtins();
            validate_widget_bundle(&seed)?;
            self.persist(&seed)?;
            return Ok(seed);
        }
        merge(&builtins(), &saved)
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
                r.keys().any(|key| {
                    key != "command" && key != "clientId" && !allowed.contains(&key.as_str())
                })
            })
        {
            return Err("Invalid widget request fields or budget.".into());
        }
        let mut guard = self.bundle.lock().unwrap_or_else(|p| p.into_inner());
        if guard.is_none() {
            *guard = Some(self.load()?);
        }
        let current = guard.as_ref().unwrap();
        let definitions = current["library"]["definitions"]
            .as_array()
            .ok_or("Missing generic source definitions.")?;
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
                for source in definitions {
                    let id = source["id"].as_str().unwrap().to_owned();
                    if latest
                        .get(&id)
                        .is_none_or(|old| old["revision"].as_u64() < source["revision"].as_u64())
                    {
                        latest.insert(id, source);
                    }
                }
                let widgets=latest.values().skip(offset).take(limit).map(|source|json!({"id":source["id"],"version":source["revision"],"revision":source["revision"],"title":source["title"],"size":source["size"],"inputs":source["inputs"],"outputs":source["outputs"]})).collect::<Vec<_>>();
                Ok(
                    json!({"widgets":widgets,"total":latest.len(),"offset":offset,"nextOffset":if offset+widgets.len()<latest.len(){Some(offset+widgets.len())}else{None}}),
                )
            }
            "widget_get" => {
                let id = request["id"].as_str().ok_or("Choose a widget ID.")?;
                let revision = request
                    .get("version")
                    .map(|v| {
                        v.as_u64()
                            .filter(|n| *n >= 1 && *n <= 65535)
                            .ok_or("Invalid widget revision.")
                    })
                    .transpose()?;
                let source = definitions
                    .iter()
                    .filter(|source| {
                        source["id"] == id && revision.is_none_or(|v| source["revision"] == v)
                    })
                    .max_by_key(|source| source["revision"].as_u64())
                    .ok_or("Widget revision not found.")?;
                Ok(json!({"bundleVersion":2,"library":{"formatVersion":2,"definitions":[source]}}))
            }
            _ => {
                let incoming = &request["bundle"];
                validate_widget_bundle(incoming)?;
                if command == "widget_save" {
                    let rows = incoming["library"]["definitions"].as_array().unwrap();
                    if rows.len() != 1 {
                        return Err("Save exactly one widget source.".into());
                    }
                    let source = &rows[0];
                    let latest = definitions
                        .iter()
                        .filter(|old| old["id"] == source["id"])
                        .filter_map(|old| old["revision"].as_u64())
                        .max();
                    let expected = request
                        .get("expectedVersion")
                        .ok_or("Provide expectedVersion, or null for a new source.")?;
                    if !((expected.is_null() && latest.is_none())
                        || (expected.as_u64().is_some() && expected.as_u64() == latest))
                        || source["revision"].as_u64() != Some(latest.unwrap_or(0) + 1)
                    {
                        return Err("Widget revision conflict. Read widget_get and save the next revision with its exact expectedVersion.".into());
                    }
                }
                let next = merge(current, incoming)?;
                self.persist(&next)?;
                *guard = Some(next.clone());
                if command == "widget_save" {
                    let source = &incoming["library"]["definitions"][0];
                    Ok(
                        json!({"id":source["id"],"version":source["revision"],"revision":source["revision"],"title":source["title"],"saved":true}),
                    )
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
    fn bundle(id: &str, revision: u64) -> Value {
        let mut bundle = builtins();
        bundle["library"]["definitions"][0]["id"] = json!(id);
        bundle["library"]["definitions"][0]["revision"] = json!(revision);
        bundle
    }
    fn save(bundle: Value, expected: Value) -> Value {
        json!({"command":"widget_save","bundle":bundle,"expectedVersion":expected})
    }
    #[test]
    fn sole_toggle_seed_and_generic_guide_validate() {
        let r = Runtime::new(None);
        let list = r.request(&json!({"command":"widgets_list"})).unwrap();
        assert_eq!(list["total"], 1);
        assert_eq!(list["widgets"][0]["id"], "toggle-v2");
        assert_eq!(list["widgets"][0]["title"], "Toggle");
        validate_widget_bundle(&guide()["example"]).unwrap();
        assert_eq!(guide()["formatVersion"], 2);
    }
    #[test]
    fn create_revise_page_and_restore_sources() {
        let dir = crate::test_directory();
        let path = dir.path().join("widgets.json");
        let r = Runtime::new(Some(path.clone()));
        let first = bundle("custom", 1);
        assert_eq!(
            r.request(&save(first.clone(), Value::Null)).unwrap()["version"],
            1
        );
        assert!(r
            .request(&save(first.clone(), Value::Null))
            .unwrap_err()
            .contains("conflict"));
        let mut next = bundle("custom", 2);
        next["library"]["definitions"][0]["title"] = json!("Revised");
        r.request(&save(next, json!(1))).unwrap();
        assert_eq!(
            r.request(&json!({"command":"widget_get","id":"custom","version":1}))
                .unwrap(),
            first
        );
        assert_eq!(
            r.request(&json!({"command":"widget_get","id":"custom"}))
                .unwrap()["library"]["definitions"][0]["revision"],
            2
        );
        let restored = Runtime::new(Some(path));
        assert_eq!(
            restored
                .request(&json!({"command":"widgets_list","limit":1}))
                .unwrap()["nextOffset"],
            1
        );
        assert_eq!(
            restored
                .request(&json!({"command":"widgets_list","offset":100}))
                .unwrap()["widgets"],
            json!([])
        );
    }
    #[test]
    fn legacy_library_resets_once_and_preserves_unrelated_settings() {
        let dir = crate::test_directory();
        let path = dir.path().join("widgets-v1.json");
        let other = dir.path().join("pairing.json");
        fs::write(&other, b"preserve pairing and trust").unwrap();
        fs::write(&path,r#"{"bundleVersion":1,"library":{"formatVersion":1,"definitions":[{"id":"obsolete"}]},"manifest":[],"assets":[]}"#).unwrap();
        let r = Runtime::new(Some(path.clone()));
        assert_eq!(
            r.request(&json!({"command":"widget_library_get"})).unwrap(),
            builtins()
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap(),
            builtins()
        );
        assert_eq!(fs::read(&other).unwrap(), b"preserve pairing and trust");
        r.request(&save(bundle("after-reset", 1), Value::Null))
            .unwrap();
        assert_eq!(
            Runtime::new(Some(path))
                .request(&json!({"command":"widgets_list"}))
                .unwrap()["total"],
            2
        );
    }
    #[test]
    fn immutable_revisions_invalid_source_and_stale_writes_preserve_previous_commit() {
        let r = Runtime::new(None);
        let first = bundle("custom", 1);
        r.request(&save(first.clone(), Value::Null)).unwrap();
        let mut conflict = first.clone();
        conflict["library"]["definitions"][0]["title"] = json!("Overwrite");
        assert!(r
            .request(&json!({"command":"widget_library_merge","bundle":conflict}))
            .is_err());
        let mut invalid = bundle("invalid", 1);
        invalid["library"]["definitions"][0]["scene"][0] =
            json!({"kind":"script","code":"alert(1)"});
        assert!(r.request(&save(invalid, Value::Null)).is_err());
        assert!(r.request(&save(bundle("custom", 3), json!(1))).is_err());
        assert_eq!(
            r.request(&json!({"command":"widget_get","id":"custom"}))
                .unwrap(),
            first
        );
    }
    #[test]
    fn identical_imports_are_idempotent_and_legacy_imports_fail() {
        let r = Runtime::new(None);
        let b = bundle("imported", 1);
        for _ in 0..2 {
            r.request(&json!({"command":"widget_library_merge","bundle":b}))
                .unwrap();
        }
        assert_eq!(
            r.request(&json!({"command":"widgets_list"})).unwrap()["total"],
            2
        );
        assert!(r.request(&json!({"command":"widget_library_merge","bundle":{"bundleVersion":1,"library":{"formatVersion":1,"definitions":[]},"manifest":[],"assets":[]}})).is_err());
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
    fn requests_concurrency_and_single_source_save_limits() {
        let r = std::sync::Arc::new(Runtime::new(None));
        for request in [
            json!({}),
            json!({"command":"unknown"}),
            json!({"command":"widgets_list","limit":0}),
            json!({"command":"widgets_list","offset":-1}),
            json!({"command":"widget_get","id":"missing"}),
            json!({"command":"widget_get","id":"toggle-v2","version":0}),
            json!({"command":"widget_library_get","path":"secret"}),
            json!({"command":"widget_save","bundle":bundle("new",1)}),
        ] {
            assert!(r.request(&request).is_err());
        }
        let handles = (0..4)
            .map(|_| {
                let r = r.clone();
                std::thread::spawn(move || {
                    r.request(&save(bundle("concurrent", 1), Value::Null))
                        .is_ok()
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            handles
                .into_iter()
                .map(|h| h.join().unwrap() as usize)
                .sum::<usize>(),
            1
        );
        let mut multiple = bundle("one", 1);
        multiple["library"]["definitions"]
            .as_array_mut()
            .unwrap()
            .push(bundle("two", 1)["library"]["definitions"][0].clone());
        assert!(r.request(&save(multiple, Value::Null)).is_err());
        assert!(r.request(&json!({"command":"widget_library_merge","bundle":{"extra":"x".repeat(BUNDLE_BYTES+1)}})).is_err());
    }
}
