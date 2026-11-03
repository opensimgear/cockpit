//! Native invalidations only. Enumeration and connection policy belong to the
//! workspace owner; observing this source never opens a serial endpoint.
use std::sync::Arc;
use osg_streams::NativeWake;
mod directory;
#[cfg(windows)]
mod windows;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
use windows::Serial;
#[cfg(target_os = "linux")]
use linux::Serial;
#[cfg(target_os = "macos")]
use macos::Serial;

pub(crate) struct PortNotifications {
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    serial: Serial,
    directory: Option<directory::DirectoryNotifications>,
}
impl PortNotifications {
    pub(crate) fn start(wake: Arc<NativeWake>) -> Result<Self, String> {
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        {
            // Install native notifications before the caller takes its first
            // enumeration baseline, so an arrival during enumeration wakes it.
            let serial = Serial::start(wake.clone())?;
            let directory = crate::local_gateway::advertisement_directory()
                .map(|path| directory::DirectoryNotifications::start(path, wake)).transpose()?;
            Ok(Self { serial, directory })
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            let _ = wake;
            Err("Native serial-port notifications are not implemented on this platform. Automatic workspace discovery is unavailable.".into())
        }
    }
    pub(crate) fn error(&self) -> Option<String> {
        let directory = self.directory.as_ref().and_then(|watch| watch.error());
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        { self.serial.error().or(directory) }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        { directory }
    }
    pub(crate) fn shutdown(&mut self) -> Result<(), String> {
        if let Some(mut directory) = self.directory.take() { directory.shutdown(); }
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        { self.serial.shutdown() }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        { Ok(()) }
    }
}
impl Drop for PortNotifications { fn drop(&mut self) { let _ = self.shutdown(); } }

#[cfg(test)] #[cfg(windows)]
mod tests {
    use super::*;
    #[test]
    fn native_windows_registration_has_explicit_status_and_idempotent_shutdown(){
        let wake=Arc::new(NativeWake::default());
        let mut source=PortNotifications::start(wake.clone()).expect("Native CM registration must be available without a gateway");
        assert!(source.error().is_none());
        assert!(source.directory.is_none(),"Windows has no Unix advertisement directory");
        source.shutdown().unwrap();source.shutdown().unwrap();
        assert!(source.error().is_none());
        drop(source);
    }
}
