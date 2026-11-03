//! A bounded host-controller inventory. OS paths are observations, never portable IDs.
use crate::stream::{Event, Journal, Page, Reason, Snapshot, SourceSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
pub const MAX_SOURCES: usize = 256;
pub const MAX_CONTROLS: usize = 256;
pub const PAGE_SIZE: usize = 32;
include!("registry/model.rs");
mod input;
mod metadata;
mod observation;
mod snapshots;
#[derive(Clone)]
pub struct Registry {
    sources: BTreeMap<String, Source>,
    observations: observation::Observations,
    journal: Journal,
    pub backend_status: String,
    pub diagnostic: String,
    pub session: String,
    pub dirty: bool,
}
impl Default for Registry {
    fn default() -> Self {
        let journal = Journal::default();
        let session = journal.clock().id().to_string();
        Self {
            sources: BTreeMap::new(),
            observations: observation::Observations::default(),
            journal,
            backend_status: "starting".into(),
            diagnostic: String::new(),
            session,
            dirty: false,
        }
    }
}
impl Registry {
    pub(crate) fn capture_clock(&self) -> crate::stream::Clock { self.journal.clock() }
    /// Wake-only callback: it must not collect state, perform I/O or take a
    /// routing lock. The receiver reads committed reports after this lock exits.
    pub fn watch_reports(&mut self, ids: &[String], wake: std::sync::Arc<dyn Fn() + Send + Sync>) -> Result<crate::stream::ReportWatch, String> {
        if ids.is_empty() || ids.len() > 16
            || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
            || ids.iter().any(|id| !self.sources.contains_key(id)) {
            return Err("Select 1 to 16 distinct known physical controllers.".into());
        }
        self.journal.watch(ids, wake)
    }
    pub fn resource_inventory(&self) -> ResourceInventory {
        ResourceInventory {
            session: self.session.clone(),
            backend_status: self.backend_status.clone(),
            diagnostic: self.diagnostic.clone(),
            sources: self
                .sources
                .values()
                .map(|source| ResourceDescriptor {
                    saved: source.saved.clone(),
                    online: source.online,
                    generation: source.generation,
                    baseline_ready: source.ready(),
                    controls: source.controls.clone(),
                    diagnostics: source.diagnostics.clone(),
                })
                .collect(),
        }
    }

    pub fn restore(&mut self, records: Vec<SavedSource>) -> Result<(), String> {
        if records.len() > MAX_SOURCES {
            return Err("Too many remembered controllers.".into());
        }
        let mut sources = BTreeMap::new();
        for saved in records {
            if uuid::Uuid::parse_str(&saved.id).is_err()
                || !saved.identity.valid()
                || !valid_label(&saved.name)
                || !valid_role(&saved.role)
                || sources.contains_key(&saved.id)
            {
                return Err("Invalid remembered controller identity or metadata.".into());
            }
            sources.insert(
                saved.id.clone(),
                Source {
                    saved,
                    online: false,
                    generation: 0,
                    sequence: 0,
                    captured_us: 0,
                    cursor: 0,
                    controls: vec![],
                    values: BTreeMap::new(),
                    diagnostics: vec![],
                },
            );
        }
        self.sources = sources;
        Ok(())
    }
    pub fn saved(&self) -> Vec<SavedSource> {
        self.sources.values().map(|s| s.saved.clone()).collect()
    }
    pub fn list(&self, offset: usize) -> Value {
        json!({"session":self.session,"backendStatus":self.backend_status,"diagnostic":self.diagnostic,
            "captureClock":{"id":format!("controllers:{}",self.session),"nowUs":self.journal.now().to_string()},
            "sources":self.sources.values().skip(offset).take(PAGE_SIZE).map(Source::summary).collect::<Vec<_>>(),
            "total":self.sources.len(),"offset":offset,"limit":PAGE_SIZE})
    }
    pub fn visibility_sources(
        &self,
        ids: &[String],
    ) -> Result<Vec<crate::visibility::session::Source>, String> {
        self.physical_sources(ids, "windows-raw-input")
    }
    pub fn physical_sources(
        &self,
        ids: &[String],
        backend: &str,
    ) -> Result<Vec<crate::visibility::session::Source>, String> {
        if ids.is_empty() || ids.len() > 16 {
            return Err("Select 1 to 16 physical controllers.".into());
        }
        ids.iter()
            .map(|id| {
                let source = self
                    .sources
                    .get(id)
                    .filter(|source| source.ready())
                    .ok_or("Selected controller requires a complete current baseline.")?;
                let identity = &source.saved.identity;
                if identity.backend != backend
                    || (identity.vendor_id == 0x1234 && identity.product_id == 0xbead)
                {
                    return Err(
                        "Selection requires an observed physical controller from the current native backend."
                            .into(),
                    );
                }
                Ok(crate::visibility::session::Source {
                    id: id.clone(),
                    session: self.session.clone(),
                    generation: source.generation,
                    sequence: source.sequence,
                    interface: identity.path.clone(),
                })
            })
            .collect()
    }
    pub fn inspect(&self, id: &str, offset: usize) -> Result<Value, String> {
        let s = self.sources.get(id).ok_or("Unknown controller.")?;
        Ok(
            json!({"source":s.summary(),"identity":s.saved.identity,"diagnostics":s.diagnostics,
            "controls":s.controls.iter().skip(offset).take(PAGE_SIZE).map(|c| json!({"capability":c,"value":s.values.get(&c.id)})).collect::<Vec<_>>(),
            "offset":offset,"total":s.controls.len(),"limit":PAGE_SIZE}),
        )
    }
    pub fn update(&mut self, id: &str, name: &str, role: &str) -> Result<(), String> {
        if !valid_label(name) || !valid_role(role) {
            return Err("Use a name of 1–80 characters and a supported role.".into());
        }
        let s = self.sources.get_mut(id).ok_or("Unknown controller.")?;
        s.saved.name = name.trim().into();
        s.saved.role = role.into();
        self.dirty = true;
        self.observation_metadata();
        Ok(())
    }
    pub fn forget(&mut self, id: &str) -> Result<(), String> {
        if self.sources.get(id).ok_or("Unknown controller.")?.online {
            return Err("Disconnect the controller before forgetting it.".into());
        }
        self.sources.remove(id);
        self.dirty = true;
        self.observation_metadata();
        Ok(())
    }
    /// Reassign only an offline record to a compatible live observation, preserving its local UUID.
    pub fn reassign(&mut self, remembered: &str, observed: &str) -> Result<(), String> {
        let previous = self
            .sources
            .get(remembered)
            .ok_or("Unknown remembered controller.")?;
        let next = self
            .sources
            .get(observed)
            .ok_or("Unknown observed controller.")?;
        if previous.online
            || !next.online
            || !previous.saved.identity.compatible(&next.saved.identity)
        {
            return Err(
                "Reassignment requires an offline identity and a compatible connected controller."
                    .into(),
            );
        }
        let mut replacement = next.clone();
        replacement.saved.id = remembered.into();
        replacement.saved.name = previous.saved.name.clone();
        replacement.saved.role = previous.saved.role.clone();
        replacement.generation = previous.generation + 1;
        replacement.sequence = 0;
        replacement.values.clear();
        self.disconnect(observed);
        self.sources.remove(observed);
        self.sources.insert(remembered.into(), replacement);
        self.dirty = true;
        self.observation_metadata();
        Ok(())
    }
    /// Captures selected current baselines and the cursor under the same lock.
    pub fn id_for_path(&self, path: &str) -> Option<String> {
        self.sources
            .iter()
            .find(|(_, s)| s.online && s.saved.identity.path == path)
            .map(|(id, _)| id.clone())
    }
}
fn valid_label(value: &str) -> bool {
    !value.trim().is_empty() && value.chars().count() <= 80 && !value.chars().any(char::is_control)
}
fn valid_role(value: &str) -> bool {
    [
        "controller",
        "joystick",
        "wheelbase",
        "pedals",
        "throttle",
        "panel",
    ]
    .contains(&value)
}

#[cfg(test)]
#[path = "registry/identity_tests.rs"]
mod identity_tests;
