use super::*;
use std::cell::Cell;

#[test]
fn ordinary_files_absence_and_unrelated_errors_never_wait() {
    let directory = crate::test_directory();
    let database = directory.path().join("cockpit.sqlite3");
    std::fs::write(&database, b"main").unwrap();
    let metadata = std::fs::symlink_metadata(&database).unwrap();
    check_with(&database, || Ok(metadata.clone()), |_| panic!("ordinary file waited")).unwrap();
    check_with(&database, || Err(io::ErrorKind::NotFound.into()), |_| panic!("absent file waited")).unwrap();
    for code in [32, 33, 123, 303] {
        let calls = Cell::new(0);
        assert!(check_with(&database, || { calls.set(calls.get()+1); Err(io::Error::from_raw_os_error(code)) }, |_| panic!("unconfirmed error waited")).is_err());
        assert_eq!(calls.get(), 1);
    }
    let metadata = std::fs::symlink_metadata(directory.path()).unwrap();
    assert!(check_with(&database, || Ok(metadata.clone()), |_| panic!("directory waited")).is_err());
}

#[cfg(windows)]
#[test]
fn access_denial_requires_a_fresh_valid_inspection_or_actual_absence() {
    let directory = crate::test_directory();
    let database = directory.path().join("cockpit.sqlite3");
    std::fs::write(&database, b"main").unwrap();
    let metadata = std::fs::symlink_metadata(&database).unwrap();
    for disappears in [false, true] {
        let calls = Cell::new(0);let mut waits = vec![];
        check_with(&database, || {
            calls.set(calls.get()+1);
            if calls.get()==1 { Err(io::Error::from_raw_os_error(5)) }
            else if disappears { Err(io::ErrorKind::NotFound.into()) }
            else { Ok(metadata.clone()) }
        }, |wait| waits.push(wait)).unwrap();
        assert_eq!(calls.get(), 2);assert_eq!(waits, [Duration::from_millis(1)]);
    }
}

#[cfg(windows)]
#[test]
fn persistent_denial_is_bounded_and_never_treated_as_absence() {
    let directory = crate::test_directory();
    let database = directory.path().join("cockpit.sqlite3");
    std::fs::write(&database, b"main").unwrap();
    let calls = Cell::new(0);let mut waits = vec![];
    let error = check_with(&database, || { calls.set(calls.get()+1); Err(io::Error::from_raw_os_error(5)) }, |wait| waits.push(wait)).unwrap_err();
    assert_eq!(error, "Cannot inspect persistence path.");
    assert_eq!(calls.get(), 4);
    assert_eq!(waits, [1,2,4].map(Duration::from_millis));
}

#[cfg(windows)]
#[test]
fn main_path_is_revalidated_after_wait_and_invalid_journal_types_still_fail() {
    let directory = crate::test_directory();
    let database = directory.path().join("cockpit.sqlite3");
    let journal = directory.path().join("cockpit.sqlite3-journal");
    std::fs::write(&database, b"main").unwrap();
    std::fs::write(&journal, b"journal").unwrap();
    let metadata = std::fs::symlink_metadata(&journal).unwrap();
    let calls = Cell::new(0);
    let result = check_with(&database, || {
        calls.set(calls.get()+1);
        if calls.get()==1 { Err(io::Error::from_raw_os_error(5)) } else { Ok(metadata.clone()) }
    }, |_| { std::fs::remove_file(&database).unwrap();std::fs::create_dir(&database).unwrap(); });
    assert!(result.unwrap_err().contains("wrong type"));
    assert_eq!(calls.get(), 1, "Changed main path must fail before another journal probe.");
    std::fs::remove_dir(&database).unwrap();std::fs::write(&database, b"main").unwrap();
    let calls = Cell::new(0);
    let invalid = std::fs::symlink_metadata(directory.path()).unwrap();
    assert!(check_with(&database, || {
        calls.set(calls.get()+1);
        if calls.get()==1 { Err(io::Error::from_raw_os_error(5)) } else { Ok(invalid.clone()) }
    }, |_| {}).unwrap_err().contains("wrong type"));
}

#[cfg(not(windows))]
#[test]
fn non_windows_access_codes_have_no_special_policy() {
    assert!(check_with(Path::new("unused"), || Err(io::Error::from_raw_os_error(5)), |_| panic!("non-Windows waited")).is_err());
}

#[cfg(windows)]
#[test]
fn reparse_journal_after_a_transient_error_is_rejected() {
    let directory = crate::test_directory();
    let database = directory.path().join("cockpit.sqlite3");
    let journal = directory.path().join("cockpit.sqlite3-journal");
    let target = directory.path().join("owned-target");
    std::fs::write(&database, b"main").unwrap();std::fs::create_dir(&target).unwrap();
    let output = std::process::Command::new("cmd.exe").args(["/D","/C","mklink","/J"]).arg(&journal).arg(&target).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let metadata = std::fs::symlink_metadata(&journal).unwrap();
    let calls = Cell::new(0);
    let error = check_with(&database, || {
        calls.set(calls.get()+1);
        if calls.get()==1 { Err(io::Error::from_raw_os_error(5)) } else { Ok(metadata.clone()) }
    }, |_| {}).unwrap_err();
    assert!(error.contains("reparse"));
    std::fs::remove_dir(&journal).unwrap();
}

#[test]
fn sidecars_remain_fixed_leaves_and_reject_indirect_or_wrong_types() {
    let directory = crate::test_directory();
    let database = directory.path().join("cockpit.sqlite3");
    std::fs::write(&database, b"main").unwrap();
    super::super::check_path(&database, false).unwrap();
    super::super::check_sidecars(&database).unwrap();
    for suffix in ["-journal", "-wal", "-shm"] {
        let path = directory.path().join(format!("cockpit.sqlite3{suffix}"));
        std::fs::create_dir(&path).unwrap();
        assert!(super::super::check_sidecars(&database).is_err());
        std::fs::remove_dir(&path).unwrap();
    }
}
