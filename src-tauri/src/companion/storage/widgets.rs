//! Current widget sources and designer snapshots share one SQLite authority.
use osg_companion_service::{validate_widget_bundle, BUNDLE_BYTES};
pub(crate) use workspace::validate_workspace;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Mutex, OnceLock},
};
pub(crate) struct Runtime {
    path: Option<PathBuf>,
    bundle: Mutex<Option<Value>>,
    database: OnceLock<osg_persistence::Database>,
    revision: Mutex<u64>,
    workspace: Mutex<(u64, Option<Value>)>,
}
const MAX_MCP_BYTES: usize = 2 * 1024 * 1024;
fn builtins() -> Value {
    let source: Value = serde_json::from_str(include_str!(
        "../../../../examples/companion/generic-toggle.source.json"
    ))
    .expect("generated Toggle");
    json!({"definitions":[source]})
}
pub(crate) fn guide() -> Value {
    json!({"example":builtins(),"workflow":[
      "Call widgets_list, then widget_get. Toggle is the only built-in pilot. Sources have one plain schema, a stable id and no format or revision fields. Saving replaces the current source with that id.",
      "Keep artwork in scene layers. Optional design contains typed expression nodes, state owners, state appearance overrides and transition effects. Regions/events/guards/actions live in behavior. Graph values use ref wires and lower to bounded expressions before compilation.",
      "Use design.owners to give a region ownership of a local signal and optional output port. Do not also write that signal or port in authored actions. Requested intent must stay separate from game-confirmed feedback and quality.",
      "Each appearance selects named region states and patches properties by stable layer id. Independent regions writing the same property require a combination override. Layer structure is global. Automatic numeric transition effects derive their endpoints from appearances; use explicit motion tracks for live-dependent endpoints.",
      "Call widget_save with a bundle containing exactly one definition: {definitions:[source]}, plus the exact expectedRevision returned by widgets_list/widget_get and a fresh operationId. Retry identical operationId/arguments only to recover an uncertain write. Saving source does not compile, publish, assign or activate a panel.",
      "Compile complete raster presentations on the host. Motion and sound are optional capability-dependent assets; devices never receive authoring layers. ESP32 remains outside this feature."
    ]})
}

fn merge(current: &Value, incoming: &Value) -> Result<Value, String> {
    validate_widget_bundle(current)?;
    validate_widget_bundle(incoming)?;
    let mut next = current.clone();
    let definitions = next["definitions"].as_array_mut().unwrap();
    for source in incoming["definitions"].as_array().unwrap() {
        if let Some(index) = definitions.iter().position(|old| old["id"] == source["id"]) {
            definitions[index] = source.clone();
        } else {
            definitions.push(source.clone());
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
            database: OnceLock::new(),
            revision: Mutex::new(0),
            workspace: Mutex::new((0, None)),
        }
    }
    fn database(&self) -> Result<Option<&osg_persistence::Database>, String> {
        let Some(path) = &self.path else {
            return Ok(None);
        };
        if self.database.get().is_none() {
            let _ = self
                .database
                .set(osg_persistence::Database::for_namespace(path)?);
        }
        Ok(self.database.get())
    }
    pub(crate) fn request(&self, request: &Value) -> Result<Value, String> {
        if matches!(
            request["command"].as_str(),
            Some("designer_workspace_read" | "designer_workspace_write")
        ) {
            return self.workspace_request(request);
        }
        let command = request["command"]
            .as_str()
            .ok_or("Missing widget operation.")?;
        let allowed: &[&str] = match command {
            "widget_library_get" => &[],
            "widget_library_merge" | "widget_save" => &["bundle", "expectedRevision"],
            "widgets_list" => &["offset", "limit"],
            "widget_get" => &["id"],
            _ => return Err("Unknown widget operation.".into()),
        };
        if request.to_string().len() > BUNDLE_BYTES + 1024
            || request.as_object().is_none_or(|o| {
                o.keys().any(|key| {
                    key != "command" && key != "clientId" && !allowed.contains(&key.as_str())
                })
            })
        {
            return Err("Invalid widget request fields or budget.".into());
        }
        let mut library = self.bundle.lock().unwrap_or_else(|p| p.into_inner());
        let mut workspace = self.workspace.lock().unwrap_or_else(|p| p.into_inner());
        let (database, library_revision, workspace_revision, current, saved) =
            self.snapshot(library.as_ref(), &workspace)?;
        *library = Some(current.clone());
        let definitions = current["definitions"].as_array().unwrap();
        match command {
            "widget_library_get" => {
                Ok(json!({"definitions":definitions,"revision":library_revision}))
            }
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
                            .filter(|n| (1..=64).contains(n))
                            .ok_or("Invalid limit.")
                    })
                    .transpose()?
                    .unwrap_or(32) as usize;
                let widgets=definitions.iter().skip(offset).take(limit).map(|s|json!({"id":s["id"],"title":s["title"],"size":s["size"],"inputs":s["inputs"],"outputs":s["outputs"]})).collect::<Vec<_>>();
                Ok(
                    json!({"widgets":widgets,"revision":library_revision,"total":definitions.len(),"offset":offset,"nextOffset":if offset+widgets.len()<definitions.len(){Some(offset+widgets.len())}else{None}}),
                )
            }
            "widget_get" => {
                let id = request["id"].as_str().ok_or("Choose a widget ID.")?;
                let source = definitions
                    .iter()
                    .find(|s| s["id"] == id)
                    .ok_or("Widget not found.")?;
                let result = json!({"definitions":[source],"revision":library_revision});
                if result.to_string().len() > MAX_MCP_BYTES {
                    return Err("Widget source exceeds MCP response budget.".into());
                }
                Ok(result)
            }
            _ => {
                if request["expectedRevision"].as_u64() != Some(library_revision) {
                    return Err("Widget library revision conflict. Reload before saving.".into());
                }
                let incoming = &request["bundle"];
                validate_widget_bundle(incoming)?;
                if command == "widget_save"
                    && incoming["definitions"].as_array().unwrap().len() != 1
                {
                    return Err("Save exactly one widget source.".into());
                }
                let next = merge(&current, incoming)?;
                let updated = saved
                    .map(|saved| workspace::synchronize(saved, &next))
                    .transpose()?;
                let (lr, wr) = self.commit(
                    database,
                    library_revision,
                    workspace_revision,
                    &next,
                    updated.as_ref(),
                )?;
                *library = Some(next.clone());
                *workspace = (wr, updated);
                *self.revision.lock().unwrap_or_else(|p| p.into_inner()) = lr;
                Ok(if command == "widget_save" {
                    json!({"definitions":incoming["definitions"],"revision":lr})
                } else {
                    json!({"definitions":next["definitions"],"revision":lr})
                })
            }
        }
    }
}
#[cfg(test)]
#[path = "widget_tests.rs"]
mod tests;
mod workspace;
