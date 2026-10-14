use super::*;
use crate::fixtures::profile;
#[test]
fn bounded_atomic_storage_and_corrupt_files() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().join("profiles"));
    let p = profile();
    assert!(store.ids().unwrap().is_empty());
    assert!(store.load(&p.id).is_err());
    assert!(store.load("../escape").is_err());
    store.import(&p.export().unwrap()).unwrap();
    assert_eq!(store.ids().unwrap(), vec![p.id.clone()]);
    assert_eq!(store.export(&p.id).unwrap(), p.export().unwrap());
    let mut changed = p.clone();
    changed.name = "New draft".into();
    store.save(&changed).unwrap();
    assert_eq!(store.load(&p.id).unwrap().name, "New draft");
    assert!(store.import(b"invalid").is_err());
    assert_eq!(store.load(&p.id).unwrap().name, "New draft");
    std::fs::write(store.path(&p.id).unwrap(), vec![b' '; DOCUMENT_LIMIT + 1]).unwrap();
    assert!(store.load(&p.id).is_err());
    let mut other = p.clone();
    other.id = uuid::Uuid::new_v4().to_string();
    std::fs::write(store.path(&p.id).unwrap(), other.export().unwrap()).unwrap();
    assert!(store.load(&p.id).is_err());
    assert!(store.path(&p.id.to_uppercase()).is_err());
    store.remove(&p.id).unwrap();
    assert!(store.remove(&p.id).is_err());
    std::fs::write(store.root.join("unrelated.txt"), b"leave me").unwrap();
    std::fs::write(store.root.join("not-a-uuid.json"), b"{}").unwrap();
    std::fs::create_dir(store.root.join("directory")).unwrap();
    assert!(store.ids().unwrap().is_empty());
    let blocked = Store::new(store.root.join("unrelated.txt"));
    assert!(blocked.ids().is_err());
    assert!(blocked.save(&p).is_err());
}
#[test]
fn saved_inventory_limit_and_external_symlinks() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let mut p = profile();
    for n in 0..MAX_SAVED {
        p.id = uuid::Uuid::from_u128(n as u128 + 1).to_string();
        store.save(&p).unwrap();
    }
    store.save(&p).unwrap();
    p.id = uuid::Uuid::new_v4().to_string();
    assert!(store.save(&p).is_err());
    std::fs::write(store.path(&p.id).unwrap(), p.export().unwrap()).unwrap();
    assert!(store.ids().is_err());
    #[cfg(unix)]
    {
        let root = tempfile::tempdir().unwrap();
        let store = Store::new(root.path().into());
        let external = root.path().join("external");
        std::fs::write(&external, p.export().unwrap()).unwrap();
        std::os::unix::fs::symlink(&external, store.path(&p.id).unwrap()).unwrap();
        assert!(store.load(&p.id).is_err());
        assert!(store.ids().unwrap().is_empty());
    }
}

#[test]
fn local_assignments_are_separate_validated_and_never_imported_with_profiles() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    let p = profile();
    store.save(&p).unwrap();
    assert!(store.load_assignments(&p.id).is_err());
    let assignments = crate::fixtures::assignments();
    store.save_assignments(&p.id, &assignments).unwrap();
    assert_eq!(store.load_assignments(&p.id).unwrap().len(), 2);
    assert_eq!(store.ids().unwrap(), vec![p.id.clone()]);
    assert_eq!(store.export(&p.id).unwrap(), p.export().unwrap());
    assert!(store.save_assignments(&p.id, &[]).is_err());
    assert_eq!(store.load_assignments(&p.id).unwrap().len(), 2);
    let path = store
        .path(&p.id)
        .unwrap()
        .with_extension("assignments.json");
    for bytes in [
        b"invalid".to_vec(),
        serde_json::to_vec(&LocalAssignments {
            schema: 2,
            profile_id: p.id.clone(),
            assignments: assignments.clone(),
        })
        .unwrap(),
        serde_json::to_vec(&LocalAssignments {
            schema: 1,
            profile_id: "different".into(),
            assignments: assignments.clone(),
        })
        .unwrap(),
        vec![b' '; ASSIGNMENT_LIMIT + 1],
    ] {
        std::fs::write(&path, bytes).unwrap();
        assert!(store.load_assignments(&p.id).is_err());
    }
    store.save_assignments(&p.id, &assignments).unwrap();
    let mut changed = p;
    changed.roles[0].controls[0].id = "changed-role-control".into();
    changed.nodes[0].inputs[0] = crate::profile::Input::Source {
        role: "left".into(),
        control: "changed-role-control".into(),
    };
    changed.nodes[4].inputs[0] = changed.nodes[0].inputs[0].clone();
    store.save(&changed).unwrap();
    assert!(store.load_assignments(&changed.id).is_err());
}
