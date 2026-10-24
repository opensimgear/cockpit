use super::*;
use crate::new_document;

#[test]
fn sqlite_library_round_trips_and_stale_saves_preserve_the_committed_document() {
    let temp = crate::test_directory();
    let root = temp.path().join("profiles-v2");
    let store = Store::new(root.clone());
    assert!(store.load().unwrap().is_none());
    let original = Library::default();
    store.save(&original).unwrap();
    let stale = Store::new(root.clone());
    stale.load().unwrap();
    let mut edited = original.clone();
    edited.profiles.push(new_document("New profile").unwrap());
    store.save(&edited).unwrap();
    assert!(stale.save(&original).unwrap_err().contains("conflict"));
    assert_eq!(
        Store::new(root.clone())
            .load()
            .unwrap()
            .unwrap()
            .profiles
            .len(),
        2
    );
    assert!(!root.join("library.json").exists());
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("library.json"), b"stale corrupt source").unwrap();
    assert_eq!(Store::new(root).load().unwrap().unwrap().profiles.len(), 2);
}

#[test]
fn malformed_unknown_and_oversize_envelopes_are_rejected() {
    let temp = crate::test_directory();
    let store = Store::new(temp.path().to_owned());
    let path = temp.path().join("library.json");
    crate::fixture_record(&path, "profiles", "library", b"not json");
    assert!(store.load().is_err());
    let mut value = serde_json::to_value(Library::default()).unwrap();
    value["unexpected"] = serde_json::json!(true);
    crate::fixture_record(
        &path,
        "profiles",
        "library",
        &serde_json::to_vec(&value).unwrap(),
    );
    assert!(store.load().is_err());
    crate::fixture_record(
        &path,
        "profiles",
        "library",
        &vec![b' '; MAX_LIBRARY_BYTES + 1],
    );
    assert!(store.load().unwrap_err().contains("Oversized"));
}

#[test]
fn library_enforces_default_uniqueness_and_count_bounds() {
    let mut library = Library::default();
    library.schema_version = 1;
    assert!(library.validate().is_err());
    library.schema_version = 2;
    library.profiles.clear();
    assert!(library.validate().is_err());
    library.profiles.push(new_document("No default").unwrap());
    assert!(library.validate().is_err());
    library = Library::default();
    library.profiles.push(default_document());
    assert!(library.validate().is_err());
    library.profiles.pop();
    for index in 0..MAX_PROFILES {
        library
            .profiles
            .push(new_document(&format!("Profile {index}")).unwrap());
    }
    assert!(library.validate().is_err());
}

#[test]
fn local_assignments_are_bounded_and_refer_to_existing_logical_resources() {
    let mut library = Library::default();
    let mut document = new_document("Flight").unwrap();
    document.resources.push(crate::Resource {
        id: "stick".into(),
        name: "Stick".into(),
        kind: crate::ResourceKind::Device,
        required: true,
        ports: vec![],
    });
    let id = document.id.clone();
    library.profiles.push(document);
    let assignment = LocalAssignment {
        resource_id: "stick".into(),
        source_id: "hid:machine-local".into(),
        ports: vec![],
    };
    library
        .local_assignments
        .insert(id.clone(), vec![assignment.clone()]);
    library.validate().unwrap();
    library
        .local_assignments
        .insert("missing-profile".into(), vec![]);
    assert!(library.validate().is_err());
    library.local_assignments.remove("missing-profile");
    library
        .local_assignments
        .get_mut(&id)
        .unwrap()
        .push(assignment.clone());
    assert!(library.validate().is_err());
    library.local_assignments.insert(
        id.clone(),
        vec![LocalAssignment {
            resource_id: "missing-resource".into(),
            ..assignment.clone()
        }],
    );
    assert!(library.validate().is_err());
    for source in ["".to_owned(), "newline\n".to_owned(), "x".repeat(1025)] {
        library.local_assignments.insert(
            id.clone(),
            vec![LocalAssignment {
                source_id: source,
                ..assignment.clone()
            }],
        );
        assert!(library.validate().is_err());
    }
    library.local_assignments.insert("extra-a".into(), vec![]);
    library.local_assignments.insert("extra-b".into(), vec![]);
    assert!(library.validate().is_err());
}

#[test]
fn relative_parent_and_nonregular_paths_are_rejected() {
    assert!(Store::new("profiles-v2".into()).load().is_err());
    let temp = crate::test_directory();
    assert!(
        Store::new(temp.path().join("..absolutely-not-a-parent").join(".."))
            .load()
            .is_err()
    );
    std::fs::create_dir(temp.path().join("cockpit.sqlite3")).unwrap();
    assert!(Store::new(temp.path().to_owned()).load().is_err());
    let file_parent = temp.path().join("not-a-directory");
    std::fs::write(&file_parent, "").unwrap();
    assert!(Store::new(file_parent.join("profiles-v2")).load().is_err());
}

#[cfg(unix)]
#[test]
fn symlinked_library_or_parent_is_never_followed() {
    use std::os::unix::fs::symlink;
    let temp = crate::test_directory();
    let real = temp.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let link = temp.path().join("link");
    symlink(&real, &link).unwrap();
    assert!(Store::new(link).save(&Library::default()).is_err());
    let target = temp.path().join("target.json");
    std::fs::write(&target, Library::default().encode().unwrap()).unwrap();
    symlink(&target, real.join("library.json")).unwrap();
    let store = Store::new(real);
    assert!(store.load().unwrap().is_none());
    store.save(&Library::default()).unwrap();
    assert_eq!(
        std::fs::read(target).unwrap(),
        Library::default().encode().unwrap()
    );
}

#[cfg(unix)]
#[test]
fn canonical_temporary_fixture_works_but_the_alias_stays_rejected() {
    use std::os::unix::fs::symlink;
    let temp = crate::test_directory();
    let real_parent = temp.path().join("real-temporary-parent");
    std::fs::create_dir(&real_parent).unwrap();
    let alias = temp.path().join("aliased-temporary-parent");
    symlink(&real_parent, &alias).unwrap();

    // Models an OS temp directory such as macOS /var/folders. Canonicalizing is
    // a fixture choice, not a change to Store's no-symlink contract.
    let fixture = crate::test_directory_in(&alias);
    assert!(fixture.path().starts_with(&real_parent));
    let root = fixture.path().join("profiles-v2");
    let store = Store::new(root);
    store.save(&Library::default()).unwrap();
    assert_eq!(store.load().unwrap().unwrap().profiles.len(), 1);

    let alias_root = alias
        .join(fixture.path().file_name().unwrap())
        .join("profiles-v2");
    assert!(Store::new(alias_root.clone()).load().is_err());
    assert!(Store::new(alias_root).save(&Library::default()).is_err());
}
