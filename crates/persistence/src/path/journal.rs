//! SQLite DELETE journals can disappear while Rust opens their metadata handle.
//! Recheck only that leaf; never retry a database command or infer absence from
//! access denial. The successful inspection still rejects every reparse point.
use std::{fs::Metadata, io, path::Path, time::Duration};

pub(super) fn check(database: &Path, journal: &Path) -> Result<(), String> {
    check_with(database, || std::fs::symlink_metadata(journal), std::thread::sleep)
}

fn check_with(database: &Path, mut inspect: impl FnMut() -> io::Result<Metadata>, mut wait: impl FnMut(Duration)) -> Result<(), String> {
    let mut result = inspect();
    for milliseconds in [1, 2, 4] {
        if !result.as_ref().err().is_some_and(transient) { break; }
        wait(Duration::from_millis(milliseconds));
        // Do not probe the journal again through a stale ancestor/main proof.
        // Their errors and indirection remain immediately fatal after any wait.
        super::check_path(database, false)?;
        result = inspect();
    }
    match result {
        Ok(metadata) => super::validate(&metadata, false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Cannot inspect persistence path.".into()),
    }
}

fn transient(error: &io::Error) -> bool {
    // Confirmed in the actual Cockpit on cockpit.sqlite3-journal. Rust can
    // return the original ACCESS_DENIED even if its directory fallback finds
    // the concurrently removed file absent. Other OS errors are not exempted.
    cfg!(windows) && error.raw_os_error() == Some(5)
}

#[cfg(test)]
mod tests;
