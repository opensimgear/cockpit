//! Durable resource metadata in the shared SQLite namespace. Restored records
//! remain offline and cannot acquire live provider or output ownership.
use crate::resources::SavedRegistry;
use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};
pub const MAX_INVENTORY_BYTES: usize = 128 * 1024 * 1024;
#[derive(Clone)]
pub struct Store {
    root: PathBuf,
    repository: Arc<OnceLock<osg_persistence::Repository<SavedRegistry>>>,
}
pub type ResourceStore = Store;
impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            repository: Arc::new(OnceLock::new()),
        }
    }
    fn repository(&self) -> Result<&osg_persistence::Repository<SavedRegistry>, String> {
        if self.repository.get().is_none() {
            let source = self.root.join("inventory.json");
            let database = osg_persistence::Database::for_namespace(&source)?;
            let repository = osg_persistence::Repository::new(
                database,
                "resources",
                "inventory",
                MAX_INVENTORY_BYTES,
                SavedRegistry::validate,
            )?;
            let _ = self.repository.set(repository);
        }
        Ok(self.repository.get().expect("repository initialized"))
    }
    pub fn load(&self) -> Result<Option<SavedRegistry>, String> {
        self.repository()?
            .load()
            .map_err(|error| format!("Cannot load resource inventory: {error}"))
    }
    pub fn save(&self, saved: &SavedRegistry) -> Result<(), String> {
        self.repository()?.save(saved)
    }
}

#[cfg(test)]
#[path = "resources_store_tests.rs"]
mod tests;
