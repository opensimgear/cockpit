//! Workspace and shared source edits use one SQLite CAS transaction.
use super::*;
#[path="workspace_validation.rs"]
mod validation;
impl Runtime {
    pub(crate) fn workspace_request(&self, request: &Value) -> Result<Value, String> {
        let write=request["command"]=="designer_workspace_write";
        let fields=if write { &["command","clientId","expectedRevision","value"][..] } else { &["command","clientId"][..] };
        if request.as_object().is_none_or(|o|o.keys().any(|key|!fields.contains(&key.as_str()))) {
            return Err("Unexpected designer request fields.".into());
        }
        let mut library=self.bundle.lock().unwrap_or_else(|p|p.into_inner());
        let mut workspace=self.workspace.lock().unwrap_or_else(|p|p.into_inner());
        // Refresh immutable shared sources from SQLite so snapshots never erase
        // sources saved by another process or authenticated MCP client.
        let database=self.database()?;
        let current_library=if database.is_some() {self.load()?} else {library.clone().unwrap_or_else(builtins)};
        *library=Some(current_library.clone());
        let (revision, mut saved)=if let Some(db)=database {
            let (revision,record)=db.read_with_revision("companion","designer-workspace")?;
            let saved=record.map(|r|serde_json::from_slice::<Value>(&r.payload).map_err(|_|"Invalid stored designer workspace.")).transpose()?;
            (revision,saved)
        } else { workspace.clone() };
        if let Some(value)=&saved { validation::validate(value)?; }
        if !write {
            if let Some(value)=&mut saved {
                let merged=merge(&current_library,&json!({"bundleVersion":2,"library":{"formatVersion":2,"definitions":value["sources"]}}))?;
                value["sources"]=merged["library"]["definitions"].clone();
                validation::validate(value)?;
            }
            *library=Some(current_library);
            return Ok(json!({"revision":revision,"value":saved}));
        }
        if request["expectedRevision"].as_u64()!=Some(revision) {
            return Err("Designer revision conflict. Reload before saving.".into());
        }
        let mut value=request.get("value").ok_or("Missing designer workspace.")?.clone();
        validation::validate(&value)?;
        let next_library=merge(&current_library,&json!({"bundleVersion":2,"library":{"formatVersion":2,"definitions":value["sources"]}}))?;
        value["sources"]=next_library["library"]["definitions"].clone();
        validation::validate(&value)?;
        let next_revision=if let Some(db)=database {
            let bytes=serde_json::to_vec(&value).map_err(|_|"Cannot encode designer workspace.")?;
            let sources=serde_json::to_vec(&next_library).map_err(|_|"Cannot encode designer sources.")?;
            let mut library_revision=self.revision.lock().unwrap_or_else(|p|p.into_inner());
            let committed=db.commit(&[
                osg_persistence::Change {domain:"companion",key:"designer-workspace",expected_revision:revision,payload:Some(&bytes)},
                osg_persistence::Change {domain:"widgets",key:"library",expected_revision:*library_revision,payload:Some(&sources)},
            ])?;
            *library_revision=committed[1]; committed[0]
        } else { revision.checked_add(1).ok_or("Designer revision exhausted.")? };
        *library=Some(next_library);
        *workspace=(next_revision,Some(value.clone()));
        Ok(json!({"revision":next_revision,"value":value}))
    }
}

#[cfg(test)]
#[path="workspace_tests.rs"]
mod tests;
