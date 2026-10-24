use super::*;
use osg_mapping::engine::Counter;
fn token() -> RunToken {
    RunToken {
        session: uuid::Uuid::new_v4().to_string(),
        generation: Counter(1),
    }
}
fn binding() -> Binding {
    crate::vjoy::tests::binding()
}
#[test]
fn read_only_verification_checks_locked_current_bytes_without_rewriting_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("output.json");
    let mut store = Store::open(path.clone()).unwrap();
    store.verify_current().unwrap();
    store.begin(token(), vec![binding()]).unwrap();
    store.transition(Phase::Confirmed).unwrap();
    let original = sql_read(&path).unwrap();
    store.verify_current().unwrap();
    assert_eq!(sql_read(&path).unwrap(), original);
    assert!(Store::open(path.clone()).is_err());
    sql_write(&path, b"external").unwrap();
    assert!(store.verify_current().unwrap_err().contains("externally"));
    assert_eq!(sql_read(&path).unwrap(), b"external");
    assert_eq!(store.record().unwrap().phase, Phase::Confirmed);
    sql_delete(&path).unwrap();
    assert!(store.verify_current().is_err());
    std::fs::create_dir(&path).unwrap();
    assert!(store.verify_current().is_err());
    std::fs::remove_dir(&path).unwrap();
    sql_write(&path, &original).unwrap();
    assert!(store.verify_current().is_err()); // Revision cannot be rewound by restoring equal bytes.
    assert_eq!(sql_read(&path).unwrap(), original);
}

#[test]
fn journal_orders_ownership_and_refuses_competing_sessions_or_stale_acknowledgements() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("output.json");
    assert!(inspect(&path).unwrap().is_none());
    let mut store = Store::open(path.clone()).unwrap();
    assert!(Store::open(path.clone()).is_err());
    assert!(store.transition(Phase::Clean).is_err());
    assert!(store.acknowledge(&token()).is_err());
    let run = token();
    store.begin(run.clone(), vec![binding()]).unwrap();
    assert_eq!(inspect(&path).unwrap().unwrap().phase, Phase::Intent);
    assert!(store.begin(token(), vec![binding()]).is_err());
    assert!(store.acknowledge(&token()).is_err());
    store.transition(Phase::Confirmed).unwrap();
    assert!(store.acknowledge(&run).is_err());
    assert!(store.transition(Phase::Intent).is_err());
    store.transition(Phase::Releasing).unwrap();
    drop(store);
    let mut store = Store::open(path.clone()).unwrap();
    store.acknowledge(&run).unwrap();
    assert!(!inspect(&path).unwrap().unwrap().pending());
    store.begin(token(), vec![binding()]).unwrap();
    store.transition(Phase::Clean).unwrap();
    drop(store);
    Store::open(path).unwrap(); // OS lock was released.
}
#[test]
fn malformed_bounded_and_externally_changed_records_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("output.json");
    let mut store = Store::open(path.clone()).unwrap();
    assert!(store.begin(token(), vec![]).is_err());
    assert!(store.begin(token(), vec![binding(), binding()]).is_err());
    let mut bad = binding();
    bad.slot = 0;
    assert!(store.begin(token(), vec![bad]).is_err());
    let mut run = token();
    run.generation = Counter(0);
    assert!(store.begin(run, vec![binding()]).is_err());
    store.begin(token(), vec![binding()]).unwrap();
    let original = sql_read(&path).unwrap();
    sql_write(&path, b"external").unwrap();
    assert!(store
        .transition(Phase::Confirmed)
        .unwrap_err()
        .contains("externally"));
    assert_eq!(sql_read(&path).unwrap(), b"external");
    drop(store);
    assert!(inspect(&path).is_err());
    assert!(Store::open(path.clone()).is_err());
    sql_write(&path, vec![b'x'; LIMIT + 1]).unwrap();
    assert!(inspect(&path).unwrap_err().contains("bound"));
    let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
    value["schema"] = 2.into();
    sql_write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(inspect(&path).is_err());
    sql_delete(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(inspect(&path).is_err());
    let lock = dir.path().join("output.lock");
    std::fs::remove_file(&lock).unwrap();
    std::fs::create_dir(&lock).unwrap();
    assert!(Store::open(path).is_err());
}
#[test]
fn process_exit_preserves_confirmed_intent_and_releases_os_lock() {
    const ENV: &str = "OSG_OUTPUT_JOURNAL_EXIT_FIXTURE";
    if let Some(path) = std::env::var_os(ENV) {
        let mut store = Store::open(PathBuf::from(path)).unwrap();
        store.begin(token(), vec![binding()]).unwrap();
        store.transition(Phase::Confirmed).unwrap();
        std::process::exit(0); // Deliberately bypass Drop, like a terminated app.
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("output.json");
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "output_journal::tests::process_exit_preserves_confirmed_intent_and_releases_os_lock",
        ])
        .env(ENV, &path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    assert!(command.status().unwrap().success());
    let mut store = Store::open(path).unwrap();
    assert_eq!(store.record().unwrap().phase, Phase::Confirmed);
    assert!(store.begin(token(), vec![binding()]).is_err());
    store.transition(Phase::Releasing).unwrap();
    store.transition(Phase::Clean).unwrap();
}
#[cfg(unix)]
#[test]
fn recovery_never_follows_symlink_records_or_lock_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("output.json");
    let target = dir.path().join("external");
    std::fs::write(&target, b"external").unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    assert!(inspect(&path).is_err());
    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&target, path.with_extension("lock")).unwrap();
    assert!(Store::open(path).is_err());
    assert_eq!(std::fs::read(target).unwrap(), b"external");
}

#[test]
fn releasing_owner_unlocks_even_while_a_duplicated_descriptor_is_alive() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("output.json");
    let store = Store::open(path.clone()).unwrap();
    let inherited = store._lock.try_clone().unwrap();
    assert!(Store::open(path.clone()).is_err());
    drop(store);
    let next = Store::open(path.clone()).unwrap();
    drop(inherited);
    assert!(Store::open(path.clone()).is_err());
    drop(next);
    Store::open(path).unwrap();
}

fn sql_read(path:&std::path::Path)->Result<Vec<u8>,String>{osg_persistence::test_support::read_evidence(path,"recovery-output")}
fn sql_write(path:&std::path::Path,data:impl AsRef<[u8]>)->Result<(),String>{osg_persistence::test_support::write_evidence(path,"recovery-output",Some(data.as_ref()))}
fn sql_delete(path:&std::path::Path)->Result<(),String>{osg_persistence::test_support::write_evidence(path,"recovery-output",None)}
