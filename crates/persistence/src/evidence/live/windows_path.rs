//! Check the full path against SQLite's retained handle on verified local WAL storage.
use rusqlite::{ffi, Connection};
use std::{
    ffi::c_void,
    fs::File,
    mem::{size_of, MaybeUninit},
    os::windows::{
        ffi::OsStrExt,
        fs::MetadataExt,
        io::{AsRawHandle, FromRawHandle, RawHandle},
    },
    path::{Component, Path, PathBuf, Prefix},
    ptr,
};

const OBJ_CASE_INSENSITIVE: u32 = 0x40;
const OBJ_DONT_REPARSE: u32 = 0x1000;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const SYNCHRONIZE: u32 = 0x0010_0000;
const FILE_SHARE_ALL: u32 = 7;
const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x20;
const FILE_NON_DIRECTORY_FILE: u32 = 0x40;
const FILE_ID_INFO_CLASS: i32 = 18;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const STATUS_INVALID_PARAMETER: i32 = 0xc000_000du32 as i32;
const STATUS_NOT_SUPPORTED: i32 = 0xc000_00bbu32 as i32;
const PATH_CHANGED: &str = "Recovery evidence storage was removed, redirected or replaced.";

#[repr(C)]
struct UnicodeString {
    length: u16,
    capacity: u16,
    buffer: *const u16,
}

#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root_directory: RawHandle,
    object_name: *const UnicodeString,
    attributes: u32,
    security_descriptor: *const c_void,
    security_quality_of_service: *const c_void,
}

#[repr(C)]
struct IoStatusBlock {
    status_or_pointer: usize,
    information: usize,
}

#[repr(C)]
#[derive(PartialEq, Eq)]
struct FileId {
    volume: u64,
    identifier: [u8; 16],
}

#[link(name = "ntdll")]
extern "system" {
    fn NtOpenFile(
        handle: *mut RawHandle,
        access: u32,
        attributes: *const ObjectAttributes,
        status: *mut IoStatusBlock,
        share: u32,
        options: u32,
    ) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetFileInformationByHandleEx(
        handle: RawHandle,
        information_class: i32,
        information: *mut c_void,
        length: u32,
    ) -> i32;
}

pub(super) struct VerifiedPath {
    name: Vec<u16>,
    id: FileId,
}

impl VerifiedPath {
    pub(super) fn open(connection: &Connection, path: &Path) -> Result<Option<Self>, String> {
        let Some(name) = object_name(path) else {
            return Ok(None);
        };
        let file = match open_regular(&name) {
            Ok(file) => file,
            Err(STATUS_INVALID_PARAMETER | STATUS_NOT_SUPPORTED) => return Ok(None),
            Err(_) => return Err(PATH_CHANGED.into()),
        };
        let mut handle: RawHandle = ptr::null_mut();
        // SAFETY: main is NUL terminated and the bundled Windows VFS writes
        // one borrowed HANDLE. The connection remains alive during the query.
        let result = unsafe {
            ffi::sqlite3_file_control(
                connection.handle(),
                c"main".as_ptr(),
                ffi::SQLITE_FCNTL_WIN32_GET_HANDLE,
                (&mut handle as *mut RawHandle).cast(),
            )
        };
        if result != ffi::SQLITE_OK {
            return Err("Cannot inspect SQLite's retained Windows file identity.".into());
        }
        let proof = Self {
            name,
            id: file_id(handle).ok_or("Cannot inspect SQLite's retained Windows file identity.")?,
        };
        proof.check_file(&file)?;
        Ok(Some(proof))
    }

    pub(super) fn check(&self, path: &Path) -> Result<(), String> {
        self.check_main()?;
        crate::path::check_sidecars_with(path, || self.check_main())
    }

    fn check_main(&self) -> Result<(), String> {
        let file = open_regular(&self.name).map_err(|_| PATH_CHANGED)?;
        self.check_file(&file)
    }

    fn check_file(&self, file: &File) -> Result<(), String> {
        if file_id(file.as_raw_handle()).as_ref() == Some(&self.id)
            && file.metadata().is_ok_and(|metadata| {
                metadata.is_file() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
            })
        {
            Ok(())
        } else {
            Err(PATH_CHANGED.into())
        }
    }
}

fn object_name(path: &Path) -> Option<Vec<u16>> {
    let mut parts = path.components();
    let Component::Prefix(prefix) = parts.next()? else {
        return None;
    };
    if !matches!(prefix.kind(), Prefix::Disk(_)) || parts.next() != Some(Component::RootDir) {
        return None;
    }
    let mut ordinary = PathBuf::from(prefix.as_os_str());
    ordinary.push(r"\");
    for part in parts {
        let Component::Normal(name) = part else {
            return None;
        };
        let units: Vec<_> = name.encode_wide().collect();
        if units.iter().any(|unit| matches!(*unit, 0 | 58)) || matches!(units.last(), Some(32 | 46))
        {
            return None;
        }
        ordinary.push(name);
    }
    let name: Vec<_> = r"\??\"
        .encode_utf16()
        .chain(ordinary.as_os_str().encode_wide())
        .collect();
    // Preserve the standard Rust path handling for unusual Win32 names.
    (name.len() < 260).then_some(name)
}

fn open_regular(name: &[u16]) -> Result<File, i32> {
    let bytes =
        u16::try_from(name.len() * size_of::<u16>()).map_err(|_| STATUS_INVALID_PARAMETER)?;
    let name = UnicodeString {
        length: bytes,
        capacity: bytes,
        buffer: name.as_ptr(),
    };
    let attributes = ObjectAttributes {
        length: size_of::<ObjectAttributes>() as u32,
        root_directory: ptr::null_mut(),
        object_name: &name,
        attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
        security_descriptor: ptr::null(),
        security_quality_of_service: ptr::null(),
    };
    let mut status = IoStatusBlock {
        status_or_pointer: 0,
        information: 0,
    };
    let mut handle = ptr::null_mut();
    // SAFETY: all descriptors and the counted UTF-16 buffer remain alive for
    // the synchronous call. A successful returned handle is owned by File.
    let result = unsafe {
        NtOpenFile(
            &mut handle,
            SYNCHRONIZE | FILE_READ_ATTRIBUTES,
            &attributes,
            &mut status,
            FILE_SHARE_ALL,
            FILE_SYNCHRONOUS_IO_NONALERT | FILE_NON_DIRECTORY_FILE,
        )
    };
    if result != 0 || handle.is_null() {
        return Err(result);
    }
    Ok(unsafe { File::from_raw_handle(handle) })
}

fn file_id(handle: RawHandle) -> Option<FileId> {
    let mut id = MaybeUninit::<FileId>::uninit();
    // SAFETY: GetFileInformationByHandleEx writes exactly one FILE_ID_INFO
    // into the correctly sized buffer when it succeeds.
    let result = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FILE_ID_INFO_CLASS,
            id.as_mut_ptr().cast(),
            size_of::<FileId>() as u32,
        )
    };
    if result == 0 {
        return None;
    }
    let id = unsafe { id.assume_init() };
    (id.identifier != [0; 16]).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_handle_requires_the_same_file_even_when_contents_match() {
        let directory = crate::test_directory();
        let path = directory.path().join("current.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TABLE value(x)").unwrap();
        let mut proof = VerifiedPath::open(&connection, &path).unwrap().unwrap();
        proof.check(&path).unwrap();
        let other = directory.path().join("other.sqlite3");
        std::fs::copy(&path, &other).unwrap();
        proof.name = object_name(&other).unwrap();
        assert!(proof.check(&other).unwrap_err().contains("replaced"));
        let absent = directory.path().join("absent.sqlite3");
        proof.name = object_name(&absent).unwrap();
        assert!(proof.check(&absent).is_err());
    }

    #[test]
    fn unusual_names_use_the_existing_path_checks() {
        for path in [
            r"C:file",
            r"C:\foo\..\file",
            r"C:\dir \file",
            r"C:\file.",
            r"C:\file:alternate",
            r"\\?\C:\file",
            r"\\server\share\file",
        ] {
            assert!(object_name(Path::new(path)).is_none(), "{path}");
        }
        assert!(object_name(Path::new(r"C:\directory\file")).is_some());
    }

    #[test]
    fn verified_local_wal_uses_the_check_and_refuses_a_junction_to_the_same_file() {
        let directory = crate::test_directory();
        let real = directory.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let path = real.join("cockpit.sqlite3");
        let mut lease = crate::NamespaceLease::acquire(&path).unwrap();
        let database = lease.open_database().unwrap();
        assert_eq!(database.journal_mode(), "wal");
        let mut live = super::super::LiveEvidence::open(&database).unwrap();
        let proof = live
            .storage
            .verified
            .as_mut()
            .expect("local WAL handle proof");
        proof.check(&path).unwrap();
        let journal = path.with_file_name("cockpit.sqlite3-journal");
        std::fs::create_dir(&journal).unwrap();
        let invalid_sidecar = proof.check(&path);
        std::fs::remove_dir(&journal).unwrap();
        assert!(invalid_sidecar.is_err());

        let junction = directory.path().join("junction");
        let output = std::process::Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&real)
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output);
        let alias = junction.join("cockpit.sqlite3");
        assert_eq!(
            std::fs::canonicalize(&alias).unwrap(),
            std::fs::canonicalize(&path).unwrap()
        );
        proof.name = object_name(&alias).unwrap();
        let refused = proof.check(&alias);
        std::fs::remove_dir(&junction).unwrap();
        assert!(refused.is_err());
    }
}
