use super::*;
fn database() -> (tempfile::TempDir, Database) {
    let directory = crate::test_directory();
    let database = Database::open(directory.path().join("cockpit.sqlite3")).unwrap();
    (directory, database)
}
#[test]
fn fresh_examples_are_persisted_without_runtime_or_security_authority() {
    let (_directory, database) = database();
    seed(&database).unwrap();
    let library: Library = serde_json::from_slice(
        &database
            .read("profiles", "library")
            .unwrap()
            .unwrap()
            .payload,
    )
    .unwrap();
    assert_eq!(library.profiles.len(), 4);
    assert!(library.local_assignments.is_empty());
    assert!(library.profiles.iter().filter(|p| p.id != DCS_ID).all(|p| p.virtual_outputs.is_empty()
        && p.desktop_outputs.is_empty()
        && p.visibility.is_empty()
        && p.overrides.is_empty()
        && p.companion_assignments.is_empty()));
    assert_eq!(
        library
            .profiles
            .iter()
            .find(|p| p.id == SIGNALS_ID)
            .unwrap()
            .bindings
            .len(),
        2
    );
    let dcs = library.profiles.iter().find(|p| p.id == DCS_ID).unwrap();
    assert_eq!(dcs.name, "DCS Feature Lab - AH-6J + F/A-18C");
    assert_eq!(dcs.revision, 1);
    assert_eq!(dcs.resources.len(), 7);
    assert_eq!(dcs.processing_nodes.len(), 44);
    assert_eq!(dcs.bindings.len(), 87);
    assert_eq!(dcs.vehicle_tabs.len(), 2);
    assert_eq!(dcs.virtual_outputs.len(), 2);
    assert_eq!(dcs.desktop_outputs.len(), 1);
    assert_eq!(dcs.visibility.len(), 1);
    assert_eq!(dcs.companion_assignments.len(), 1);
    assert_eq!(database.profile_index().unwrap().len(), 4);
    assert!(database.read("widgets", "library").unwrap().is_some());
    for (domain, key) in [
        ("secrets", "companion-host"),
        ("security", "mcp"),
        ("profiles", "selection"),
        ("recovery-profile", "profile-session-v1.json"),
    ] {
        assert!(database.read(domain, key).unwrap().is_none());
    }
}
#[test]
fn restarting_never_overwrites_edits_or_recreates_removed_examples() {
    let (directory, database) = database();
    seed(&database).unwrap();
    let mut library: Library = serde_json::from_slice(
        &database
            .read("profiles", "library")
            .unwrap()
            .unwrap()
            .payload,
    )
    .unwrap();
    library.profiles.retain(|p| p.id != WORKSPACE_ID);
    library
        .profiles
        .iter_mut()
        .find(|p| p.id == SIGNALS_ID)
        .unwrap()
        .name = "My edited example".into();
    let bytes = library.encode().unwrap();
    database
        .commit(&[Change {
            domain: "profiles",
            key: "library",
            expected_revision: 1,
            payload: Some(&bytes),
        }])
        .unwrap();
    let reopened = Database::open(directory.path().join("cockpit.sqlite3")).unwrap();
    seed(&reopened).unwrap();
    assert_eq!(
        reopened
            .read("profiles", "library")
            .unwrap()
            .unwrap()
            .payload,
        bytes
    );
    assert_eq!(reopened.revision("profiles", "library").unwrap(), 2);
}
#[test]
fn failed_seed_is_atomic_and_does_not_import_an_existing_library() {
    let (_directory, database) = database();
    let held = osg_persistence::test_support::hold_writer(&database).unwrap();
    assert!(seed(&database).unwrap_err().contains("busy"));
    drop(held);
    for (domain, key) in [
        ("profiles", "library"),
        ("widgets", "library"),
        ("development", "initialized"),
    ] {
        assert_eq!(database.revision(domain, key).unwrap(), 0);
    }
    database
        .commit(&[Change {
            domain: "profiles",
            key: "library",
            expected_revision: 0,
            payload: Some(&Library::default().encode().unwrap()),
        }])
        .unwrap();
    assert!(seed(&database).unwrap_err().contains("fresh SQLite"));
    assert!(database
        .read("development", "initialized")
        .unwrap()
        .is_none());
    assert!(database.read("widgets", "library").unwrap().is_none());
}

#[test]
fn invalid_initialization_evidence_cannot_trigger_default_overwrites(){
    let (_directory,database)=database();
    database.commit(&[Change{domain:"development",key:"initialized",expected_revision:0,payload:Some(b"invalid")}]).unwrap();
    assert!(seed(&database).unwrap_err().contains("initialization marker"));
    assert!(database.read("profiles","library").unwrap().is_none());
    assert!(database.read("widgets","library").unwrap().is_none());
}
