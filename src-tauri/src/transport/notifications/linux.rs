//! A libudev monitor observes completed userspace rules, not early kernel adds.
//! Its FD and a cancellation socket are the only waits; no enumeration timer.
use super::*;
use std::{ffi::{c_char, c_int, c_void}, io::{self, Write}, os::fd::{AsRawFd, RawFd},
    os::unix::net::UnixStream, sync::{atomic::{AtomicBool, Ordering}, Mutex}, thread::{self, JoinHandle}};

#[repr(C)]
struct PollFd { fd: c_int, events: i16, revents: i16 }
const READ: i16 = 1;
const FAILED: i16 = 8 | 16 | 32;
const MAX_BATCH: usize = 64;

#[link(name = "udev")]
unsafe extern "C" {
    fn udev_new() -> *mut c_void;
    fn udev_unref(context: *mut c_void) -> *mut c_void;
    fn udev_monitor_new_from_netlink(context: *mut c_void, source: *const c_char) -> *mut c_void;
    fn udev_monitor_unref(monitor: *mut c_void) -> *mut c_void;
    fn udev_monitor_filter_add_match_subsystem_devtype(monitor: *mut c_void, subsystem: *const c_char, kind: *const c_char) -> c_int;
    fn udev_monitor_enable_receiving(monitor: *mut c_void) -> c_int;
    fn udev_monitor_get_fd(monitor: *mut c_void) -> c_int;
    fn udev_monitor_receive_device(monitor: *mut c_void) -> *mut c_void;
    fn udev_device_unref(device: *mut c_void) -> *mut c_void;
}
unsafe extern "C" { fn poll(fds: *mut PollFd, count: usize, timeout: c_int) -> c_int; }

struct Monitor { context: *mut c_void, monitor: *mut c_void, fd: RawFd }
impl Monitor {
    fn open() -> Result<Self, String> {
        // A netlink subscription without a userspace daemon would silently
        // stay quiet. Fail explicitly when the required udev service is absent.
        if !std::path::Path::new("/run/udev/control").exists() {
            return Err("Completed udev port notifications are unavailable: the udev service is not present.".into());
        }
        unsafe {
            let context = udev_new();
            if context.is_null() { return Err("Cannot create the udev port notification context.".into()); }
            let monitor = udev_monitor_new_from_netlink(context, c"udev".as_ptr());
            let mut result = Self { context, monitor, fd: -1 };
            if monitor.is_null() { return Err("Cannot create the completed udev port notification monitor.".into()); }
            if udev_monitor_filter_add_match_subsystem_devtype(monitor, c"tty".as_ptr(), std::ptr::null()) < 0
                || udev_monitor_enable_receiving(monitor) < 0 {
                return Err("Cannot subscribe to completed udev serial-port notifications.".into());
            }
            result.fd = udev_monitor_get_fd(monitor);
            if result.fd < 0 { return Err("The udev port notification monitor has no descriptor.".into()); }
            Ok(result)
        }
    }
    fn receive(&self) -> io::Result<bool> {
        let device = unsafe { udev_monitor_receive_device(self.monitor) };
        if !device.is_null() { unsafe { udev_device_unref(device); } return Ok(true); }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::WouldBlock { Ok(false) } else { Err(error) }
    }
}
impl Drop for Monitor {
    fn drop(&mut self) { unsafe {
        if !self.monitor.is_null() { udev_monitor_unref(self.monitor); }
        if !self.context.is_null() { udev_unref(self.context); }
    } }
}

pub(super) struct Serial {
    closed: Arc<AtomicBool>, cancel: Option<UnixStream>, worker: Option<JoinHandle<()>>,
    error: Arc<Mutex<Option<String>>>,
}
impl Serial {
    pub(super) fn start(wake: Arc<NativeWake>) -> Result<Self, String> {
        let (cancel, stopped) = UnixStream::pair().map_err(|_| "Cannot create port notification cancellation sockets.")?;
        cancel.set_nonblocking(true).map_err(|_| "Cannot configure port notification cancellation.")?;
        let closed = Arc::new(AtomicBool::new(false)); let shutdown = closed.clone();
        let error = Arc::new(Mutex::new(None)); let failure = error.clone();
        let (ready, started) = std::sync::mpsc::sync_channel(1);
        let worker = thread::Builder::new().name("serial-port-events".into()).spawn(move || {
            let monitor = match Monitor::open() {
                Ok(monitor) => { let _ = ready.send(Ok(())); monitor },
                Err(error) => { let _ = ready.send(Err(error)); return; },
            };
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run(monitor.fd, stopped.as_raw_fd(), &shutdown, &wake, || monitor.receive())
            }));
            let message = match outcome { Ok(Ok(())) => None, Ok(Err(error)) => Some(error), Err(_) => Some("The udev port notification owner failed.".into()) };
            if let Some(message) = message { *failure.lock().unwrap_or_else(|p|p.into_inner()) = Some(message); wake.notify(); }
        }).map_err(|_| "Cannot start the udev port notification owner.")?;
        match started.recv() {
            Ok(Ok(())) => Ok(Self { closed, cancel: Some(cancel), worker: Some(worker), error }),
            Ok(Err(error)) => { let _ = worker.join(); Err(error) },
            Err(_) => { let _ = worker.join(); Err("The udev port notification owner stopped during startup.".into()) },
        }
    }
    pub(super) fn error(&self) -> Option<String> { self.error.lock().unwrap_or_else(|p|p.into_inner()).clone() }
    pub(super) fn shutdown(&mut self) -> Result<(), String> {
        self.closed.store(true, Ordering::Release);
        if let Some(mut cancel) = self.cancel.take() { let _ = cancel.write(&[1]); }
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
        Ok(())
    }
}
impl Drop for Serial { fn drop(&mut self) { let _ = self.shutdown(); } }

fn run(fd: RawFd, cancel: RawFd, closed: &AtomicBool, wake: &NativeWake, mut receive: impl FnMut() -> io::Result<bool>) -> Result<(), String> {
    while !closed.load(Ordering::Acquire) {
        let mut descriptors = [PollFd { fd: cancel, events: READ, revents: 0 }, PollFd { fd, events: READ, revents: 0 }];
        if unsafe { poll(descriptors.as_mut_ptr(), descriptors.len(), -1) } < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted { continue; }
            return Err("Cannot wait for completed udev serial-port notifications.".into());
        }
        if closed.load(Ordering::Acquire) || descriptors[0].revents != 0 { return Ok(()); }
        if descriptors[1].revents & FAILED != 0 { return Err("Completed udev serial-port notification continuity was lost.".into()); }
        if descriptors[1].revents & READ != 0 {
            let mut changed = false;
            for _ in 0..MAX_BATCH {
                if closed.load(Ordering::Acquire) { return Ok(()); }
                match receive() {
                    Ok(true) => changed = true,
                    Ok(false) => break,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => break,
                    Err(_) => return Err("Completed udev serial-port notifications were lost; restart the workspace observer.".into()),
                }
            }
            if changed { wake.notify(); }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
