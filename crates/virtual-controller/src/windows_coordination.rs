//! Transient provisioning coordination. Cancellation never kills the helper or
//! an in-flight PnP child; the transaction observes it between bounded steps.
use crate::{
    registry::wide,
    windows_process::{Identity, Process},
};
use std::{
    ptr,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0,
        WAIT_TIMEOUT,
    },
    System::Threading::*,
};
const LOCK: &str = "Global\\OpenSimGear.vJoy.Provision.v1";
fn error() -> String {
    format!(
        "Provisioning coordination failed: {}.",
        std::io::Error::last_os_error()
    )
}
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn operation(id: &str) -> Result<(), String> {
    if uuid::Uuid::parse_str(id)
        .ok()
        .map(|u| u.to_string())
        .as_deref()
        != Some(id)
    {
        return Err("Provisioning operation requires a canonical UUID.".into());
    }
    Ok(())
}
fn event_name(id: &str) -> Result<Vec<u16>, String> {
    operation(id)?;
    wide(&format!("Local\\OpenSimGear.vJoy.Cancel.{id}"))
}
pub struct Cancellation(Handle);
// Event handles support concurrent SetEvent/WaitForSingleObject across threads.
// The final Arc owner closes the handle; the thread-affine mutex is not Send.
unsafe impl Send for Cancellation {}
unsafe impl Sync for Cancellation {}
impl Cancellation {
    pub fn create(id: &str) -> Result<Self, String> {
        let name = event_name(id)?;
        let handle = unsafe { CreateEventW(ptr::null(), 1, 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(error());
        }
        let exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        let event = Self(Handle(handle));
        if exists {
            return Err("Cancellation identity already exists; refusing to adopt it.".into());
        }
        Ok(event)
    }
    pub fn open(id: &str) -> Result<Self, String> {
        let name = event_name(id)?;
        let handle = unsafe { OpenEventW(SYNCHRONIZATION_SYNCHRONIZE, 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(error());
        }
        Ok(Self(Handle(handle)))
    }
    pub fn cancel(&self) -> Result<(), String> {
        if unsafe { SetEvent(self.0 .0) } == 0 {
            Err(error())
        } else {
            Ok(())
        }
    }
    pub fn cancelled(&self) -> Result<bool, String> {
        match unsafe { WaitForSingleObject(self.0 .0, 0) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(error()),
        }
    }
}
pub struct Exclusive(Handle);
impl Exclusive {
    pub fn acquire() -> Result<Self, String> {
        Self::named(LOCK)
    }
    fn named(name: &str) -> Result<Self, String> {
        let name = wide(name)?;
        let handle = unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(error());
        }
        let handle = Handle(handle);
        match unsafe { WaitForSingleObject(handle.0, 0) } {
            WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Self(handle)),
            WAIT_TIMEOUT => Err("Another provisioning helper is still running.".into()),
            _ => Err(error()),
        }
    }
}
impl Drop for Exclusive {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0 .0);
        }
    }
}
pub struct Monitor {
    owner: Process,
    cancellation: Cancellation,
    started: Instant,
}
fn allowed(alive: bool, cancelled: bool, elapsed: Duration) -> Result<(), String> {
    if !alive {
        return Err("Cockpit exited; provisioning cancelled.".into());
    }
    if cancelled {
        return Err("Provisioning cancelled by its owner.".into());
    }
    if elapsed >= Duration::from_secs(60) {
        return Err("Provisioning operation deadline expired.".into());
    }
    Ok(())
}
impl Monitor {
    pub fn open(id: &str, owner: &Identity) -> Result<Self, String> {
        operation(id)?;
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        let owner = Process::track(owner, &executable)?.ok_or("Cockpit owner no longer exists.")?;
        Ok(Self {
            owner,
            cancellation: Cancellation::open(id)?,
            started: Instant::now(),
        })
    }
    pub fn check(&self) -> Result<(), String> {
        allowed(
            self.owner.alive()?,
            self.cancellation.cancelled()?,
            self.started.elapsed(),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_cancellation_is_exact_persistent_and_never_adopts_an_existing_signal() {
        let id = uuid::Uuid::new_v4().to_string();
        assert!(Cancellation::open(&id).is_err());
        assert!(Cancellation::create("bad").is_err());
        let event = Cancellation::create(&id).unwrap();
        assert!(Cancellation::create(&id).is_err());
        let reader = Cancellation::open(&id).unwrap();
        assert!(!event.cancelled().unwrap());
        assert!(!reader.cancelled().unwrap());
        assert!(reader.cancel().is_err()); // Helper has only synchronization access.
        let monitor = Monitor::open(&id, &crate::windows_process::current().unwrap()).unwrap();
        monitor.check().unwrap();
        event.cancel().unwrap();
        assert!(reader.cancelled().unwrap());
        assert!(monitor.check().unwrap_err().contains("cancelled"));
        drop(event);
        assert!(reader.cancelled().unwrap()); // Held exact kernel object survives sender exit.
        assert!(Monitor::open("bad", &crate::windows_process::current().unwrap()).is_err());
        let mut stale = crate::windows_process::current().unwrap();
        stale.created = "1".into();
        assert!(Monitor::open(&id, &stale).is_err());
        assert!(allowed(false, false, Duration::ZERO)
            .unwrap_err()
            .contains("exited"));
        assert!(allowed(true, false, Duration::from_secs(60))
            .unwrap_err()
            .contains("deadline"));
    }
    #[test]
    fn helper_mutex_excludes_other_threads_and_releases_on_normal_or_abandoned_exit() {
        let name = format!("Local\\OpenSimGear.Test.{}", uuid::Uuid::new_v4());
        let guard = Exclusive::named(&name).unwrap();
        let copy = name.clone();
        assert!(std::thread::spawn(move || Exclusive::named(&copy).is_err())
            .join()
            .unwrap());
        drop(guard);
        let copy = name.clone();
        let abandoned = std::thread::spawn(move || {
            let guard = Exclusive::named(&copy).unwrap();
            // Thread termination abandons ownership; preserve one handle to test that path.
            let handle = guard.0 .0 as usize;
            std::mem::forget(guard);
            handle
        })
        .join()
        .unwrap();
        let guard = Exclusive::named(&name).unwrap();
        unsafe {
            CloseHandle(abandoned as HANDLE);
        }
        drop(guard);
        assert!(Exclusive::named("invalid\0name").is_err());
        assert!(allowed(true, false, Duration::ZERO).is_ok());
    }
}
