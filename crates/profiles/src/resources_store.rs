//! Bounded inventory metadata persistence. Live ownership and values are absent
//! from this envelope, so restoring it can only produce offline references.
use crate::resources::SavedRegistry;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

pub const MAX_INVENTORY_BYTES: usize = 128 * 1024 * 1024;
#[derive(Clone)]
pub struct Store {
    root: PathBuf,
}
pub type ResourceStore = Store;

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn load(&self) -> Result<Option<SavedRegistry>, String> {
        self.check_path(false)?;
        let file = match std::fs::File::open(self.root.join("inventory.json")) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("Cannot open resource inventory: {error}")),
        };
        let mut bytes = Vec::new();
        file.take(MAX_INVENTORY_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > MAX_INVENTORY_BYTES {
            return Err("Resource inventory exceeds 128 MiB.".into());
        }
        let saved: SavedRegistry = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Cannot parse resource inventory: {e}"))?;
        saved.validate()?;
        Ok(Some(saved))
    }
    pub fn save(&self, saved: &SavedRegistry) -> Result<(), String> {
        self.save_with(saved, |temporary, target| {
            temporary
                .persist(target)
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
    }
    fn save_with(
        &self,
        saved: &SavedRegistry,
        replace: impl FnOnce(tempfile::NamedTempFile, &Path) -> Result<(), String>,
    ) -> Result<(), String> {
        saved.validate()?;
        let bytes = serde_json::to_vec(saved).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_INVENTORY_BYTES {
            return Err("Resource inventory exceeds 128 MiB.".into());
        }
        self.check_path(true)?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(&self.root).map_err(|e| e.to_string())?;
        temporary.write_all(&bytes).map_err(|e| e.to_string())?;
        temporary.as_file().sync_all().map_err(|e| e.to_string())?;
        #[cfg(unix)]
        let directory = std::fs::File::open(&self.root).map_err(|e| e.to_string())?;
        self.check_path(false)?;
        replace(temporary, &self.root.join("inventory.json"))?;
        // Replacement commits the new file. Do not report a false rollback if
        // best-effort directory syncing fails after that commit boundary.
        #[cfg(unix)]
        let _ = directory.sync_all();
        Ok(())
    }
    fn check_path(&self, create: bool) -> Result<(), String> {
        if !self.root.is_absolute()
            || self
                .root
                .components()
                .any(|c| matches!(c, Component::ParentDir))
        {
            return Err(
                "Inventory storage requires an absolute path without parent traversal.".into(),
            );
        }
        let mut checked = PathBuf::new();
        let mut components = self.root.components().peekable();
        while let Some(component) = components.next() {
            checked.push(component.as_os_str());
            // A Windows prefix such as \\?\C: is not the complete root.
            // Check it with its following separator, then every full ancestor.
            if matches!(component, Component::Prefix(_))
                && matches!(components.peek(), Some(Component::RootDir))
            {
                continue;
            }
            match std::fs::symlink_metadata(&checked) {
                Ok(metadata) => {
                    reject_link(&metadata)?;
                    if !metadata.is_dir() {
                        return Err("An inventory storage parent is not a directory.".into());
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    if create {
                        std::fs::create_dir(&checked).map_err(|e| e.to_string())?;
                        reject_link(
                            &std::fs::symlink_metadata(&checked).map_err(|e| e.to_string())?,
                        )?;
                    }
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        match std::fs::symlink_metadata(self.root.join("inventory.json")) {
            Ok(metadata) => {
                reject_link(&metadata)?;
                if !metadata.is_file() {
                    return Err("Resource inventory is not a regular file.".into());
                }
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}
fn reject_link(metadata: &std::fs::Metadata) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Inventory storage does not follow reparse points.".into());
        }
    }
    if metadata.file_type().is_symlink() {
        return Err("Inventory storage does not follow symlinks.".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "resources_store_tests.rs"]
mod tests;
