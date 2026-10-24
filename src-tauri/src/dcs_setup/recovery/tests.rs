use super::*;
fn record(target: &Path) -> Value {
    json!({"id":uuid::Uuid::new_v4().to_string(),"target":target,"createdScripts":false,"installModule":false,"files":{},"exportBefore":null,"exportAfter":hash(b"after"),"version":VERSION,"archiveSha256":ARCHIVE_SHA256,"aircraftBefore":null,"aircraftAfter":null,"createdAircraftDir":false,"physicsBefore":null,"physicsAfter":hash(b"physics"),"createdPhysicsDir":false})
}
fn raw(work: &Path) -> Vec<u8> {
    osg_persistence::test_support::read_evidence(&work.join(JOURNAL), DOMAIN).unwrap()
}
fn foreign(work: &Path, value: &Value) {
    osg_persistence::test_support::write_evidence(
        &work.join(JOURNAL),
        DOMAIN,
        Some(&serde_json::to_vec(value).unwrap()),
    )
    .unwrap();
}
#[test]
fn old_files_are_ignored_and_current_receipts_commit_without_json_files() {
    let directory = crate::test_directory();
    let target = directory.path().join("DCS");
    fs::create_dir(&target).unwrap();
    let work = directory.path().join("work");
    fs::create_dir(&work).unwrap();
    let retired = work.join(JOURNAL);
    fs::write(&retired, b"discarded old journal").unwrap();
    assert!(!pending(&work));
    assert!(completed(&target, &work).unwrap().is_empty());
    assert!(!work.join("cockpit.sqlite3").exists());
    let first = record(&target);
    let mut ledger = Ledger::open(&work).unwrap();
    assert!(Ledger::open(&work).is_err());
    ledger.begin(first.clone()).unwrap();
    assert!(pending(&work));
    assert!(ledger.begin(record(&target)).is_err());
    ledger.finish(State::Completed).unwrap();
    assert!(!pending(&work));
    assert_eq!(completed(&target, &work).unwrap()[0].2, first);
    assert_eq!(fs::read(&retired).unwrap(), b"discarded old journal");
    assert!(!work
        .join(first["id"].as_str().unwrap())
        .join("completed.json")
        .exists());
    ledger.begin_removal(&first).unwrap();
    assert!(pending(&work));
    assert!(completed(&target, &work).unwrap().is_empty());
    ledger.finish(State::Restored).unwrap();
    assert!(!pending(&work));
    assert!(completed(&target, &work).unwrap().is_empty());
    assert!(ledger.finish(State::Restored).is_err());
    drop(ledger);
    Ledger::open(&work).unwrap();
}
#[test]
fn related_receipts_have_durable_order_without_filesystem_timestamps() {
    let directory = crate::test_directory();
    let target = directory.path().join("DCS");
    fs::create_dir(&target).unwrap();
    let work = directory.path().join("work");
    let mut ledger = Ledger::open(&work).unwrap();
    let first = record(&target);
    let second = record(&target);
    for value in [&first, &second] {
        ledger.begin(value.clone()).unwrap();
        ledger.finish(State::Completed).unwrap();
    }
    assert_eq!(
        ledger
            .completed(&target, &work)
            .unwrap()
            .iter()
            .map(|r| r.2["id"].clone())
            .collect::<Vec<_>>(),
        [second["id"].clone(), first["id"].clone()]
    );
    let mut stale = second.clone();
    stale["createdScripts"] = json!(true);
    assert!(ledger.begin_removal(&stale).is_err());
    ledger.begin_removal(&second).unwrap();
    assert!(ledger.begin_removal(&first).is_err());
    ledger.finish(State::Restored).unwrap();
    assert_eq!(ledger.completed(&target, &work).unwrap()[0].2, first);
}
#[test]
fn busy_and_external_edits_never_acknowledge_or_replace_evidence() {
    let directory = crate::test_directory();
    let work = directory.path().join("work");
    let value = record(directory.path());
    let mut ledger = Ledger::open(&work).unwrap();
    let database = osg_persistence::Database::for_namespace(&work.join(JOURNAL)).unwrap();
    let blocker = osg_persistence::test_support::hold_writer(&database).unwrap();
    assert!(ledger.begin(value.clone()).unwrap_err().contains("busy"));
    assert!(ledger.pending().is_none());
    drop(blocker);
    ledger.begin(value.clone()).unwrap();
    let original = raw(&work);
    let changed = json!({"pending":null,"receipts":[]});
    foreign(&work, &changed);
    assert!(ledger.verify_pending(&value).is_err());
    assert!(ledger.finish(State::Completed).is_err());
    assert_eq!(
        serde_json::from_slice::<Value>(&raw(&work)).unwrap(),
        changed
    );
    osg_persistence::test_support::write_evidence(&work.join(JOURNAL), DOMAIN, Some(&original))
        .unwrap();
    assert!(ledger.verify().is_err());
}
#[test]
fn malformed_or_unbounded_sql_evidence_blocks_without_defaults_or_import() {
    let directory = crate::test_directory();
    let work = directory.path().join("work");
    for value in [
        json!({}),
        json!({"pending":null}),
        json!({"pending":null,"receipts":[],"schemaVersion":1}),
    ] {
        foreign(&work, &value);
        assert!(pending(&work));
        assert!(Ledger::open(&work).is_err());
        assert_eq!(serde_json::from_slice::<Value>(&raw(&work)).unwrap(), value);
    }
    let value = record(directory.path());
    let duplicate = json!({"pending":value,"receipts":[{"state":"completed","record":value}]});
    foreign(&work, &duplicate);
    assert!(Ledger::open(&work).is_err());
    let oversized = json!({"pending":null,"receipts":vec![json!({"state":"restored","record":value});MAX_RECEIPTS+1]});
    foreign(&work, &oversized);
    assert!(Ledger::open(&work).is_err());
    for key in [
        "id",
        "createdScripts",
        "exportAfter",
        "version",
        "files",
        "target",
    ] {
        let mut bad = value.clone();
        bad.as_object_mut().unwrap().remove(key);
        assert!(record::validate(&bad).is_err());
    }
    for (key, change) in [
        ("id", json!("../bad")),
        ("target", json!("relative")),
        ("createdScripts", json!(1)),
        ("exportAfter", json!("wrong")),
        ("physicsAfter", Value::Null),
        ("version", json!("unapproved")),
        ("files", json!({"../external":"0".repeat(64)})),
    ] {
        let mut bad = value.clone();
        bad[key] = change;
        assert!(record::validate(&bad).is_err());
    }
}
#[test]
fn process_exit_preserves_only_pending_evidence_and_releases_lifetime_lock() {
    const VARIABLE: &str = "OSG_DCS_SQLITE_CRASH_FIXTURE";
    if let Some(work) = std::env::var_os(VARIABLE) {
        let work = PathBuf::from(work);
        let mut ledger = Ledger::open(&work).unwrap();
        ledger.begin(record(work.parent().unwrap())).unwrap();
        std::process::exit(0);
    }
    let directory = crate::test_directory();
    let work = directory.path().join("work");
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command.args(["--exact","dcs_setup::recovery::tests::process_exit_preserves_only_pending_evidence_and_releases_lifetime_lock"]).env(VARIABLE,&work).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    assert!(command.status().unwrap().success());
    assert!(pending(&work));
    let ledger = Ledger::open(&work).unwrap();
    assert!(ledger.pending().is_some());
    assert!(!directory.path().join("Scripts").exists());
    drop(ledger);
    assert_eq!(super::super::recover(&work).unwrap()["restored"], true);
    assert!(!pending(&work));
    assert!(!directory.path().join("Scripts").exists());
}
