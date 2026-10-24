//! Widget edits and workspace changes fence both row revisions in one commit.
use super::*;
#[path = "workspace_validation.rs"]
mod validation;
pub(super) fn synchronize(mut value: Value, library: &Value) -> Result<Value, String> {
    validation::validate(&value)?;
    let old = value["sources"].as_array().unwrap();
    let sources = library["definitions"].as_array().unwrap();
    let changed = sources
        .iter()
        .filter(|source| {
            old.iter().find(|previous| previous["id"] == source["id"]) != Some(*source)
        })
        .map(|source| source["id"].clone())
        .collect::<Vec<_>>();
    let stale = value["drafts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|draft| {
            draft["widgets"]
                .as_array()
                .unwrap()
                .iter()
                .any(|widget| changed.contains(&widget["sourceId"]))
        })
        .filter_map(|draft| draft["id"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    for id in stale {
        value["publications"].as_object_mut().unwrap().remove(&id);
    }
    value["sources"] = json!(sources);
    validation::validate(&value)?;
    Ok(value)
}
impl Runtime {
    pub(super) fn snapshot<'a>(
        &'a self,
        library: Option<&Value>,
        workspace: &(u64, Option<Value>),
    ) -> Result<
        (
            Option<&'a osg_persistence::Database>,
            u64,
            u64,
            Value,
            Option<Value>,
        ),
        String,
    > {
        let database = self.database()?;
        let (lr, wr, current, saved) = if let Some(database) = database {
            let rows = database
                .read_many(&[("widgets", "library"), ("companion", "designer-workspace")])?;
            let current = rows[0]
                .1
                .as_ref()
                .map(|r| {
                    serde_json::from_slice::<Value>(&r.payload)
                        .map_err(|_| "Invalid stored widget sources.")
                })
                .transpose()?
                .unwrap_or_else(builtins);
            let saved = rows[1]
                .1
                .as_ref()
                .map(|r| {
                    serde_json::from_slice::<Value>(&r.payload)
                        .map_err(|_| "Invalid stored designer workspace.")
                })
                .transpose()?;
            (rows[0].0, rows[1].0, current, saved)
        } else {
            (
                *self.revision.lock().unwrap_or_else(|p| p.into_inner()),
                workspace.0,
                library.cloned().unwrap_or_else(builtins),
                workspace.1.clone(),
            )
        };
        validate_widget_bundle(&current)?;
        if let Some(value) = &saved {
            validation::validate(value)?;
        }
        Ok((database, lr, wr, current, saved))
    }
    pub(super) fn commit(
        &self,
        database: Option<&osg_persistence::Database>,
        lr: u64,
        wr: u64,
        library: &Value,
        workspace: Option<&Value>,
    ) -> Result<(u64, u64), String> {
        let Some(database) = database else {
            return Ok((
                lr.checked_add(1).ok_or("Widget revision exhausted.")?,
                wr.checked_add(1).ok_or("Workspace revision exhausted.")?,
            ));
        };
        let sources = serde_json::to_vec(library).map_err(|_| "Cannot encode widget sources.")?;
        let bytes = workspace
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|_| "Cannot encode designer workspace.")?;
        let committed = database.commit(&[
            osg_persistence::Change {
                domain: "widgets",
                key: "library",
                expected_revision: lr,
                payload: Some(&sources),
            },
            osg_persistence::Change {
                domain: "companion",
                key: "designer-workspace",
                expected_revision: wr,
                payload: bytes.as_deref(),
            },
        ])?;
        Ok((committed[0], committed[1]))
    }
    pub(crate) fn workspace_request(&self, request: &Value) -> Result<Value, String> {
        let write = request["command"] == "designer_workspace_write";
        let allowed = if write {
            &[
                "command",
                "clientId",
                "expectedRevision",
                "expectedLibraryRevision",
                "value",
            ][..]
        } else {
            &["command", "clientId"][..]
        };
        if request
            .as_object()
            .is_none_or(|o| o.keys().any(|key| !allowed.contains(&key.as_str())))
        {
            return Err("Unexpected designer request fields.".into());
        }
        let mut library = self.bundle.lock().unwrap_or_else(|p| p.into_inner());
        let mut workspace = self.workspace.lock().unwrap_or_else(|p| p.into_inner());
        let (database, lr, wr, current, saved) = self.snapshot(library.as_ref(), &workspace)?;
        if !write {
            let value = saved
                .map(|saved| synchronize(saved, &current))
                .transpose()?
                .unwrap_or_else(
                    || json!({"sources":current["definitions"],"drafts":[],"publications":{}}),
                );
            *library = Some(current);
            return Ok(json!({"revision":wr,"libraryRevision":lr,"value":value}));
        }
        if request["expectedRevision"].as_u64() != Some(wr)
            || request["expectedLibraryRevision"].as_u64() != Some(lr)
        {
            return Err(
                "Designer or widget library revision conflict. Reload before saving.".into(),
            );
        }
        let mut value = request
            .get("value")
            .ok_or("Missing designer workspace.")?
            .clone();
        validation::validate(&value)?;
        let next = merge(&current, &json!({"definitions":value["sources"]}))?;
        value["sources"] = next["definitions"].clone();
        validation::validate(&value)?;
        let (lr, wr) = self.commit(database, lr, wr, &next, Some(&value))?;
        *library = Some(next);
        *workspace = (wr, Some(value.clone()));
        *self.revision.lock().unwrap_or_else(|p| p.into_inner()) = lr;
        Ok(json!({"revision":wr,"libraryRevision":lr,"value":value}))
    }
}
#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;
