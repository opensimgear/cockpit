//! Native applications register all supported application roots against one database
//! before starting workers. Unregistered fixture/standalone namespaces stay local.
use crate::Database;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, RwLock},
};

type Entry = (PathBuf, PathBuf, Arc<Mutex<Option<Database>>>);
static NAMESPACES: OnceLock<RwLock<Vec<Entry>>> = OnceLock::new();

pub fn register_namespace(roots: &[PathBuf], database: &Path) -> Result<(), String> {
    crate::check_path(database, false)?;
    for root in roots {
        crate::check_path(&root.join(".namespace-check"), false)?;
    }
    let mut namespaces = NAMESPACES
        .get_or_init(Default::default)
        .write()
        .map_err(|_| "Persistence namespace access failed.")?;
    for root in roots {
        if namespaces
            .iter()
            .any(|(existing, target, _)| existing == root && target != database)
        {
            return Err("A persistence source root already belongs to another database.".into());
        }
    }
    let shared = namespaces
        .iter()
        .find(|(_, target, _)| target == database)
        .map(|(_, _, pin)| pin.clone())
        .unwrap_or_else(|| Arc::new(Mutex::new(None)));
    for root in roots {
        if !namespaces.iter().any(|(existing, _, _)| existing == root) {
            namespaces.push((root.clone(), database.to_owned(), shared.clone()));
        }
    }
    // Longest matching root wins for explicitly isolated nested installations.
    namespaces.sort_by_key(|(root, _, _)| std::cmp::Reverse(root.components().count()));
    Ok(())
}

fn namespace_path(source: &Path) -> Result<PathBuf, String> {
    if source.is_dir() {
        return Err("A persistence namespace anchor cannot be a directory.".into());
    }
    if !source.is_absolute()
        || source
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("Persistence source requires an absolute path without traversal.".into());
    }
    let parent = source
        .parent()
        .ok_or("Persistence path has no namespace directory.")?;
    crate::check_path(&parent.join(".namespace-check"), false)?;
    if let Some(namespaces) = NAMESPACES.get() {
        let namespaces = namespaces
            .read()
            .map_err(|_| "Persistence namespace access failed.")?;
        if let Some((_, database, _)) = namespaces
            .iter()
            .find(|(root, _, _)| source.starts_with(root))
        {
            return Ok(database.clone());
        }
    }
    let parent = source
        .parent()
        .ok_or("Persistence source has no namespace directory.")?;
    let nested = matches!(
        parent.file_name().and_then(|s| s.to_str()),
        Some("profiles-v2" | "profile-resources-v1" | "companions" | "mappings")
    );
    let root = if nested {
        parent.parent().ok_or("Invalid persistence namespace.")?
    } else {
        parent
    };
    Ok(root.join("cockpit.sqlite3"))
}

pub(crate) fn evidence_key(source: &Path) -> Result<String, String> {
    crate::check_path(source, false)?;
    let database = namespace_path(source)?;
    let namespaces = NAMESPACES
        .get()
        .map(|n| n.read().map_err(|_| "Persistence namespace access failed."))
        .transpose()?;
    let registered = namespaces.as_ref().and_then(|n| {
        n.iter()
            .find(|(root, _, _)| source.starts_with(root))
            .map(|(root, _, _)| root.as_path())
    });
    let root = registered
        .or_else(|| database.parent())
        .ok_or("Invalid evidence namespace.")?;
    let relative = source
        .strip_prefix(root)
        .map_err(|_| "Evidence is outside its namespace.")?;
    let key = relative
        .to_str()
        .ok_or("Invalid evidence identity.")?
        .replace('\\', "/");
    crate::identifier(&key)?;
    Ok(key)
}

fn pinned(source: &Path, create: bool) -> Result<Option<Database>, String> {
    let entry = NAMESPACES
        .get()
        .map(|n| n.read().map_err(|_| "Persistence namespace access failed."))
        .transpose()?
        .and_then(|n| {
            n.iter()
                .find(|(root, _, _)| source.starts_with(root))
                .cloned()
        });
    let Some((_, path, pinned)) = entry else {
        return Ok(None);
    };
    let mut pinned = pinned
        .lock()
        .map_err(|_| "Persistence namespace access failed.")?;
    if let Some(database) = pinned.as_ref() {
        database.connection()?;
        return Ok(Some(database.clone()));
    }
    if !create
        && !path
            .try_exists()
            .map_err(|_| "Cannot inspect the persistence namespace.")?
    {
        return Ok(None);
    }
    let database = if create {
        Database::open(path)?
    } else {
        Database::open_existing(path)?
    };
    *pinned = Some(database.clone());
    Ok(Some(database))
}
impl Database {
    pub fn for_namespace(source: &Path) -> Result<Self, String> {
        let path = namespace_path(source)?;
        if let Some(database) = pinned(source, true)? {
            database.connection()?;
            return Ok(database);
        }
        Self::open(path)
    }
    /// Passive discovery never creates a namespace, database or protection key.
    pub fn existing_for_namespace(source: &Path) -> Result<Option<Self>, String> {
        let path = namespace_path(source)?;
        if let Some(database) = pinned(source, false)? {
            database.connection()?;
            return Ok(Some(database));
        }
        crate::check_path(&path, false)?;
        if !path
            .try_exists()
            .map_err(|_| "Cannot inspect the persistence namespace.")?
        {
            return Ok(None);
        }
        Self::open_existing(path).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn passive_discovery_never_creates_missing_namespaces() {
        let dir = crate::test_directory();
        let anchor = dir.path().join("missing/companions/host-state");
        assert!(Database::existing_for_namespace(&anchor).unwrap().is_none());
        assert!(!dir.path().join("missing").exists());
        let created = Database::for_namespace(&anchor).unwrap();
        assert_eq!(
            Database::existing_for_namespace(&anchor)
                .unwrap()
                .unwrap()
                .path(),
            created.path()
        );
    }
    #[test]
    fn explicit_roots_share_authority_and_other_namespaces_remain_isolated() {
        let dir = crate::test_directory();
        let a = dir.path().join("native");
        let b = dir.path().join("desktop");
        let database = dir.path().join("state/cockpit.sqlite3");
        register_namespace(&[a.clone(), b.clone()], &database).unwrap();
        assert_eq!(
            Database::for_namespace(&a.join("profiles-v2/library.json"))
                .unwrap()
                .path(),
            database
        );
        assert_eq!(
            Database::for_namespace(&b.join("tray.json"))
                .unwrap()
                .path(),
            database
        );
        assert!(register_namespace(&[a], &dir.path().join("other.sqlite3")).is_err());
        assert!(Database::for_namespace(Path::new("relative.json")).is_err());
        let other = dir.path().join("other/profiles-v2/library.json");
        assert_eq!(
            Database::for_namespace(&other).unwrap().path(),
            dir.path().join("other/cockpit.sqlite3")
        );
        let local = dir.path().join("standalone/client.dpapi");
        assert_eq!(
            Database::for_namespace(&local).unwrap().path(),
            dir.path().join("standalone/cockpit.sqlite3")
        );
    }
}
