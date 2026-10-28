use super::*;
use crate::Change;

fn store() -> (tempfile::TempDir, Evidence) {
    let directory = crate::test_directory();
    let mut evidence = Evidence::open_live(&directory.path().join("run.json"), "live", 32).unwrap();
    evidence.write(b"intent").unwrap();
    (directory, evidence)
}

#[test]
fn repeated_verification_reuses_validated_connection_and_observes_fresh_commits() {
    let (_directory, evidence) = store();
    let opens = crate::connection::OPEN_ATTEMPTS.with(|attempts| attempts.get());
    for _ in 0..100 {
        evidence.verify().unwrap();
    }
    assert_eq!(
        crate::connection::OPEN_ATTEMPTS.with(|attempts| attempts.get()),
        opens
    );
    // Identical bytes still consume a revision, so an equal-byte rewrite must
    // invalidate the old authority even though the cached statement is reused.
    evidence
        .database
        .commit(&[Change {
            domain: "live",
            key: &evidence.key,
            expected_revision: 1,
            payload: Some(b"intent"),
        }])
        .unwrap();
    assert!(evidence
        .verify()
        .unwrap_err()
        .contains("changed externally"));
}

#[test]
fn payload_edits_and_tombstones_are_rejected_without_holding_a_snapshot() {
    let (directory, mut evidence) = store();
    let writer = evidence.database.connection().unwrap();
    writer
        .execute(
            "UPDATE records SET payload=?1 WHERE domain='live'",
            [b"edited".as_slice()],
        )
        .unwrap();
    assert!(evidence.verify().is_err());
    assert!(evidence.write(b"overwrite").is_err());
    evidence = Evidence::open_live(&directory.path().join("run.json"), "live", 32).unwrap();
    evidence
        .database
        .commit(&[Change {
            domain: "live",
            key: &evidence.key,
            expected_revision: 1,
            payload: None,
        }])
        .unwrap();
    assert!(evidence.verify().is_err());
    let tombstone = Evidence::open_live(&directory.path().join("run.json"), "live", 32).unwrap();
    assert!(tombstone.payload().is_none());
    tombstone.verify().unwrap();
}

#[test]
fn malformed_null_revision_cannot_impersonate_absent_evidence() {
    let directory = crate::test_directory();
    let evidence = Evidence::open_live(&directory.path().join("run.json"), "live", 32).unwrap();
    let writer = evidence.database.connection().unwrap();
    writer.execute_batch("ALTER TABLE records RENAME TO retired_records;
        CREATE TABLE records(domain TEXT,key TEXT,revision INTEGER,payload BLOB);").unwrap();
    writer.execute("INSERT INTO records VALUES('live',?1,NULL,NULL)", [&evidence.key]).unwrap();
    assert!(evidence.verify().is_err());
}

#[test]
fn writer_reservation_allows_reads_and_exclusive_lock_uses_short_busy_budget() {
    let (_directory, evidence) = store();
    let writer = evidence.database.connection().unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    evidence.verify().unwrap();
    writer.execute_batch("ROLLBACK; BEGIN EXCLUSIVE").unwrap();
    // Verify the actual SQLite policy, avoiding a wall-clock timing assertion
    // whose result would depend on whether the CI worker was descheduled.
    let reader = evidence.live.as_ref().unwrap().lock().unwrap();
    assert_eq!(
        reader
            .connection
            .pragma_query_value(None, "busy_timeout", |r| r.get::<_, i64>(0))
            .unwrap(),
        5
    );
    drop(reader);
    assert!(evidence.verify().unwrap_err().contains("busy"));
    writer.execute_batch("ROLLBACK").unwrap();
    evidence.verify().unwrap();
}

#[test]
fn epoch_and_sidecar_indirection_revoke_live_authority() {
    let (_directory, evidence) = store();
    let writer = evidence.database.connection().unwrap();
    writer
        .execute(
            "UPDATE incarnation SET epoch='00000000000000000000000000000000'",
            [],
        )
        .unwrap();
    assert!(evidence
        .verify()
        .unwrap_err()
        .contains("restored or replaced"));
    let (_directory, evidence) = store();
    let mut journal = evidence.database.path().as_os_str().to_os_string();
    journal.push("-journal");
    std::fs::create_dir(&journal).unwrap();
    assert!(evidence.verify().is_err());
}

#[test]
fn retained_connection_fences_database_replacement_even_with_identical_epoch_and_bytes() {
    let (directory, evidence) = store();
    let replacement = directory.path().join("replacement.sqlite3");
    evidence.database.backup(&replacement).unwrap();
    let path = evidence.database.path();
    let retired = directory.path().join("retired.sqlite3");
    #[cfg(windows)]
    {
        // The bundled Windows VFS denies FILE_SHARE_DELETE for the lifetime of
        // the open connection; replacement cannot get past the OS handle.
        assert!(std::fs::rename(path, &retired).is_err());
        assert!(std::fs::remove_file(path).is_err());
        evidence.verify().unwrap();
        drop(evidence);
        std::fs::rename(directory.path().join("cockpit.sqlite3"), retired).unwrap();
    }
    #[cfg(unix)]
    {
        std::fs::rename(path, &retired).unwrap();
        assert!(evidence.verify().is_err());
        std::fs::rename(replacement, path).unwrap();
        assert!(evidence
            .verify()
            .unwrap_err()
            .contains("removed or replaced"));
    }
}

#[test]
fn database_parent_replacement_cannot_leave_old_connection_authoritative() {
    let directory = crate::test_directory();
    let parent = directory.path().join("owned");
    let retired = directory.path().join("retired");
    std::fs::create_dir(&parent).unwrap();
    // Control: this parent can be renamed before any live SQLite handle exists.
    std::fs::rename(&parent, &retired).unwrap();
    std::fs::rename(&retired, &parent).unwrap();
    let mut evidence = Evidence::open_live(&parent.join("run.json"), "live", 32).unwrap();
    evidence.write(b"intent").unwrap();
    let backup = directory.path().join("replacement.sqlite3");
    evidence.database.backup(&backup).unwrap();
    #[cfg(windows)]
    {
        assert!(std::fs::rename(&parent, &retired).is_err());
        evidence.verify().unwrap();
        drop(evidence);
        std::fs::rename(&parent, &retired).unwrap();
    }
    #[cfg(unix)]
    {
        std::fs::rename(&parent, &retired).unwrap();
        assert!(evidence.verify().is_err());
        std::fs::create_dir(&parent).unwrap();
        std::fs::copy(backup, parent.join("cockpit.sqlite3")).unwrap();
        assert!(evidence.verify().is_err());
    }
}

#[test]
#[ignore = "manual latency comparison; reports measurements without a scheduler-sensitive threshold"]
fn live_verification_latency() {
    let directory = crate::test_directory();
    let anchor = directory
        .path()
        .join("native/config/output/session/run.json");
    let mut regular = Evidence::open(&anchor, "benchmark", 8192).unwrap();
    regular.write(&vec![b'x'; 8192]).unwrap();
    let live = Evidence::open_live(&anchor, "benchmark", 8192).unwrap();
    let mut regular_us = Vec::new();
    let mut live_us = Vec::new();
    for _ in 0..300 {
        let started = std::time::Instant::now();
        regular.verify().unwrap();
        regular_us.push(started.elapsed().as_micros());
        let started = std::time::Instant::now();
        live.verify().unwrap();
        live_us.push(started.elapsed().as_micros());
    }
    regular_us.sort_unstable();
    live_us.sort_unstable();
    println!(
        "verification microseconds p50/p95/max: fresh={}/{}/{} live={}/{}/{}",
        regular_us[150], regular_us[285], regular_us[299], live_us[150], live_us[285], live_us[299]
    );
}
