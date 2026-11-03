//! Committed, bounded profile projections. Observers only capture typed values.
use super::*;
use serde::Serialize;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub name: String,
    pub revision: u64,
    pub is_default: bool,
    pub semantic_hash: String,
    pub game_ids: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub profiles: Vec<Summary>,
    pub session: crate::session::SessionStatus,
    pub diagnostics: Vec<String>,
    pub read_only: bool,
    pub versions: BTreeMap<String, String>,
    #[serde(skip)]
    pub generation: u64,
}
pub type Observer = Arc<dyn Fn(Snapshot) + Send + Sync>;
pub(super) struct Cache {
    pub profiles: Vec<Summary>,
    pub generation: u64,
    versions: BTreeMap<String, u64>,
    observer: Option<Observer>,
    last: Option<Snapshot>,
}
pub(super) fn summaries(library: &Library) -> Result<Vec<Summary>, String> {
    library.profiles.iter().map(|document| Ok(Summary {
        id: document.id.clone(), name: document.name.clone(), revision: document.revision,
        is_default: document.id == DEFAULT_PROFILE_ID, semantic_hash: semantic_hash(document)?,
        game_ids: document.eligibility.game_ids.clone(),
    })).collect()
}
impl Cache {
    pub fn new(library: &Library) -> Self {
        let profiles = summaries(library).expect("loaded profile library was validated");
        let versions = profiles.iter().map(|row| (row.id.clone(), 1)).collect();
        Self { profiles, versions, generation: 1, observer: None, last: None }
    }
    pub fn reset(&mut self, profiles: Vec<Summary>, generation: u64) {
        self.versions = profiles.iter().map(|row| (row.id.clone(), 1)).collect();
        self.profiles = profiles;
        self.generation = generation;
    }
}
impl Service {
    /// Caller owns the Service lock, covering registration and the first capture.
    pub fn observe_read_model(&mut self, observer: Observer) {
        self.read_model.observer = Some(observer);
        self.read_model.last = None;
        self.notify_read_model();
    }
    pub fn read_model_snapshot(&self) -> Snapshot {
        Snapshot { profiles: self.read_model.profiles.clone(), session: self.session.status().clone(),
            diagnostics: self.diagnostics.clone(), read_only: self.read_only,
            versions: self.read_model.versions.iter().map(|(id, version)| (id.clone(), version.to_string())).collect(),
            generation: self.read_model.generation }
    }
    pub(super) fn notify_read_model(&mut self) {
        if self.read_model.observer.is_none() { return; }
        let next = self.read_model_snapshot();
        if self.read_model.last.as_ref() == Some(&next) { return; }
        self.read_model.last = Some(next.clone());
        if let Some(observer) = &self.read_model.observer { observer(next); }
    }
    pub(super) fn commit_library(&mut self, library: Library) -> Result<(), String> {
        self.install_library(library, true)
    }
    pub(super) fn install_library(&mut self, library: Library, notify: bool) -> Result<(), String> {
        // Hashing and counter checks precede the write; failure cannot follow a commit.
        let profiles = summaries(&library)?;
        let mut versions = BTreeMap::new();
        for document in &library.profiles {
            let id = &document.id;
            let unchanged = self.library.profiles.iter().find(|old| old.id == *id) == Some(document)
                && self.library.local_assignments.get(id) == library.local_assignments.get(id)
                && self.library.local_virtual_slots.get(id) == library.local_virtual_slots.get(id);
            let old = self.read_model.versions.get(id).copied().unwrap_or(0);
            versions.insert(id.clone(), if unchanged { old } else { old.checked_add(1).ok_or("Profile local version exhausted.")? });
        }
        self.persist(&library)?;
        self.library = library;
        self.read_model.profiles = profiles;
        self.read_model.versions = versions;
        if notify { self.notify_read_model(); }
        Ok(())
    }
}

#[cfg(test)]
#[path = "read_model/tests.rs"]
mod tests;
