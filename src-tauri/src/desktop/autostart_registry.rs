use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_KEY_DELETED, WAIT_FAILED, WAIT_OBJECT_0},
    System::{
        Registry::{
            RegCloseKey, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER,
            HKEY_LOCAL_MACHINE, KEY_NOTIFY, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME,
        },
        Threading::{CreateEventW, SetEvent, WaitForMultipleObjects, INFINITE},
    },
};

const RUN: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

#[derive(Clone)]
pub(super) struct Source;

pub(super) struct Stop(OwnedHandle);

impl Stop {
    pub(super) fn new() -> io::Result<Self> { event().map(Self) }

    pub(super) fn signal(&self) { unsafe { SetEvent(self.0.as_raw_handle()) }; }
}

impl Source {
    pub(super) fn run(&self, stop: &Stop, capture: impl FnMut()) -> io::Result<()> {
        watch(&stop.0, &[(HKEY_CURRENT_USER, RUN), (HKEY_LOCAL_MACHINE, RUN),
            (HKEY_CURRENT_USER, APPROVED), (HKEY_LOCAL_MACHINE, APPROVED)], capture)
    }
}

pub(super) fn event() -> io::Result<OwnedHandle> {
    let handle = unsafe { CreateEventW(null(), 0, 0, null()) };
    if handle.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }
}

struct Key(HKEY);

impl Key {
    fn open(hive: HKEY, path: &str) -> io::Result<Self> {
        let path: Vec<_> = path.encode_utf16().chain([0]).collect();
        let mut key = null_mut();
        match unsafe { RegOpenKeyExW(hive, path.as_ptr(), 0, KEY_NOTIFY, &mut key) } {
            0 => Ok(Self(key)),
            error => Err(io::Error::from_raw_os_error(error as i32)),
        }
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

fn missing(error: &io::Error) -> bool {
    matches!(error.raw_os_error().map(|code| code as u32), Some(ERROR_FILE_NOT_FOUND | ERROR_KEY_DELETED))
}

struct Watch {
    _key: Key,
    event: OwnedHandle,
}

impl Watch {
    fn arm(hive: HKEY, leaf: &str) -> io::Result<Self> {
        let mut path = leaf;
        for _ in 0..16 {
            let key = match Key::open(hive, path) {
                Ok(key) => key,
                Err(error) if missing(&error) => match path.rsplit_once('\\') {
                    Some((parent, _)) => { path = parent; continue; }
                    None => return Err(error),
                },
                Err(error) => return Err(error),
            };
            let event = event()?;
            let parent = path != leaf;
            let filter = REG_NOTIFY_CHANGE_LAST_SET | if parent { REG_NOTIFY_CHANGE_NAME } else { 0 };
            let status = unsafe { RegNotifyChangeKeyValue(key.0, parent.into(), filter, event.as_raw_handle(), 1) };
            if status == ERROR_KEY_DELETED {
                path = leaf;
                continue;
            }
            if status != 0 {
                return Err(io::Error::from_raw_os_error(status as i32));
            }
            if parent {
                // Close the gap between the missing lookup and arming the parent.
                match Key::open(hive, leaf) {
                    Ok(_) => { path = leaf; continue; }
                    Err(error) if missing(&error) => {}
                    Err(error) => return Err(error),
                }
            }
            return Ok(Self { _key: key, event });
        }
        Err(io::Error::other("The auto-start registry keys changed while opening notifications."))
    }
}

fn watch(stop: &OwnedHandle, paths: &[(HKEY, &str)], mut capture: impl FnMut()) -> io::Result<()> {
    let mut watches = paths.iter().map(|(hive, path)| Watch::arm(*hive, path)).collect::<io::Result<Vec<_>>>()?;
    let mut handles = Vec::with_capacity(watches.len() + 1);
    loop {
        // Notifications are armed before each capture, including the baseline.
        capture();
        handles.clear();
        handles.push(stop.as_raw_handle());
        handles.extend(watches.iter().map(|watch| watch.event.as_raw_handle()));
        let signaled = unsafe { WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 0, INFINITE) };
        if signaled == WAIT_OBJECT_0 {
            return Ok(());
        }
        if signaled == WAIT_FAILED {
            return Err(io::Error::last_os_error());
        }
        let index = (signaled - WAIT_OBJECT_0 - 1) as usize;
        let (hive, path) = paths.get(index).ok_or_else(|| io::Error::other("Unexpected registry wait result."))?;
        watches[index] = Watch::arm(*hive, path)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::{mpsc, Arc}, thread, time::{Duration, Instant}};
    use windows_sys::Win32::System::{
        Registry::{RegCreateKeyExW, RegDeleteKeyW, RegSetValueExW, KEY_SET_VALUE, REG_DWORD},
        Threading::SetEvent,
    };

    struct TestKey(String);

    impl TestKey {
        fn wide(&self) -> Vec<u16> { self.0.encode_utf16().chain([0]).collect() }

        fn create(&self) -> Key {
            let mut key = null_mut();
            assert_eq!(unsafe {
                RegCreateKeyExW(HKEY_CURRENT_USER, self.wide().as_ptr(), 0, null(), 0,
                    KEY_SET_VALUE, null(), &mut key, null_mut())
            }, 0);
            Key(key)
        }

        fn remove(&self) -> u32 {
            unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, self.wide().as_ptr()) }
        }
    }

    impl Drop for TestKey {
        fn drop(&mut self) { self.remove(); }
    }

    #[test]
    fn observes_missing_leaf_value_change_deletion_and_recreation() {
        let leaf = TestKey(format!(r"SOFTWARE\OpenSimGear-Autostart-{}", uuid::Uuid::new_v4()));
        let (changes, captures) = mpsc::channel();
        let stop = Arc::new(event().unwrap());
        let worker_stop = stop.clone();
        let path = leaf.0.clone();
        let worker = thread::spawn(move || {
            watch(&worker_stop, &[(HKEY_CURRENT_USER, &path)], || {
                let _ = changes.send(Key::open(HKEY_CURRENT_USER, &path).is_ok());
            })
        });
        let outcome = std::panic::catch_unwind(|| {
            let captured = |expected| {
                let deadline = Instant::now() + Duration::from_secs(3);
                while captures.recv_timeout(deadline.saturating_duration_since(Instant::now())).unwrap() != expected {}
            };
            captured(false);
            let key = leaf.create();
            captured(true);
            while captures.try_recv().is_ok() {}
            let name: Vec<_> = "Probe".encode_utf16().chain([0]).collect();
            let value = 42_u32.to_le_bytes();
            assert_eq!(unsafe { RegSetValueExW(key.0, name.as_ptr(), 0, REG_DWORD, value.as_ptr(), value.len() as u32) }, 0);
            captured(true);
            drop(key);
            assert_eq!(leaf.remove(), 0);
            captured(false);
            drop(leaf.create());
            captured(true);
        });
        assert_ne!(unsafe { SetEvent(stop.as_raw_handle()) }, 0);
        worker.join().unwrap().unwrap();
        if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
    }
}
