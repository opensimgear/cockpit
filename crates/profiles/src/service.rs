//! Named-profile CRUD and session metadata. This boundary cannot execute an
//! output, change HidHide, start integrations, or publish companion layouts.
use crate::session::{SelectionMode, Session};
use crate::store::{Library, Store};
use crate::{
    new_document, semantic_hash, ProfileDocument, DEFAULT_PROFILE_ID, MAX_PROFILES, MAX_REVISION,
};
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct Service {
    library: Library,
    store: Option<Store>,
    session: Session,
    diagnostics: Vec<String>,
    read_only: bool,
}

impl Service {
    pub fn new(root: Option<PathBuf>) -> Self {
        let store = root.map(Store::new);
        let mut library = Library::default();
        let mut diagnostics = vec![];
        if let Some(store) = &store {
            match store.load() {
                Ok(Some(saved)) => library = saved,
                Ok(None) => {
                    if let Err(error) = store.save(&library) {
                        diagnostics.push(error);
                    }
                }
                Err(error) => diagnostics.push(error),
            }
        }
        let read_only = !diagnostics.is_empty();
        Self {
            library,
            store,
            session: Session::default(),
            diagnostics,
            read_only,
        }
    }

    pub fn request(&mut self, request: &Value) -> Result<Value, String> {
        if serde_json::to_vec(request)
            .map_err(|e| e.to_string())?
            .len()
            > crate::MAX_PROFILE_BYTES + 64 * 1024
        {
            return Err("Profile request exceeds its size limit.".into());
        }
        let command = request["command"]
            .as_str()
            .ok_or("A profile command is required.")?;
        let fields: &[&str] = match command {
            "profile_list" | "profile_status" => &[],
            "profile_apply" => &["id"],
            "profile_get" | "profile_export" => &["id"],
            "profile_create" => &["name"],
            "profile_rename" => &["id", "name", "expectedRevision"],
            "profile_duplicate" => &["id", "name"],
            "profile_delete" => &["id", "expectedRevision"],
            "profile_save" => &["document", "expectedRevision"],
            "profile_import" => &["document", "name"],
            "profile_select" => &["id", "selectionSource"],
            "profile_pause" => &["paused"],
            "profile_selection_mode" => &["mode"],
            "profile_reset" => &["confirm"],
            _ => return Err("Unknown profile command.".into()),
        };
        let envelope = request
            .as_object()
            .ok_or("A profile request must be an object.")?;
        if envelope
            .keys()
            .any(|key| key != "command" && !fields.contains(&key.as_str()))
        {
            return Err("Unknown field in profile request.".into());
        }
        match command {
            "profile_list" => {
                let profiles = self
                    .library
                    .profiles
                    .iter()
                    .map(|document| {
                        Ok(json!({
                            "id": document.id, "name": document.name, "revision": document.revision,
                            "isDefault": document.id == DEFAULT_PROFILE_ID,
                            "semanticHash": semantic_hash(document)?,
                        }))
                    })
                    .collect::<Result<Vec<Value>, String>>()?;
                Ok(json!({"profiles":profiles,"session":self.session.status(),
                    "diagnostics":self.diagnostics,"readOnly":self.read_only}))
            }
            "profile_status" => Ok(self.status()),
            "profile_get" | "profile_export" => {
                let document = self.document(required_text(request, "id")?)?;
                Ok(json!({"document":document}))
            }
            "profile_apply" => Err(
                "Profile output activation is unavailable until P4; no outputs were started."
                    .into(),
            ),
            "profile_select" => {
                let id = required_text(request, "id")?;
                self.document(id)?;
                let source = selection_mode(required_text(request, "selectionSource")?)?;
                self.session.select(id, source);
                Ok(self.status())
            }
            "profile_pause" => {
                let paused = request["paused"]
                    .as_bool()
                    .ok_or("paused must be a boolean.")?;
                self.session.set_paused(paused)?;
                Ok(self.status())
            }
            "profile_selection_mode" => {
                self.session
                    .set_mode(selection_mode(required_text(request, "mode")?)?);
                Ok(self.status())
            }
            "profile_reset" => {
                if request["confirm"].as_bool() != Some(true) {
                    return Err(
                        "Reset requires confirm: true and replaces only the new profile library."
                            .into(),
                    );
                }
                let library = Library::default();
                self.persist(&library)?;
                self.library = library;
                self.session = Session::default();
                self.read_only = false;
                self.diagnostics.clear();
                Ok(self.status())
            }
            _ => {
                if self.read_only {
                    return Err("Profile storage is read-only after a load error; repair it or explicitly reset the new library.".into());
                }
                self.mutate(command, request)
            }
        }
    }

    fn mutate(&mut self, command: &str, request: &Value) -> Result<Value, String> {
        let mut library = self.library.clone();
        let output = match command {
            "profile_create" => {
                self.ensure_room()?;
                let document = new_document(required_text(request, "name")?)?;
                library.profiles.push(document.clone());
                document
            }
            "profile_import" | "profile_duplicate" => {
                self.ensure_room()?;
                let mut document = if command == "profile_import" {
                    decode_document(&request["document"])?
                } else {
                    self.document(required_text(request, "id")?)?.clone()
                };
                let name = optional_text(request, "name")?
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        if command == "profile_duplicate" {
                            format!(
                                "{} copy",
                                document.name.chars().take(75).collect::<String>()
                            )
                        } else {
                            document.name.clone()
                        }
                    });
                document.id = new_document(&name)?.id;
                document.name = name.trim().to_owned();
                document.revision = 1;
                // No assignment entry is copied; imported JSON contains only a
                // portable document, never local bindings or runtime state.
                library.profiles.push(document.clone());
                document
            }
            "profile_rename" => {
                let id = required_text(request, "id")?;
                let expected = expected_revision(request)?;
                let stored = library
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == id)
                    .ok_or("Profile not found.")?;
                check_revision(stored, expected)?;
                let name = new_document(required_text(request, "name")?)?.name;
                stored.name = name;
                stored.revision = next_revision(stored.revision)?;
                stored.clone()
            }
            "profile_save" => {
                let mut document = decode_document(&request["document"])?;
                let expected = expected_revision(request)?;
                let stored = library
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == document.id)
                    .ok_or("Profile not found.")?;
                check_revision(stored, expected)?;
                if document.revision != expected {
                    return Err("The document revision must match expectedRevision.".into());
                }
                document.revision = next_revision(expected)?;
                *stored = document.clone();
                if let Some(assignments) = library.local_assignments.get_mut(&document.id) {
                    assignments
                        .retain(|a| document.resources.iter().any(|r| r.id == a.resource_id));
                }
                document
            }
            "profile_delete" => {
                let id = required_text(request, "id")?;
                if id == DEFAULT_PROFILE_ID {
                    return Err("The Default profile cannot be deleted.".into());
                }
                check_revision(self.document(id)?, expected_revision(request)?)?;
                library.profiles.retain(|p| p.id != id);
                library.local_assignments.remove(id);
                self.persist(&library)?;
                self.library = library;
                self.session.fallback_if_removed(id);
                return Ok(json!({"deletedId":id,"session":self.session.status()}));
            }
            _ => return Err("Unknown profile mutation.".into()),
        };
        self.persist(&library)?;
        self.library = library;
        Ok(json!({"document":output,"session":self.session.status()}))
    }

    fn persist(&self, library: &Library) -> Result<(), String> {
        library.encode()?;
        if let Some(store) = &self.store {
            store.save(library)?;
        }
        Ok(())
    }

    fn document(&self, id: &str) -> Result<&ProfileDocument, String> {
        self.library
            .profiles
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| "Profile not found.".into())
    }

    fn ensure_room(&self) -> Result<(), String> {
        if self.library.profiles.len() >= MAX_PROFILES {
            return Err("Delete an unused profile before adding another; the limit is 32.".into());
        }
        Ok(())
    }

    fn status(&self) -> Value {
        json!({"session":self.session.status(),"diagnostics":self.diagnostics,"readOnly":self.read_only})
    }
}

fn required_text<'a>(request: &'a Value, field: &str) -> Result<&'a str, String> {
    request[field]
        .as_str()
        .ok_or_else(|| format!("{field} must be a string."))
}

fn optional_text<'a>(request: &'a Value, field: &str) -> Result<Option<&'a str>, String> {
    if request.get(field).is_none() {
        Ok(None)
    } else {
        required_text(request, field).map(Some)
    }
}

fn selection_mode(mode: &str) -> Result<SelectionMode, String> {
    match mode {
        "manual" => Ok(SelectionMode::Manual),
        "automatic" => Ok(SelectionMode::Automatic),
        _ => Err("Selection mode must be manual or automatic.".into()),
    }
}

fn expected_revision(request: &Value) -> Result<u64, String> {
    request["expectedRevision"]
        .as_u64()
        .filter(|r| (1..=MAX_REVISION).contains(r))
        .ok_or_else(|| "A valid expectedRevision is required.".into())
}

fn check_revision(document: &ProfileDocument, expected: u64) -> Result<(), String> {
    if document.revision != expected {
        return Err(format!(
            "Profile revision conflict: expected {expected}, current {}.",
            document.revision
        ));
    }
    Ok(())
}

fn next_revision(revision: u64) -> Result<u64, String> {
    revision
        .checked_add(1)
        .filter(|r| *r <= MAX_REVISION)
        .ok_or_else(|| "Profile revision limit reached.".into())
}

fn decode_document(value: &Value) -> Result<ProfileDocument, String> {
    ProfileDocument::parse(&serde_json::to_vec(value).map_err(|e| e.to_string())?)
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
