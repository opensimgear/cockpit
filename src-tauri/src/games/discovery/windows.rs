#[cfg(windows)]
mod native {
    use super::*;
    use std::{ffi::c_void, mem, ptr};
    type RawHandle = *mut c_void;
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[repr(C)]
    struct ProcessEntry {
        size: u32,
        usage: u32,
        pid: u32,
        heap: usize,
        module: u32,
        threads: u32,
        parent: u32,
        priority: i32,
        flags: u32,
        executable: [u16; 260],
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> RawHandle;
        fn Process32FirstW(snapshot: RawHandle, entry: *mut ProcessEntry) -> i32;
        fn Process32NextW(snapshot: RawHandle, entry: *mut ProcessEntry) -> i32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> RawHandle;
        fn GetProcessTimes(
            handle: RawHandle,
            created: *mut FileTime,
            exited: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        fn QueryFullProcessImageNameW(
            handle: RawHandle,
            flags: u32,
            buffer: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn WaitForSingleObject(handle: RawHandle, milliseconds: u32) -> u32;
        fn CloseHandle(handle: RawHandle) -> i32;
    }
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegGetValueW(
            key: RawHandle,
            subkey: *const u16,
            name: *const u16,
            flags: u32,
            kind: *mut u32,
            data: *mut c_void,
            size: *mut u32,
        ) -> i32;
    }
    struct Handle(RawHandle);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    fn registry(key: i32, subkey: &str, name: &str) -> Option<PathBuf> {
        let mut value = vec![0u16; 32768];
        let mut length = (value.len() * 2) as u32;
        let result = unsafe {
            RegGetValueW(
                key as isize as RawHandle,
                wide(subkey).as_ptr(),
                wide(name).as_ptr(),
                6,
                ptr::null_mut(),
                value.as_mut_ptr().cast(),
                &mut length,
            )
        };
        if result != 0 || length < 2 || length as usize > value.len() * 2 {
            return None;
        }
        value.truncate(length as usize / 2);
        let length = value.iter().position(|c| *c == 0).unwrap_or(value.len());
        let value = String::from_utf16(&value[..length]).ok()?;
        if value.is_empty() {
            None
        } else {
            Some(PathBuf::from(value))
        }
    }
    pub(super) fn roots() -> Roots {
        let mut roots = Roots {
            launch_supported: true,
            ..Roots::default()
        };
        for (key, subkey, name) in [
            (0x80000001u32 as i32, "Software\\Valve\\Steam", "SteamPath"),
            (
                0x80000002u32 as i32,
                "SOFTWARE\\WOW6432Node\\Valve\\Steam",
                "InstallPath",
            ),
            (
                0x80000002u32 as i32,
                "SOFTWARE\\Valve\\Steam",
                "InstallPath",
            ),
        ] {
            if let Some(path) = registry(key, subkey, name) {
                add_path(&mut roots.steam, path);
            }
        }
        for name in ["DCS World", "DCS World OpenBeta"] {
            if let Some(path) = registry(
                0x80000001u32 as i32,
                &format!("Software\\Eagle Dynamics\\{name}"),
                "Path",
            ) {
                add_path(&mut roots.dcs, path);
            }
        }
        for env in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Some(root) = std::env::var_os(env) {
                let root = PathBuf::from(root);
                add_path(&mut roots.steam, root.join("Steam"));
                for name in ["DCS World", "DCS World OpenBeta"] {
                    add_path(&mut roots.dcs, root.join("Eagle Dynamics").join(name));
                }
            }
        }
        roots
    }
    fn open(pid: u32) -> Result<Option<Handle>, String> {
        let handle = unsafe { OpenProcess(0x1000 | 0x00100000, 0, pid) };
        if handle.is_null() {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(87) {
                return Ok(None);
            }
            return Err(format!("Cannot inspect game process {pid}: {error}"));
        }
        Ok(Some(Handle(handle)))
    }
    fn identity(handle: &Handle, pid: u32) -> Result<Option<ProcessIdentity>, String> {
        let state = unsafe { WaitForSingleObject(handle.0, 0) };
        if state == 0 {
            return Ok(None);
        }
        if state != 258 {
            return Err("Cannot determine whether the game process is running.".into());
        }
        let (mut created, mut exited, mut kernel, mut user) = (
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
        );
        if unsafe { GetProcessTimes(handle.0, &mut created, &mut exited, &mut kernel, &mut user) }
            == 0
        {
            return Err(format!(
                "Cannot read game process creation time: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut image = vec![0u16; 32768];
        let mut length = image.len() as u32;
        if unsafe { QueryFullProcessImageNameW(handle.0, 0, image.as_mut_ptr(), &mut length) } == 0
        {
            return Err(format!(
                "Cannot read game process executable: {}",
                std::io::Error::last_os_error()
            ));
        }
        let path = String::from_utf16(&image[..length as usize])
            .map_err(|_| "Game process path is not valid text.")?;
        let epoch = ((created.high as u64) << 32) | created.low as u64;
        if epoch == 0 {
            return Err("Game process creation time is unavailable.".into());
        }
        if unsafe { WaitForSingleObject(handle.0, 0) } != 258 {
            return Ok(None);
        }
        Ok(Some(ProcessIdentity { pid, path, epoch }))
    }
    pub(super) fn spawn(request: &LaunchRequest) -> Result<(), String> {
        std::process::Command::new(&request.executable)
            .args(&request.arguments)
            .current_dir(&request.directory)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Could not launch the game: {e}"))
    }
    pub(super) fn find(paths: &[String]) -> Result<Option<ProcessIdentity>, String> {
        let exclusive = paths == ["X-Plane.exe"];
        inventory(paths)?.inspect(paths, exclusive)
    }
    pub(super) fn inventory(paths: &[String]) -> Result<ProcessInventory, String> {
        if paths.is_empty() {
            return Ok(ProcessInventory::default());
        }
        let exclusive = paths == ["X-Plane.exe"];
        let mut inventory = ProcessInventory::default();
        let raw = unsafe { CreateToolhelp32Snapshot(2, 0) };
        if raw as isize == -1 {
            return Err(format!(
                "Cannot list game processes: {}",
                std::io::Error::last_os_error()
            ));
        }
        let snapshot = Handle(raw);
        // PROCESSENTRY32W is plain integers plus an inline UTF-16 buffer.
        let mut entry: ProcessEntry = unsafe { mem::zeroed() };
        entry.size = mem::size_of::<ProcessEntry>() as u32;
        let mut available = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
        if !available && std::io::Error::last_os_error().raw_os_error() != Some(18) {
            return Err(format!(
                "Cannot inspect game process list: {}",
                std::io::Error::last_os_error()
            ));
        }
        while available {
            let length = entry.executable.iter().position(|c| *c == 0).unwrap_or(260);
            let basename = String::from_utf16_lossy(&entry.executable[..length]);
            if paths.iter().any(|p| {
                p.rsplit(['\\', '/'])
                    .next()
                    .is_some_and(|s| s.eq_ignore_ascii_case(&basename))
            }) {
                match open(entry.pid).and_then(|handle| match handle {
                    Some(handle) => identity(&handle, entry.pid),
                    None => Ok(None),
                }) {
                    Ok(Some(identity))
                        if exclusive || paths.iter().any(|p| paths_match(p, &identity.path)) =>
                    {
                        inventory.processes.push((identity, entry.parent))
                    }
                    Err(error) => {
                        inventory
                            .errors
                            .entry(basename.to_lowercase())
                            .or_insert(error);
                    }
                    _ => {}
                }
            }
            available = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
            if !available && std::io::Error::last_os_error().raw_os_error() != Some(18) {
                return Err(format!(
                    "Cannot continue inspecting game processes: {}",
                    std::io::Error::last_os_error()
                ));
            }
        }
        Ok(inventory)
    }
    pub(super) fn alive(expected: &ProcessIdentity) -> Result<bool, String> {
        let Some(handle) = open(expected.pid)? else {
            return Ok(false);
        };
        Ok(identity(&handle, expected.pid)?.is_some_and(|actual| {
            actual.epoch == expected.epoch && paths_match(&actual.path, &expected.path)
        }))
    }
}
