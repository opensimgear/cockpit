use super::*;
use crate::resources::{Registry, SavedRegistry};
use serde_json::json;

#[test]
fn empty_inventory_is_fresh_atomic_and_independent_of_profile_library() {
    let temp = crate::test_directory();
    let store = Store::new(temp.path().join("profiles-v1"));
    assert!(store.load().unwrap().is_none());
    store.save(&SavedRegistry::default()).unwrap();
    let before = std::fs::read(store.root.join("inventory.json")).unwrap();
    assert!(store
        .save_with(&SavedRegistry::default(), |_, _| Err(
            "simulated replacement failure".into()
        ))
        .is_err());
    assert_eq!(
        std::fs::read(store.root.join("inventory.json")).unwrap(),
        before
    );
    assert_eq!(
        Registry::restore(store.load().unwrap().unwrap())
            .unwrap()
            .list(0, 32)
            .unwrap()
            .total,
        0
    );
    assert!(!store.root.join("library.json").exists());
    assert_eq!(std::fs::read_dir(&store.root).unwrap().count(), 1);
}

#[test]
fn corrupt_unknown_runtime_and_oversize_envelopes_are_not_loaded() {
    let temp = crate::test_directory();
    let store = Store::new(temp.path().to_owned());
    let path = temp.path().join("inventory.json");
    std::fs::write(&path, "invalid").unwrap();
    assert!(store.load().is_err());
    let mut value = serde_json::to_value(SavedRegistry::default()).unwrap();
    value["activeRoutes"] = json!([]);
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(store.load().is_err());
    std::fs::File::create(&path)
        .unwrap()
        .set_len(MAX_INVENTORY_BYTES as u64 + 1)
        .unwrap();
    assert!(store.load().unwrap_err().contains("32 MiB"));
    assert!(store
        .save(&SavedRegistry {
            schema_version: 2,
            ..SavedRegistry::default()
        })
        .is_err());
}

#[test]
fn inventory_storage_rejects_traversal_and_non_regular_targets() {
    assert!(Store::new("relative".into()).load().is_err());
    let temp = crate::test_directory();
    assert!(Store::new(temp.path().join("x").join("..")).load().is_err());
    std::fs::create_dir(temp.path().join("inventory.json")).unwrap();
    assert!(Store::new(temp.path().to_owned())
        .save(&SavedRegistry::default())
        .is_err());
    let file = temp.path().join("file");
    std::fs::write(&file, "").unwrap();
    assert!(Store::new(file.join("inventory")).load().is_err());
}

#[test]
fn a_saved_resource_round_trips_offline_and_tampered_port_metadata_is_preserved_as_error() {
    use crate::resources::{
        Availability, ResourceType, SavedCatalogue, SavedResource, SavedSighting,
    };
    use crate::resources_ports::{catalogue_hash, PortContract, PortDescriptor, ScalarSchema};
    let port = PortDescriptor {
        id: "button:0".into(),
        name: "Button".into(),
        direction: crate::Direction::Output,
        contract: PortContract::State {
            schema: ScalarSchema::Boolean {},
            freshness_ms: 1000,
            fallback: Some(crate::PortValue::Boolean(false)),
        },
    };
    let hash = catalogue_hash(&[port.clone()]).unwrap();
    let saved = SavedRegistry {
        schema_version: 1,
        resources: vec![SavedResource {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Controller".into(),
            resource_type: ResourceType::HostController,
            sightings: vec![SavedSighting {
                scope: "controllers".into(),
                key: "observed-source".into(),
                evidence: vec![],
                attributes: Default::default(),
                diagnostics: vec![],
                catalogue: Some(SavedCatalogue {
                    revision: hash.clone(),
                    total: 1,
                    complete: true,
                    ports: vec![port],
                    hash,
                }),
            }],
        }],
        redirects: Default::default(),
    };
    let temp = crate::test_directory();
    let store = Store::new(temp.path().to_owned());
    store.save(&saved).unwrap();
    let restored = Registry::restore(store.load().unwrap().unwrap()).unwrap();
    let summary = &restored.list(0, 32).unwrap().resources[0];
    assert_eq!(summary.availability, Availability::Offline);
    assert_eq!(summary.cached_port_count, 1);
    let path = temp.path().join("inventory.json");
    let mut value = serde_json::to_value(saved).unwrap();
    value["resources"][0]["sightings"][0]["catalogue"]["ports"][0]["contract"]["freshnessMs"] =
        json!(2000);
    let corrupt = serde_json::to_vec(&value).unwrap();
    std::fs::write(&path, &corrupt).unwrap();
    assert!(store.load().unwrap_err().contains("fingerprint"));
    assert_eq!(std::fs::read(&path).unwrap(), corrupt);
}

#[cfg(unix)]
#[test]
fn inventory_storage_refuses_symlinked_parent_and_target() {
    use std::os::unix::fs::symlink;
    let temp = crate::test_directory();
    let real = temp.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let alias = temp.path().join("alias");
    symlink(&real, &alias).unwrap();
    assert!(Store::new(alias).save(&SavedRegistry::default()).is_err());
    let target = temp.path().join("target");
    std::fs::write(&target, "{}").unwrap();
    symlink(&target, real.join("inventory.json")).unwrap();
    assert!(Store::new(real).load().is_err());
}
