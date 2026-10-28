//! Named-profile CRUD and session metadata. This boundary cannot execute an
//! output, change HidHide, start integrations, or publish companion layouts.
use crate::session::{SelectionMode, Session};
use crate::store::{Library, LocalAssignment, Store};
use crate::{
    new_document, semantic_hash, ProfileDocument, DEFAULT_PROFILE_ID, MAX_PROFILES, MAX_REVISION,
};
use serde_json::{json, Value};
use std::path::PathBuf;

#[path = "virtual_slots.rs"]
pub mod virtual_slots;

pub struct Service {
    library: Library,
    store: Option<Store>,
    session: Session,
    diagnostics: Vec<String>,
    read_only: bool,
}

impl Service {
    /// Portable saved graphs for native selection, without hashing a UI projection.
    pub fn saved_documents(&self) -> &[ProfileDocument] { &self.library.profiles }

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
        let request_limit = if matches!(request["command"].as_str(), Some("profile_editor_save" | "profile_draft_save")) {
            crate::MAX_EDITOR_REQUEST_BYTES
        } else {
            crate::MAX_PROFILE_BYTES + 64 * 1024
        };
        if serde_json::to_vec(request)
            .map_err(|e| e.to_string())?
            .len()
            > request_limit
        {
            return Err("Profile request exceeds its size limit.".into());
        }
        let command = request["command"]
            .as_str()
            .ok_or("A profile command is required.")?;
        let fields: &[&str] = match command {
            "profile_list" | "profile_status" | "profile_companion_references" => &[],
            "profile_apply" => &["id"],
            "profile_get" | "profile_export" | "profile_assignments_get" | "profile_virtual_slots_get" => &["id"],
            "profile_virtual_slot_set" => &["id", "expectedRevision", "nodeId", "slot"],
            "profile_assignments_save" => &["id", "expectedRevision", "assignments"],
            "profile_create" => &["name"],
            "profile_rename" => &["id", "name", "expectedRevision"],
            "profile_duplicate" => &["id", "name", "expectedRevision"],
            "profile_delete" => &["id", "expectedRevision"],
            "profile_save" => &["document", "expectedRevision"],
            "profile_editor_save" | "profile_draft_save" => &["document", "expectedRevision", "assignments"],
            "profile_import" => &["document", "name"],
            "profile_select" => &["id", "selectionSource"],
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
                            "gameIds": document.eligibility.game_ids,
                        }))
                    })
                    .collect::<Result<Vec<Value>, String>>()?;
                Ok(json!({"profiles":profiles,"session":self.session.status(),
                    "diagnostics":self.diagnostics,"readOnly":self.read_only}))
            }
            "profile_status" => Ok(self.status()),
            "profile_companion_references" => Ok(json!({"publicationReferences":self.companion_references()?})),
            "profile_get" | "profile_export" => {
                let document = self.document(required_text(request, "id")?)?;
                Ok(json!({"document":document}))
            }
            "profile_virtual_slots_get" => self.virtual_slots_response(required_text(request, "id")?),
            "profile_virtual_slot_set" => self.set_virtual_slot(request),
            "profile_assignments_get" => {
                let id = required_text(request, "id")?;
                let (document, assignments, read_only) = self.compile_source(id)?;
                Ok(
                    json!({"document":document,"assignments":assignments,"session":self.session.status(),"readOnly":read_only}),
                )
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
                if let Some(store)=&self.store {store.reset(&library)?;}
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

    /// Saved graph references are independent of editor selection and live Hub
    /// assignments. Hold the Service lock through pruning when consuming them.
    pub fn companion_references(&self) -> Result<Value, String> {
        use sha2::{Digest, Sha256};
        if self.read_only {
            return Err("Profile storage is read-only; preserve companion publications until its saved references can be verified.".into());
        }
        let hashes = self.library.profiles.iter()
            .flat_map(|document| document.companion_assignments.iter().map(|assignment| assignment.publication_hash.as_str()))
            .collect::<std::collections::BTreeSet<_>>();
        let bytes = serde_json::to_vec(&self.library).map_err(|error| error.to_string())?;
        Ok(json!({"schemaVersion":1,"libraryFingerprint":format!("sha256:{:x}",Sha256::digest(bytes)),"hashes":hashes}))
    }

    /// Native saved-panel propagation. This is intentionally absent from the
    /// browser command allowlist: only the verified panel save supplies aliases.
    /// Stage the entire library so validation, revision or storage failure cannot
    /// leave some profiles following a different saved panel than the others.
    pub fn refresh_companion_panel(
        &mut self,
        panel_id: &str,
        publication_hash: &str,
        previous_hashes: &std::collections::BTreeSet<String>,
    ) -> Result<Vec<String>, String> {
        if self.read_only {
            return Err("Profile storage is read-only; the saved panel cannot update its profile references.".into());
        }
        if !crate::validation::companion_panel_identifier(panel_id)
            || !crate::validation::companion_publication_hash(publication_hash)
            || previous_hashes.iter().any(|hash| !crate::validation::companion_publication_hash(hash))
        {
            return Err("A valid saved panel identity and immutable publication hashes are required.".into());
        }
        let mut library = self.library.clone();
        let mut changed = vec![];
        for document in &mut library.profiles {
            let mut updated = false;
            for assignment in &mut document.companion_assignments {
                let follows_panel = assignment.panel_id.as_deref() == Some(panel_id)
                    || (assignment.panel_id.is_none()
                        && (previous_hashes.contains(&assignment.publication_hash)
                            || assignment.publication_hash == publication_hash));
                if follows_panel && (assignment.panel_id.as_deref() != Some(panel_id)
                    || assignment.publication_hash != publication_hash)
                {
                    assignment.panel_id = Some(panel_id.into());
                    assignment.publication_hash = publication_hash.into();
                    updated = true;
                }
            }
            if updated {
                document.revision = next_revision(document.revision)?;
                crate::validate_document(document)?;
                changed.push(document.id.clone());
            }
        }
        if !changed.is_empty() {
            self.persist(&library)?;
            self.library = library;
        }
        Ok(changed)
    }

    fn mutate(&mut self, command: &str, request: &Value) -> Result<Value, String> {
        let mut library = self.library.clone();
        let output = match command {
            "profile_editor_save" | "profile_draft_save" => {
                let mut document = decode_document(&request["document"])?;
                let assignments: Vec<LocalAssignment> =
                    serde_json::from_value(request["assignments"].clone())
                        .map_err(|error| format!("Invalid local assignments: {error}"))?;
                match request.get("expectedRevision") {
                    Some(Value::Null) => {
                        if library
                            .profiles
                            .iter()
                            .any(|stored| stored.id == document.id)
                        {
                            return Err("A new editor profile requires a fresh identity; this profile already exists.".into());
                        }
                        if document.revision != 1 {
                            return Err("A new editor profile must start at revision 1.".into());
                        }
                        self.ensure_room()?;
                        library.profiles.push(document.clone());
                    }
                    Some(_) => {
                        let expected = expected_revision(request)?;
                        let stored = library
                            .profiles
                            .iter_mut()
                            .find(|stored| stored.id == document.id)
                            .ok_or("Profile not found.")?;
                        check_revision(stored, expected)?;
                        if document.revision != expected {
                            return Err("The document revision must match expectedRevision.".into());
                        }
                        document.revision = next_revision(expected)?;
                        *stored = document.clone();
                    }
                    None => {
                        return Err(
                            "An explicit null or numeric expectedRevision is required.".into()
                        )
                    }
                }
                // Validate the complete graph and exact assignment set together,
                // then perform one bounded atomic replacement. Memory and the
                // session remain unchanged if validation or persistence fails.
                library
                    .local_assignments
                    .insert(document.id.clone(), assignments.clone());
                let profiles = library.profiles.iter().map(|profile| {
                    Ok(json!({"id":profile.id,"name":profile.name,"revision":profile.revision,
                        "isDefault":profile.id == DEFAULT_PROFILE_ID,"semanticHash":semantic_hash(profile)?,"gameIds":profile.eligibility.game_ids}))
                }).collect::<Result<Vec<Value>, String>>()?;
                library.prune_virtual_slots();
                self.persist(&library)?;
                self.library = library;
                return Ok(
                    json!({"document":document,"assignments":assignments,"profiles":profiles,"session":self.session.status()}),
                );
            }
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
                    let source = self.document(required_text(request, "id")?)?;
                    if request.get("expectedRevision").is_some() {
                        check_revision(source, expected_revision(request)?)?;
                    }
                    source.clone()
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
            "profile_assignments_save" => {
                let id = required_text(request, "id")?;
                let expected = expected_revision(request)?;
                let assignments: Vec<LocalAssignment> =
                    serde_json::from_value(request["assignments"].clone())
                        .map_err(|e| format!("Invalid local assignments: {e}"))?;
                let document = library
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == id)
                    .ok_or("Profile not found.")?;
                check_revision(document, expected)?;
                document.revision = next_revision(expected)?;
                let output = document.clone();
                library
                    .local_assignments
                    .insert(id.to_owned(), assignments.clone());
                library.prune_virtual_slots();
                self.persist(&library)?;
                self.library = library;
                return Ok(
                    json!({"document":output,"assignments":assignments,"session":self.session.status()}),
                );
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
                    assignments.retain(|a| {
                        document.resources.iter().any(|r| {
                            r.id == a.resource_id
                                && r.ports.len() == a.ports.len()
                                && r.ports.iter().all(|p| {
                                    a.ports
                                        .iter()
                                        .any(|expected| expected.profile_port_id == p.id)
                                })
                        })
                    });
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
                library.prune_virtual_slots();
                self.persist(&library)?;
                self.library = library;
                self.session.fallback_if_removed(id);
                return Ok(json!({"deletedId":id,"session":self.session.status()}));
            }
            _ => return Err("Unknown profile mutation.".into()),
        };
        library.prune_virtual_slots();
        self.persist(&library)?;
        self.library = library;
        Ok(json!({"document":output,"session":self.session.status()}))
    }

    /// Captures saved authoring/local binding data without changing runtime state or acquiring a device.
    pub fn compile_source(
        &self,
        id: &str,
    ) -> Result<(ProfileDocument, Vec<LocalAssignment>, bool), String> {
        Ok((
            self.document(id)?.clone(),
            self.library
                .local_assignments
                .get(id)
                .cloned()
                .unwrap_or_default(),
            self.read_only,
        ))
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

#[cfg(test)]
#[path = "service_editor_tests.rs"]
mod editor_tests;
