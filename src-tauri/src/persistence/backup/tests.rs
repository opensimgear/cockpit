use super::*;
fn fixture() -> (tempfile::TempDir, Runtime) {
    let directory = crate::test_directory();
    let database = Database::open(directory.path().join("cockpit.sqlite3")).unwrap();
    let runtime = Runtime {
        database: Ok(database),
        root: directory.path().to_owned(),
        controls: Mutex::new(()),
    };
    (directory, runtime)
}
fn set(runtime: &Runtime, domain: &str, key: &str, value: &[u8]) {
    let db = runtime.database().unwrap();
    db.commit(&[Change {
        domain,
        key,
        expected_revision: db.revision(domain, key).unwrap(),
        payload: Some(value),
    }])
    .unwrap();
}
#[test]
fn online_backup_restore_reopens_content_with_rollback_and_no_historical_authority() {
    let (_directory, runtime) = fixture();
    set(&runtime, "ui", "theme", b"true");
    set(
        &runtime,
        "secrets",
        "companion-host",
        b"native sealed ciphertext",
    );
    let first = runtime.create(0).unwrap();
    let id = first["backup"]["id"].as_str().unwrap();
    assert!(runtime.create(0).is_err());
    runtime.verify(id, 1).unwrap();
    set(&runtime, "ui", "theme", b"false");
    set(
        &runtime,
        "security",
        "mcp",
        b"current hash-only credentials",
    );
    let queued = runtime.queue(id, 1).unwrap();
    assert_eq!(queued["restartRequired"], true);
    assert!(runtime.create(1).is_err());
    assert!(runtime.remove(id, 1).is_err());
    assert!(runtime.queue(id, 1).is_err());
    let lease = NamespaceLease::acquire(runtime.database().unwrap().path()).unwrap();
    let database = runtime.finish_pending(&lease).unwrap().unwrap();
    assert!(runtime.database().unwrap().read("ui", "theme").is_err());
    assert_eq!(
        database.read("ui", "theme").unwrap().unwrap().payload,
        b"true"
    );
    assert!(database
        .read("secrets", "companion-host")
        .unwrap()
        .is_none());
    assert!(database.read("security", "mcp").unwrap().is_none());
    assert!(database
        .read("storage", "pending-restore")
        .unwrap()
        .is_none());
    let next = Runtime {
        database: Ok(database),
        root: runtime.root.clone(),
        controls: Mutex::new(()),
    };
    let status = next.status().unwrap();
    assert_eq!(status["backups"].as_array().unwrap().len(), 2);
    assert_eq!(status["restartRequired"], false);
    assert!(next.finish_pending(&lease).unwrap().is_none());
    assert!(next.remove(id, 1).is_err());
    assert_eq!(next.remove(id, 2).unwrap()["filesRemoved"], true);
    assert!(next.directory(id).unwrap().exists() == false);
}
#[test]
fn manifest_asset_tamper_and_invalid_metadata_preserve_current_records() {
    let (_directory, runtime) = fixture();
    let bytes = b"original DCS export";
    let hash = assets::digest(bytes);
    let operation = uuid::Uuid::new_v4().to_string();
    let relative = format!("games.integration-setup.setup-work/{operation}/Export.lua.before");
    assets::write_new(&runtime.root.join(&relative), bytes).unwrap();
    set(
        &runtime,
        "recovery-dcs",
        "games.integration-setup.setup-work/dcs-setup-journal.json",
        &serde_json::to_vec(&json!({"pending":{"id":operation,"exportBefore":hash},"receipts":[]}))
            .unwrap(),
    );
    let created = runtime.create(0).unwrap();
    let id = created["backup"]["id"].as_str().unwrap();
    let directory = runtime.directory(id).unwrap();
    let (_, manifest) = runtime.verified(id, 1).unwrap();
    assert_eq!(manifest.assets.len(), 1);
    assert_eq!(manifest.assets[0].path, relative);
    assert!(runtime.directory("../escape").is_err());
    let asset = directory.join("assets").join(&hash);
    fs::write(&asset, b"changed").unwrap();
    assert!(runtime.verify(id, 1).unwrap_err().contains("hash check"));
    fs::write(&asset, bytes).unwrap();
    runtime.verify(id, 1).unwrap();
    let path = directory.join("manifest.json");
    let original = fs::read(&path).unwrap();
    fs::write(&path, b"{}").unwrap();
    assert!(runtime
        .queue(id, 1)
        .unwrap_err()
        .contains("manifest changed"));
    fs::write(&path, original).unwrap();
    fs::write(directory.join("snapshot.sqlite3"), b"corrupt snapshot").unwrap();
    assert!(runtime.queue(id, 1).is_err());
    assert!(runtime.pending().unwrap().is_none());
    assert_eq!(runtime.status().unwrap()["revision"], 1);
    set(&runtime, "storage", "backups", b"{}");
    assert!(runtime.status().is_err());
    assert!(Runtime::new(false).status().is_err());
}
#[test]
fn busy_index_commit_and_full_inventory_do_not_publish_success() {
    let (_directory, runtime) = fixture();
    let db = runtime.database().unwrap();
    let blocker = osg_persistence::test_support::hold_writer(db).unwrap();
    assert!(runtime.create(0).is_err());
    drop(blocker);
    assert_eq!(runtime.status().unwrap()["revision"], 0);
    assert!(runtime.index().unwrap().1.entries.is_empty());
    let entries = (0..MAX_BACKUPS)
        .map(|_| Entry {
            id: uuid::Uuid::new_v4().to_string(),
            created_at: 1,
            manifest_hash: "0".repeat(64),
        })
        .collect();
    runtime.save(0, &Index { entries }).unwrap();
    assert!(runtime.create(1).is_err());
    assert!(runtime.remove("../bad", 1).is_err());
    assert_eq!(runtime.status().unwrap()["revision"], 1);
}
#[test]
fn manifest_validator_and_current_artwork_restore_are_bounded_and_hash_checked() {
    let (_directory, runtime) = fixture();
    let bytes = b"immutable jpeg fixture";
    let hash = assets::digest(bytes);
    let id = uuid::Uuid::new_v4().to_string();
    let directory = runtime.directory(&id).unwrap();
    assets::write_new(&directory.join("assets").join(&hash), bytes).unwrap();
    let mut manifest = Manifest {
        id,
        created_at: 1,
        database_hash: "0".repeat(64),
        assets: vec![Asset {
            path: format!("game-artwork/{hash}.jpg"),
            sha256: hash.clone(),
            size: bytes.len() as u64,
        }],
    };
    manifest.validate().unwrap();
    assets::verify(&directory, &manifest).unwrap();
    assets::restore(&runtime.root, &directory, &manifest).unwrap();
    assets::restore(&runtime.root, &directory, &manifest).unwrap();
    fs::write(
        runtime.root.join(&manifest.assets[0].path),
        b"external edit",
    )
    .unwrap();
    assert!(assets::restore(&runtime.root, &directory, &manifest).is_err());
    for path in [
        "../outside",
        "game-artwork/fake.jpg",
        "games.integration-setup.setup-work/../../Export.lua.before",
    ] {
        manifest.assets[0].path = path.into();
        assert!(manifest.validate().is_err());
    }
    assert!(assets::read(&directory.join("assets").join(&hash), 1).is_err());
    assert!(assets::write_new(&directory.join("assets").join(&hash), bytes).is_err());
    assert!(!model::hash("wrong"));
    assert!(!model::id("BAD"));
}

#[test]
fn queued_restore_detects_new_recovery_evidence_before_any_replacement() {
    let (_directory, runtime) = fixture();
    set(&runtime, "ui", "theme", b"true");
    let created = runtime.create(0).unwrap();
    let id = created["backup"]["id"].as_str().unwrap();
    runtime.queue(id, 1).unwrap();
    set(
        &runtime,
        "recovery-output",
        "new-owner",
        b"new current evidence",
    );
    let lease = NamespaceLease::acquire(runtime.database().unwrap().path()).unwrap();
    assert!(runtime
        .finish_pending(&lease)
        .unwrap_err()
        .contains("changed after quiescence"));
    assert_eq!(
        runtime
            .database()
            .unwrap()
            .read("ui", "theme")
            .unwrap()
            .unwrap()
            .payload,
        b"true"
    );
    assert_eq!(runtime.index().unwrap().1.entries.len(), 1);
    assert!(runtime.pending().unwrap().is_some());
}
#[test]
fn backup_captures_only_referenced_artwork_and_preserves_offline_hashes() {
    let (_directory, runtime) = fixture();
    let bytes = b"bounded managed image";
    let hash = assets::digest(bytes);
    let mut settings = crate::game_installations::Settings::default();
    settings.backgrounds.insert(
        "dcs".into(),
        crate::game_artwork::Background {
            image: format!("artwork:{hash}.jpg"),
            position_x: 50.,
            position_y: 50.,
            crop: None,
        },
    );
    set(
        &runtime,
        "games",
        "installations",
        &serde_json::to_vec(&settings).unwrap(),
    );
    assert!(runtime.create(0).is_err());
    assert_eq!(runtime.status().unwrap()["revision"], 0);
    assets::write_new(
        &runtime.root.join(format!("game-artwork/{hash}.jpg")),
        bytes,
    )
    .unwrap();
    let created = runtime.create(0).unwrap();
    let id = created["backup"]["id"].as_str().unwrap();
    let (_, manifest) = runtime.verified(id, 1).unwrap();
    assert_eq!(manifest.assets.len(), 1);
    assert_eq!(manifest.assets[0].sha256, hash);
    assert!(runtime.remove(id, 0).is_err());
    assert_eq!(runtime.remove(id, 1).unwrap()["filesRemoved"], true);
}

#[test]
#[cfg(windows)]
fn windows_restore_retains_saved_panels_with_new_dpapi_host_and_no_old_pairings() {
    use osg_companion_service::persistence::{HostSnapshotStore, SealedSqliteStore};
    let (_directory, runtime) = fixture();
    let upload: Value = serde_json::from_str(include_str!(
        "../../../../crates/companion-service/tests/fixtures/compiled-toggle-publication.json"
    ))
    .unwrap();
    let hash = upload["publicationHash"].as_str().unwrap();
    let document: Value =
        serde_json::from_str(upload["canonicalDocument"].as_str().unwrap()).unwrap();
    let panel = document["ui"]["id"].as_str().unwrap();
    let archive = json!({"publications":[upload],"savedHeads":{panel:hash}});
    let storage =
        SealedSqliteStore::new(runtime.root.join("companions/companion-host-state.dpapi"));
    let old = json!({"hostId":"old-restored-host","credentials":[{"companionId":"old-paired-browser","name":"Previous browser","tokenHash":"a".repeat(64),"kind":"browser"}],"instances":[]});
    storage
        .write_host_snapshot(
            &serde_json::to_vec(&old).unwrap(),
            &serde_json::to_vec(&archive).unwrap(),
        )
        .unwrap();
    let before = osg_companion_service::Hub::open_existing(
        runtime.root.join("companions"),
        std::sync::Arc::new(osg_companion_service::NullSourceSink),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        before
            .command(&json!({"command":"companion_status"}))
            .unwrap()["clients"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    drop(before);
    let created = runtime.create(0).unwrap();
    let id = created["backup"]["id"].as_str().unwrap();
    runtime.queue(id, 1).unwrap();
    let lease = NamespaceLease::acquire(runtime.database().unwrap().path()).unwrap();
    let restored = runtime.finish_pending(&lease).unwrap().unwrap();
    let host = osg_companion_service::Hub::open_existing(
        runtime.root.join("companions"),
        std::sync::Arc::new(osg_companion_service::NullSourceSink),
    )
    .unwrap()
    .unwrap();
    let status = host
        .command(&json!({"command":"companion_status"}))
        .unwrap();
    assert_ne!(status["hostId"], old["hostId"]);
    assert_eq!(status["clients"], json!([]));
    assert_eq!(status["instances"], json!([]));
    assert_eq!(
        host.saved_heads().get(panel).map(String::as_str),
        Some(hash)
    );
    assert_eq!(host.saved_panels().len(), 1);
    assert!(restored
        .read("secrets", "companion-identity")
        .unwrap()
        .is_none());
    assert_eq!(
        serde_json::from_slice::<Value>(
            &restored
                .read("companion", "publications")
                .unwrap()
                .unwrap()
                .payload
        )
        .unwrap(),
        archive
    );
}
