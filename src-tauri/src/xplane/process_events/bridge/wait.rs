//! A registered kernel wait, cancelled by unregistering rather than a timer.
use super::super::native::{Handle, RawHandle};
use std::{ffi::c_void, ptr, sync::Mutex};
use tokio::sync::oneshot;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn RegisterWaitForSingleObject(
        wait: *mut RawHandle,
        object: RawHandle,
        callback: unsafe extern "system" fn(*mut c_void, u8),
        context: *mut c_void,
        milliseconds: u32,
        flags: u32,
    ) -> i32;
    fn UnregisterWaitEx(wait: RawHandle, completion: RawHandle) -> i32;
}
struct Signal(Mutex<Option<oneshot::Sender<()>>>);
pub(super) struct Exit {
    registration: RawHandle,
    signal: Option<Box<Signal>>,
    _process: Option<Handle>,
    receiver: oneshot::Receiver<()>,
}
// The registration owns a stable boxed callback context and retained kernel handle.
unsafe impl Send for Exit {}
impl Exit {
    pub(super) fn new(process: Handle) -> Result<Self, String> {
        let (sender, receiver) = oneshot::channel();
        let mut signal = Box::new(Signal(Mutex::new(Some(sender))));
        let mut registration = ptr::null_mut();
        if unsafe {
            RegisterWaitForSingleObject(
                &mut registration,
                process.raw(),
                notify,
                (&mut *signal as *mut Signal).cast(),
                u32::MAX,
                8,
            )
        } == 0
        {
            return Err(format!(
                "Cannot register native process exit wait: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self {
            registration,
            signal: Some(signal),
            _process: Some(process),
            receiver,
        })
    }
    pub(super) async fn wait(&mut self) -> Result<(), String> {
        (&mut self.receiver)
            .await
            .map_err(|_| "Native process exit wait disconnected.".into())
    }
    pub(super) fn process(&self) -> &Handle {
        self._process.as_ref().unwrap()
    }
}
unsafe extern "system" fn notify(context: *mut c_void, _: u8) {
    let signal = unsafe { &*context.cast::<Signal>() };
    if let Ok(mut sender) = signal.0.lock() {
        if let Some(sender) = sender.take() {
            let _ = sender.send(());
        }
    }
}
impl Drop for Exit {
    fn drop(&mut self) {
        // INVALID_HANDLE_VALUE makes unregister wait for any callback in flight.
        // That callback only sends one wake-up, and never calls this destructor.
        if unsafe { UnregisterWaitEx(self.registration, -1isize as RawHandle) } == 0 {
            // In an unexpected unregister failure the callback may remain live.
            // Leak its tiny context and retained handle rather than permit UAF.
            // The helper's process exit reclaims these; parent qualification must
            // still treat abnormal native wait cleanup as unproven.
            if let Some(signal) = self.signal.take() {
                std::mem::forget(signal);
            }
            if let Some(process) = self._process.take() {
                std::mem::forget(process);
            }
        }
    }
}
