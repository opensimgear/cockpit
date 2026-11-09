//! WAL admission is limited to a positively identified local NTFS drive.
use std::{
    ffi::c_void,
    fs::{self, OpenOptions},
    io::ErrorKind,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::{Component, Path, Prefix},
    ptr,
};

const DRIVE_FIXED: u32 = 3;
const FILE_STANDARD_INFO: i32 = 1;
const OPEN_REPARSE_POINT: u32 = 0x0020_0000;

#[repr(C)]
struct FileStandardInfo {
    allocation_size: i64,
    end_of_file: i64,
    number_of_links: u32,
    delete_pending: u8,
    directory: u8,
}

#[link(name = "kernel32")]
extern "system" {
    fn QueryDosDeviceW(device: *const u16, target: *mut u16, capacity: u32) -> u32;
    fn GetDriveTypeW(root: *const u16) -> u32;
    fn GetVolumeInformationW(
        root: *const u16,
        label: *mut u16,
        label_capacity: u32,
        serial: *mut u32,
        max_component: *mut u32,
        flags: *mut u32,
        filesystem: *mut u16,
        filesystem_capacity: u32,
    ) -> i32;
    fn GetFileInformationByHandleEx(
        file: *mut c_void,
        class: i32,
        info: *mut c_void,
        size: u32,
    ) -> i32;
}

fn disk(path: &Path, verbatim: bool) -> Option<u8> {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return None;
    };
    if components.next() != Some(Component::RootDir) {
        return None;
    }
    match prefix.kind() {
        Prefix::Disk(letter) => Some(letter.to_ascii_uppercase()),
        Prefix::VerbatimDisk(letter) if verbatim => Some(letter.to_ascii_uppercase()),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LocalVolume {
    device_number: u32,
    serial: u32,
}

fn fixed_ntfs(letter: u8) -> Option<LocalVolume> {
    let device = [u16::from(letter), b':' as u16, 0];
    let root = [device[0], device[1], b'\\' as u16, 0];
    let mut targets = [0; 256];
    let mut filesystem = [0; 16];
    let mut serial = 0;
    // SAFETY: All input strings are terminated and the API receives each
    // writable array's capacity in UTF-16 code units.
    unsafe {
        if GetDriveTypeW(root.as_ptr()) != DRIVE_FIXED
            || QueryDosDeviceW(device.as_ptr(), targets.as_mut_ptr(), targets.len() as u32) == 0
        {
            return None;
        }
        if GetVolumeInformationW(
            root.as_ptr(),
            ptr::null_mut(),
            0,
            &mut serial,
            ptr::null_mut(),
            ptr::null_mut(),
            filesystem.as_mut_ptr(),
            filesystem.len() as u32,
        ) == 0
        {
            return None;
        }
    }
    let target = String::from_utf16_lossy(
        &targets[..targets
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(targets.len())],
    );
    let device_number = target
        .strip_prefix(r"\Device\HarddiskVolume")
        .filter(|number| number.bytes().all(|byte| byte.is_ascii_digit()))?
        .parse()
        .ok()?;
    filesystem
        .starts_with(&[b'N' as u16, b'T' as u16, b'F' as u16, b'S' as u16, 0])
        .then_some(LocalVolume {
            device_number,
            serial,
        })
}

fn unaliased_file(path: &Path) -> bool {
    let file = match OpenOptions::new()
        .access_mode(0)
        .custom_flags(OPEN_REPARSE_POINT)
        .open(path)
    {
        Ok(file) => file,
        Err(error) => return error.kind() == ErrorKind::NotFound,
    };
    let mut info = std::mem::MaybeUninit::<FileStandardInfo>::uninit();
    // SAFETY: The borrowed handle outlives the call. A successful call writes
    // one FILE_STANDARD_INFO into the correctly sized, aligned buffer.
    let info = unsafe {
        if GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FILE_STANDARD_INFO,
            info.as_mut_ptr().cast(),
            std::mem::size_of::<FileStandardInfo>() as u32,
        ) == 0
        {
            return false;
        }
        info.assume_init()
    };
    info.number_of_links == 1 && info.delete_pending == 0 && info.directory == 0
}

pub(super) fn eligible(path: &Path) -> Option<LocalVolume> {
    // The generic path guard already rejects reparse points. Compare the
    // resolved parent drive as well, without rewriting the configured path.
    let letter = disk(path, false)?;
    let volume = fixed_ntfs(letter)?;
    let parent = fs::canonicalize(path.parent()?).ok()?;
    if disk(&parent, true) != Some(letter) || !unaliased_file(path) {
        return None;
    }
    ["-wal", "-shm"]
        .iter()
        .all(|suffix| {
            let mut name = path.as_os_str().to_os_string();
            name.push(suffix);
            unaliased_file(Path::new(&name))
        })
        .then_some(volume)
}
