//! One-time destructive retirement of precompiled widget publications and their endpoint mappings.
//! Pairing credentials, listener/TLS state and unrelated profile nodes are outside this reset.
use osg_companion_service::{Hub, Publication, PublicationUpload};
use osg_profiles::{resources::ResourceType, ResourceKind};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Component, Path},
};

const CACHE_LIMIT: u64 = 80 * 1024 * 1024;
const PROFILE_MARKER: &str = "widget-profile-cutover-v4.json";
const PANEL_MARKER: &str = "widget-panel-cutover-v4.json";

fn check_root(directory: &Path) -> Result<(), String> {
    if !directory.is_absolute()
        || directory
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(
            "Widget cutover requires an absolute storage directory without traversal.".into(),
        );
    }
    let mut checked = std::path::PathBuf::new();
    for component in directory.components() {
        checked.push(component.as_os_str());
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&checked) {
            Ok(metadata) => {
                regular(&metadata)?;
                if !metadata.is_dir() {
                    return Err("Widget cutover parent is not a directory.".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}
fn regular(metadata: &fs::Metadata) -> Result<(), String> {
    let linked = metadata.file_type().is_symlink();
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        linked || metadata.file_attributes() & 0x400 != 0
    };
    if linked {
        Err("Widget cutover does not follow linked storage paths.".into())
    } else {
        Ok(())
    }
}
fn read(directory: &Path, name: &str, limit: u64) -> Result<Option<Vec<u8>>, String> {
    check_root(directory)?;
    let path = directory.join(name);
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            regular(&metadata)?;
            if !metadata.is_file() {
                return Err("Widget cutover expected a regular storage file.".into());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    }
    let mut bytes = vec![];
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("Widget cutover storage exceeds its bounded envelope.".into());
    }
    Ok(Some(bytes))
}
fn write(directory: &Path, name: &str, value: &Value) -> Result<(), String> {
    check_root(directory)?;
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let path = directory.join(name);
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        regular(&metadata)?;
        if !metadata.is_file() {
            return Err("Widget cutover cannot replace a non-file.".into());
        }
    }
    let mut temporary = tempfile::NamedTempFile::new_in(directory).map_err(|e| e.to_string())?;
    temporary
        .write_all(&serde_json::to_vec(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    temporary.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
fn complete(directory: &Path, name: &str) -> Result<bool, String> {
    let Some(bytes) = read(directory, name, 4096)? else {
        return Ok(false);
    };
    let marker: Value =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid widget cutover completion record.")?;
    if marker["version"] != 4 || marker["complete"] != true {
        return Err("Invalid widget cutover completion record.".into());
    }
    Ok(true)
}
fn retained(directory: &Path) -> Result<(Vec<PublicationUpload>, usize), String> {
    let Some(bytes) = read(directory, "companion-panels.json", CACHE_LIMIT)? else {
        return Ok((vec![], 0));
    };
    let uploads: Vec<PublicationUpload> = match serde_json::from_slice(&bytes) {
        Ok(uploads) => uploads,
        Err(_) => return Ok((vec![], 1)),
    };
    if uploads.len() > 32 {
        return Ok((vec![], uploads.len()));
    }
    let total = uploads.len();
    let retained = uploads
        .into_iter()
        .filter(|upload| {
            serde_json::from_str::<Value>(&upload.canonical_document)
                .is_ok_and(|document| document["schemaVersion"] == 4)
                && Publication::validate(upload.clone()).is_ok()
        })
        .collect::<Vec<_>>();
    let removed = total - retained.len();
    Ok((retained, removed))
}

/// Runs synchronously when the native companion runtime is constructed, before profile stores load.
pub(crate) fn profiles(directory: &Path) -> Result<Option<String>, String> {
    profiles_with_finish(directory, |report| write(directory, PROFILE_MARKER, report))
}
fn profiles_with_finish(
    directory: &Path,
    finish: impl FnOnce(&Value) -> Result<(), String>,
) -> Result<Option<String>, String> {
    if complete(directory, PROFILE_MARKER)? {
        return Ok(None);
    }
    let parent = directory
        .parent()
        .ok_or("Widget storage has no application directory.")?;
    let (valid, _) = retained(directory)?;
    let hashes = valid
        .iter()
        .map(|p| p.publication_hash.clone())
        .collect::<BTreeSet<_>>();
    let profile_store = osg_profiles::store::Store::new(parent.join("profiles-v2"));
    let mut changed_profiles = 0;
    let mut had_store = false;
    let mut removed_resources = 0;
    let mut removed_bindings = 0;
    if let Some(mut library) = profile_store.load()? {
        had_store = true;
        for document in &mut library.profiles {
            let keep = document
                .companion_assignments
                .iter()
                .filter(|a| hashes.contains(&a.publication_hash))
                .map(|a| a.resource_id.clone())
                .collect::<BTreeSet<_>>();
            let removed = document
                .resources
                .iter()
                .filter(|r| r.kind == ResourceKind::Companion && !keep.contains(&r.id))
                .map(|r| r.id.clone())
                .collect::<BTreeSet<_>>();
            if removed.is_empty() {
                continue;
            }
            let before = document.bindings.len();
            document.resources.retain(|r| !removed.contains(&r.id));
            document
                .bindings
                .retain(|b| !removed.contains(&b.from.node_id) && !removed.contains(&b.to.node_id));
            document
                .companion_assignments
                .retain(|a| !removed.contains(&a.resource_id));
            document
                .visibility
                .retain(|v| !removed.contains(&v.resource_id));
            document
                .overrides
                .retain(|v| !removed.contains(&v.resource_id));
            document
                .editor
                .positions
                .retain(|p| !removed.contains(&p.node_id));
            document
                .editor
                .collapsed_port_groups
                .retain(|g| !removed.contains(&g.node_id));
            for group in &mut document.editor.groups {
                group.members.retain(|id| !removed.contains(id));
            }
            for tab in &mut document.vehicle_tabs {
                tab.node_ids.retain(|id| !removed.contains(id));
            }
            let binding_ids = document
                .bindings
                .iter()
                .map(|b| b.id.clone())
                .collect::<BTreeSet<_>>();
            document
                .editor
                .wire_routes
                .retain(|r| binding_ids.contains(&r.binding_id));
            if let Some(assignments) = library.local_assignments.get_mut(&document.id) {
                assignments.retain(|a| !removed.contains(&a.resource_id));
            }
            document.revision = document
                .revision
                .checked_add(1)
                .filter(|revision| *revision <= osg_profiles::MAX_REVISION)
                .ok_or("Profile revision exhausted during widget cutover.")?;
            osg_profiles::validate_document(document)?;
            changed_profiles += 1;
            removed_resources += removed.len();
            removed_bindings += before - document.bindings.len();
        }
        if changed_profiles > 0 {
            profile_store.save(&library)?;
        }
    }
    let inventory_store =
        osg_profiles::resources_store::Store::new(parent.join("profile-resources-v1"));
    let mut removed_catalogues = 0;
    if let Some(mut inventory) = inventory_store.load()? {
        had_store = true;
        for resource in &mut inventory.resources {
            if resource.resource_type != ResourceType::Companion {
                continue;
            }
            for sighting in &mut resource.sightings {
                if sighting
                    .attributes
                    .get("publicationHash")
                    .is_some_and(|hash| hashes.contains(hash))
                {
                    continue;
                }
                let removed = sighting.catalogue.take().is_some();
                let old = sighting.attributes.remove("publicationHash").is_some()
                    | sighting
                        .attributes
                        .remove("capabilityFingerprint")
                        .is_some();
                if removed || old {
                    removed_catalogues += 1;
                    let message =
                        "Old widget endpoint metadata was reset. Save and assign a compiled panel."
                            .to_owned();
                    if !sighting.diagnostics.contains(&message) {
                        sighting.diagnostics.push(message);
                    }
                }
            }
        }
        if removed_catalogues > 0 {
            inventory_store.save(&inventory)?;
        }
    }
    if !had_store {
        return Ok(None);
    }
    let report = json!({"version":4,"complete":true,"profiles":changed_profiles,"resources":removed_resources,"bindings":removed_bindings,"catalogues":removed_catalogues});
    finish(&report)?;
    Ok((changed_profiles+removed_catalogues>0).then(||format!("Widget designer reset removed {removed_resources} old companion resources and {removed_bindings} bindings from {changed_profiles} profiles; unrelated profile settings and paired devices were kept.")))
}

/// Runs after protected Hub loading, before retained publications or transports become available.
pub(crate) fn panels(hub: &Hub, directory: &Path) -> Result<Option<String>, String> {
    panels_with_finish(hub, directory, |report| {
        write(directory, PANEL_MARKER, report)
    })
}
fn remove_retired_archives(directory: &Path) -> Result<usize, String> {
    check_root(directory)?;
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.to_string()),
    };
    let mut removed = 0;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(key) = name
            .strip_prefix("companion-panels.rejected-")
            .and_then(|name| name.strip_suffix(".json"))
        else {
            continue;
        };
        if !uuid::Uuid::parse_str(key).is_ok_and(|id| id.to_string() == key) {
            continue;
        }
        let Some(bytes) = read(directory, name, CACHE_LIMIT)? else {
            continue;
        };
        let retired =
            serde_json::from_slice::<Vec<PublicationUpload>>(&bytes).map_or(true, |uploads| {
                uploads.iter().any(|upload| {
                    !serde_json::from_str::<Value>(&upload.canonical_document)
                        .is_ok_and(|document| document["schemaVersion"] == 4)
                })
            });
        if retired {
            fs::remove_file(directory.join(name)).map_err(|e| e.to_string())?;
            removed += 1;
        }
    }
    Ok(removed)
}
fn panels_with_finish(
    hub: &Hub,
    directory: &Path,
    finish: impl FnOnce(&Value) -> Result<(), String>,
) -> Result<Option<String>, String> {
    if complete(directory, PANEL_MARKER)? {
        return Ok(None);
    }
    let (valid, removed) = retained(directory)?;
    let hashes = valid
        .iter()
        .map(|p| p.publication_hash.clone())
        .collect::<BTreeSet<_>>();
    let status = hub.command(&json!({"command":"companion_status"}))?;
    let assignments = status["instances"]
        .as_array()
        .ok_or("Missing native companion assignments during widget cutover.")?;
    let mut unassigned = 0;
    for assignment in assignments {
        let esp32 = status["clients"].as_array().is_some_and(|clients| {
            clients.iter().any(|client| {
                client["companionId"] == assignment["companionId"] && client["kind"] == "esp32"
            })
        });
        if !esp32
            && assignment["publicationHash"]
                .as_str()
                .is_some_and(|hash| hashes.contains(hash))
        {
            continue;
        }
        hub.command(
            &json!({"command":"companion_unassign","instanceId":assignment["instanceId"]}),
        )?;
        unassigned += 1;
    }
    if read(directory, "companion-panels.json", CACHE_LIMIT)?.is_some() {
        write(
            directory,
            "companion-panels.json",
            &serde_json::to_value(&valid).map_err(|e| e.to_string())?,
        )?;
    }
    if let Some(bytes) = read(directory, "companion-panel-heads.json", 65536)? {
        let mut heads: std::collections::BTreeMap<String, String> =
            serde_json::from_slice(&bytes).unwrap_or_default();
        heads.retain(|id, hash| {
            valid.iter().any(|p| {
                p.publication_hash == *hash
                    && serde_json::from_str::<Value>(&p.canonical_document)
                        .is_ok_and(|d| d["ui"]["id"] == *id)
            })
        });
        write(directory, "companion-panel-heads.json", &json!(heads))?;
    }
    let archives = remove_retired_archives(directory)?;
    let report = json!({"version":4,"complete":true,"publications":removed,"assignments":unassigned,"archives":archives});
    finish(&report)?;
    Ok((removed+unassigned+archives>0).then(||format!("Widget designer reset removed {removed} old publications and unassigned {unassigned} obsolete panels. Pairing, certificates and listener settings were kept.")))
}

#[cfg(test)]
#[path = "widget_cutover_tests.rs"]
mod tests;
