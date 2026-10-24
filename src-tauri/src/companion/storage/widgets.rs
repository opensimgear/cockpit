//! Immutable generic widget sources shared by the designer and authenticated MCP.
use osg_companion_service::{validate_widget_bundle, BUNDLE_BYTES};
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
    let toggle: Value = serde_json::from_str(include_str!(
        "../../../../examples/companion/generic-toggle.source.json"
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
            database: OnceLock::new(),
            revision: Mutex::new(0),
            workspace: Mutex::new((0, None)),
        }
    }
    fn database(&self) -> Result<Option<&osg_persistence::Database>, String> {
        let Some(path) = &self.path else { return Ok(None); };
        if self.database.get().is_none() {
            let _ = self.database.set(osg_persistence::Database::for_namespace(path)?);
        }
        Ok(self.database.get())
    }
    fn persist(&self, bundle: &Value) -> Result<(), String> {
        if let Some(database) = self.database()? {
            let bytes = serde_json::to_vec(bundle).map_err(|_| "Cannot encode widget sources.")?;
            let mut revision = self.revision.lock().unwrap_or_else(|p| p.into_inner());
            *revision = database.commit(&[osg_persistence::Change { domain:"widgets", key:"library", expected_revision:*revision, payload:Some(&bytes) }])?[0];
        }
        Ok(())
    }
    fn load(&self) -> Result<Value, String> {
        let Some(database) = self.database()? else { return Ok(builtins()); };
        let (revision, record) = database.read_with_revision("widgets", "library")?;
        let bundle = match record {
            Some(record) => merge(&builtins(), &serde_json::from_slice(&record.payload).map_err(|_| "Invalid stored widget sources.")?)?,
            None => builtins(),
        };
        *self.revision.lock().unwrap_or_else(|p| p.into_inner()) = revision;
        Ok(bundle)
    }
    pub(crate) fn request(&self, request: &Value) -> Result<Value, String> {
        if matches!(request["command"].as_str(), Some("designer_workspace_read" | "designer_workspace_write")) {
            return self.workspace_request(request);
        }
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
#[path = "widget_tests.rs"]
mod tests;

#[path = "workspace.rs"]
mod workspace;
