//! The HidHide service key is watchable without opening its restricted Parameters key.
use osg_streams::NativeWake;
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr::{null, null_mut},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
};
use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WIN32_ERROR},
    System::{
        Registry::*,
        Threading::{CreateEventW, SetEvent, WaitForMultipleObjects, INFINITE},
    },
};

const SERVICE_KEY: &str = r"SYSTEM\CurrentControlSet\Services\HidHide";

pub struct PolicyWatch {
    live: Arc<AtomicBool>,
    stop: Arc<OwnedHandle>,
    thread: Option<JoinHandle<()>>,
}

impl PolicyWatch {
    pub(super) fn hidhide(wake: Arc<NativeWake>) -> io::Result<Self> {
        Self::start(HKEY_LOCAL_MACHINE, SERVICE_KEY, wake)
    }

    pub(in crate::visibility) fn start(hive: HKEY, path: &str, wake: Arc<NativeWake>) -> io::Result<Self> {
        let path: Vec<_> = path.encode_utf16().chain([0]).collect();
        let mut handle = null_mut();
        status(unsafe { RegOpenKeyExW(hive, path.as_ptr(), 0, KEY_NOTIFY | KEY_WOW64_64KEY, &mut handle) })?;
        let key = Key(handle);
        let changed = event()?;
        let stop = Arc::new(event()?);
        let live = Arc::new(AtomicBool::new(true));
        key.arm(&changed)?;
        let worker_stop = stop.clone();
        let worker_live = live.clone();
        let thread = thread::Builder::new().name("hidhide-policy-watch".into())
            .spawn(move || observe(key, changed, worker_stop, worker_live, wake))?;
        Ok(Self { live, stop, thread: Some(thread) })
    }

    pub(in crate::visibility) fn is_live(&self) -> bool {
        self.live.load(Ordering::Acquire)
    }
}

impl Drop for PolicyWatch {
    fn drop(&mut self) {
        let signaled = unsafe { SetEvent(self.stop.as_raw_handle()) };
        if signaled != 0 {
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

struct Key(HKEY);

// Only the owner thread rearms and closes the registered key.
unsafe impl Send for Key {}

impl Key {
    fn arm(&self, changed: &OwnedHandle) -> io::Result<()> {
        let filter = REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_CHANGE_SECURITY
            | REG_NOTIFY_THREAD_AGNOSTIC;
        status(unsafe { RegNotifyChangeKeyValue(self.0, 1, filter, changed.as_raw_handle(), 1) })
    }
}

impl Drop for Key {
    fn drop(&mut self) { unsafe { RegCloseKey(self.0) }; }
}

fn status(code: WIN32_ERROR) -> io::Result<()> {
    if code == 0 { Ok(()) } else { Err(io::Error::from_raw_os_error(code as i32)) }
}

fn event() -> io::Result<OwnedHandle> {
    let handle = unsafe { CreateEventW(null(), 0, 0, null()) };
    if handle.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }
}

fn observe(
    key: Key, changed: OwnedHandle, stop: Arc<OwnedHandle>, live: Arc<AtomicBool>, wake: Arc<NativeWake>,
) {
    let handles = [stop.as_raw_handle(), changed.as_raw_handle()];
    loop {
        let signaled = unsafe { WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 0, INFINITE) };
        if signaled == WAIT_OBJECT_0 { return; }
        if signaled != WAIT_OBJECT_0 + 1 || key.arm(&changed).is_err() { break; }
        // Rearm before the authoritative read so a change racing it is retained.
        wake.notify();
    }
    live.store(false, Ordering::Release);
    wake.notify();
}
