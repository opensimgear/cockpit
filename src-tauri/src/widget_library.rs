//! Current editable widget sources shared by the designer and authenticated MCP.
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
    .expect("generated Toggle source");
    json!({"definitions":[toggle]})
}
pub(crate) fn guide() -> Value {
    json!({"example":builtins(),"workflow":[
      "Call widgets_list, then widget_get. Toggle is the only built-in pilot. Sources have one plain schema, a stable id and no format or revision fields. Saving replaces the current source with that id.",
      "Keep artwork in scene layers. Optional design contains typed expression nodes, state owners, state appearance overrides and transition effects. Regions/events/guards/actions live in behavior. Graph values use ref wires and lower to bounded expressions before compilation.",
      "Use design.owners to give a region ownership of a local signal and optional output port. Do not also write that signal or port in authored actions. Requested intent must stay separate from game-confirmed feedback and quality.",
      "Each appearance selects named region states and patches properties by stable layer id. Independent regions writing the same property require a combination override. Layer structure is global. Automatic numeric transition effects derive their endpoints from appearances; use explicit motion tracks for live-dependent endpoints.",
      "Call widget_save with a bundle containing exactly one definition: {definitions:[source]}, plus a fresh operationId. Retry identical operationId/arguments only to recover an uncertain write. Saving source does not compile, publish, assign or activate a panel.",
      "Compile complete raster presentations on the host. Motion and sound are optional capability-dependent assets; devices never receive authoring layers. ESP32 remains outside this feature."
    ]})
}
fn merge(current: &Value, incoming: &Value) -> Result<Value, String> {
    validate_widget_bundle(current)?;
    validate_widget_bundle(incoming)?;
    let mut next = current.clone();
    let target = next["definitions"].as_array_mut().unwrap();
    for source in incoming["definitions"].as_array().unwrap() {
        if let Some(index) = target.iter().position(|old| old["id"] == source["id"]) {
            target[index] = source.clone();
        } else {
            target.push(source.clone());
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
            let parent = path.parent().ok_or("Missing widget storage directory")?;
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
            return Err("Stored widget library exceeds budget".into());
        }
        let saved: Value =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid saved widget library")?;
        merge(&builtins(), &saved)
    }
    pub(crate) fn request(&self, request: &Value) -> Result<Value, String> {
        let command = request["command"]
            .as_str()
            .ok_or("Missing widget operation")?;
        let allowed: &[&str] = match command {
            "widget_library_get" => &[],
            "widget_library_merge" | "widget_save" => &["bundle"],
            "widgets_list" => &["offset", "limit"],
            "widget_get" => &["id"],
            _ => return Err("Unknown widget operation".into()),
        };
        if request.to_string().len() > BUNDLE_BYTES + 1024
            || request.as_object().is_none_or(|r| {
                r.keys()
                    .any(|k| k != "command" && k != "clientId" && !allowed.contains(&k.as_str()))
            })
        {
            return Err("Invalid widget request fields or budget".into());
        }
        let mut guard = self.bundle.lock().unwrap_or_else(|p| p.into_inner());
        if guard.is_none() {
            *guard = Some(self.load()?);
        }
        let current = guard.as_ref().unwrap();
        let definitions = current["definitions"]
            .as_array()
            .ok_or("Missing definitions")?;
        match command {
            "widget_library_get" => Ok(current.clone()),
            "widgets_list" => {
                let offset = request
                    .get("offset")
                    .map(|v| v.as_u64().filter(|n| *n <= 262144).ok_or("Invalid offset"))
                    .transpose()?
                    .unwrap_or(0) as usize;
                let limit = request
                    .get("limit")
                    .map(|v| {
                        v.as_u64()
                            .filter(|n| *n >= 1 && *n <= 64)
                            .ok_or("Invalid limit")
                    })
                    .transpose()?
                    .unwrap_or(32) as usize;
                let widgets=definitions.iter().skip(offset).take(limit).map(|s|json!({"id":s["id"],"title":s["title"],"size":s["size"],"inputs":s["inputs"],"outputs":s["outputs"]})).collect::<Vec<_>>();
                Ok(
                    json!({"widgets":widgets,"total":definitions.len(),"offset":offset,"nextOffset":if offset+widgets.len()<definitions.len(){Some(offset+widgets.len())}else{None}}),
                )
            }
            "widget_get" => {
                let id = request["id"].as_str().ok_or("Choose a widget ID")?;
                let source = definitions
                    .iter()
                    .find(|s| s["id"] == id)
                    .ok_or("Widget not found")?;
                let result = json!({"definitions":[source]});
                if result.to_string().len() > MAX_MCP_BYTES {
                    return Err("Widget source exceeds MCP response budget".into());
                }
                Ok(result)
            }
            _ => {
                let incoming = &request["bundle"];
                validate_widget_bundle(incoming)?;
                if command == "widget_save"
                    && incoming["definitions"].as_array().unwrap().len() != 1
                {
                    return Err("Save exactly one widget source".into());
                }
                let next = merge(current, incoming)?;
                self.persist(&next)?;
                *guard = Some(next.clone());
                if command == "widget_save" {
                    let source = &incoming["definitions"][0];
                    Ok(json!({"id":source["id"],"title":source["title"],"saved":true}))
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
    fn custom() -> Value {
        let mut b = builtins();
        b["definitions"][0]["id"] = json!("custom");
        b
    }
    #[test]
    fn plain_sources_replace_in_place_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("widgets.json");
        let runtime = Runtime::new(Some(path.clone()));
        let mut b = custom();
        runtime
            .request(&json!({"command":"widget_save","bundle":b}))
            .unwrap();
        b["definitions"][0]["title"] = json!("Edited");
        runtime
            .request(&json!({"command":"widget_save","bundle":b}))
            .unwrap();
        let loaded = Runtime::new(Some(path));
        assert_eq!(
            loaded
                .request(&json!({"command":"widget_get","id":"custom"}))
                .unwrap(),
            b
        );
        let list = loaded.request(&json!({"command":"widgets_list"})).unwrap();
        assert_eq!(list["total"], 2);
        assert!(list["widgets"][0].get("revision").is_none());
        assert!(guide()["workflow"].as_array().unwrap().len() > 3);
    }
    #[test]
    fn invalid_updates_do_not_replace_current_source() {
        let runtime = Runtime::new(None);
        let b = custom();
        runtime
            .request(&json!({"command":"widget_save","bundle":b}))
            .unwrap();
        let mut invalid = b.clone();
        invalid["definitions"][0]["design"]["owners"][0]["signal"] = json!("missing");
        assert!(runtime
            .request(&json!({"command":"widget_save","bundle":invalid}))
            .is_err());
        assert_eq!(
            runtime
                .request(&json!({"command":"widget_get","id":"custom"}))
                .unwrap(),
            b
        );
        for request in [
            json!({"command":"widgets_list","limit":0}),
            json!({"command":"widgets_list","offset":-1}),
            json!({"command":"widget_get","id":"custom","version":1}),
            json!({"command":"widget_save","bundle":b,"expectedVersion":1}),
            json!({"command":"widget_library_get","path":"elsewhere"}),
            json!({"command":"unknown"}),
        ] {
            assert!(runtime.request(&request).is_err());
        }
        assert!(runtime
            .request(&json!({"command":"widget_save","bundle":{"definitions":[]}}))
            .is_err());
    }
    #[test]
    fn failed_disk_write_keeps_live_state() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("blocked");
        fs::write(&parent, "file").unwrap();
        let runtime = Runtime::new(Some(parent.join("widgets.json")));
        *runtime.bundle.lock().unwrap() = Some(builtins());
        assert!(runtime
            .request(&json!({"command":"widget_save","bundle":custom()}))
            .is_err());
        assert!(runtime
            .request(&json!({"command":"widget_get","id":"custom"}))
            .is_err());
    }
    #[test]
    fn pagination_and_rejected_old_fields() {
        let runtime = Runtime::new(None);
        assert_eq!(
            runtime
                .request(&json!({"command":"widgets_list","offset":10,"limit":1}))
                .unwrap()["widgets"],
            json!([])
        );
        let mut old = custom();
        old["definitions"][0]["revision"] = json!(1);
        assert!(runtime
            .request(&json!({"command":"widget_save","bundle":old}))
            .is_err());
    }
}
