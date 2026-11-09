use std::{
    fs::File,
    io::{ErrorKind, Read},
    path::Path,
};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::LocalVolume;
#[cfg(not(windows))]
type LocalVolume = ();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JournalMode {
    Delete,
    Wal(LocalVolume),
}

impl JournalMode {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Delete => "delete",
            Self::Wal(_) => "wal",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Header {
    Empty,
    Delete,
    Wal,
    Other,
}

fn header(path: &Path) -> Result<Header, String> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Header::Empty),
        Err(_) => return Err("Cannot inspect the persistence journal header.".into()),
    };
    let mut bytes = [0; 20];
    if let Err(error) = file.read_exact(&mut bytes) {
        if error.kind() != ErrorKind::UnexpectedEof {
            return Err("Cannot inspect the persistence journal header.".into());
        }
        return match file.metadata().map(|metadata| metadata.len()) {
            Ok(0) => Ok(Header::Empty),
            Ok(_) => Ok(Header::Other),
            Err(_) => Err("Cannot inspect the persistence journal header.".into()),
        };
    }
    // SQLite file-format read and write versions, independent of PRAGMA.
    Ok(match (&bytes[..16], &bytes[18..]) {
        (b"SQLite format 3\0", [1, 1]) => Header::Delete,
        (b"SQLite format 3\0", [2, 2]) => Header::Wal,
        _ => Header::Other,
    })
}

pub(super) fn no_sidecars(path: &Path) -> bool {
    ["-journal", "-wal", "-shm"].iter().all(|suffix| {
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        std::fs::symlink_metadata(name).is_err_and(|error| error.kind() == ErrorKind::NotFound)
    })
}

fn local_storage(path: &Path) -> Option<LocalVolume> {
    #[cfg(windows)]
    return windows::eligible(path);
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

pub(crate) fn select(path: &Path) -> Result<(JournalMode, bool), String> {
    crate::check_path(path, false)?;
    crate::path::check_sidecars(path)?;
    let format = header(path)?;
    if format == Header::Wal {
        return if let Some(volume) = local_storage(path) {
            Ok((JournalMode::Wal(volume), false))
        } else {
            Err("The WAL database requires its verified local Windows storage. Preserve the database and its sidecars.".into())
        };
    }
    // Leave any rollback journal to the existing recovery path. Promotion
    // can be reconsidered at the next exclusive startup.
    if matches!(format, Header::Delete | Header::Empty) && no_sidecars(path) {
        if let Some(volume) = local_storage(path) {
            return Ok((JournalMode::Wal(volume), true));
        }
    }
    Ok((JournalMode::Delete, false))
}

pub(crate) fn check_open(path: &Path, mode: JournalMode, enable_wal: bool) -> Result<(), String> {
    let format = header(path)?;
    let valid = match mode {
        JournalMode::Delete => format != Header::Wal,
        JournalMode::Wal(volume) => {
            local_storage(path) == Some(volume)
                && (format == Header::Wal
                    || (enable_wal
                        && matches!(format, Header::Empty | Header::Delete)
                        && no_sidecars(path)))
        }
    };
    if valid {
        Ok(())
    } else {
        Err("Persistence journal mode or storage location changed. Preserve the database and its sidecars.".into())
    }
}

pub(crate) fn require_snapshot(path: &Path) -> Result<(), String> {
    crate::path::check_live_path(path)?;
    if header(path)? != Header::Delete || !no_sidecars(path) {
        return Err("The backup must be a standalone rollback-journal database.".into());
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests;
