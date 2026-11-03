//! Configuration-manager callbacks carry no device strings, handles or I/O.
use super::*;
use std::{ffi::c_void, sync::atomic::{AtomicBool, Ordering}};
use ::windows::Win32::Devices::DeviceAndDriverInstallation::*;

struct Context { wake: Arc<NativeWake>, closed: AtomicBool }
impl Context {
    fn dispatch(&self, action: CM_NOTIFY_ACTION) {
        if !self.closed.load(Ordering::Acquire)
            && matches!(action, CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL | CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL) {
            self.wake.notify();
        }
    }
}
pub(super) struct Serial { handle: Option<HCMNOTIFICATION>, context: Option<Box<Context>> }
// CM notification registrations are not thread-affine. The boxed callback
// address stays stable, and exclusive shutdown waits for pending callbacks.
unsafe impl Send for Serial {}
impl Serial {
    pub(super) fn start(wake: Arc<NativeWake>) -> Result<Self, String> {
        Self::register(wake, |context, handle| {
            let filter = CM_NOTIFY_FILTER { cbSize: size_of::<CM_NOTIFY_FILTER>() as u32,
                Flags: CM_NOTIFY_FILTER_FLAG_ALL_INTERFACE_CLASSES,
                FilterType: CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE, ..Default::default() };
            // SAFETY: the filter lives through registration, callback context
            // is boxed before registration and retained through unregistration.
            unsafe { CM_Register_Notification(&filter, Some(context), Some(callback), handle) }
        })
    }
    fn register(wake: Arc<NativeWake>, register: impl FnOnce(*const c_void, &mut HCMNOTIFICATION) -> CONFIGRET) -> Result<Self, String> {
        let context = Box::new(Context { wake, closed: AtomicBool::new(false) });
        let mut handle = HCMNOTIFICATION::default();
        let result = register((&*context as *const Context).cast(), &mut handle);
        if result != CR_SUCCESS { return Err(format!("Cannot start serial-port notifications (Configuration Manager {}).", result.0)); }
        Ok(Self { handle: Some(handle), context: Some(context) })
    }
    pub(super) fn shutdown(&mut self) -> Result<(), String> {
        self.shutdown_with(|handle| unsafe { CM_Unregister_Notification(handle) })
    }
    fn shutdown_with(&mut self, unregister: impl FnOnce(HCMNOTIFICATION) -> CONFIGRET) -> Result<(), String> {
        let Some(handle) = self.handle.take() else { return Ok(()); };
        if let Some(context) = &self.context { context.closed.store(true, Ordering::Release); }
        // Must run on the owner, never on the callback. CM_Unregister waits for
        // pending callbacks before a successful return permits freeing context.
        let result = unregister(handle);
        if result != CR_SUCCESS {
            // Failure cannot establish callback quiescence. Retain the closed
            // inert context rather than freeing memory still owned by Windows.
            if let Some(context) = self.context.take() { Box::leak(context); }
            return Err(format!("Cannot stop serial-port notifications (Configuration Manager {}).", result.0));
        }
        self.context = None;
        Ok(())
    }
}
impl Drop for Serial { fn drop(&mut self) { let _ = self.shutdown(); } }

unsafe extern "system" fn callback(_: HCMNOTIFICATION, context: *const c_void, action: CM_NOTIFY_ACTION,
    _: *const CM_NOTIFY_EVENT_DATA, _: u32) -> u32 {
    if !context.is_null() {
        // SAFETY: Serial retains this stable allocation until successful
        // unregistration has waited for this and all other pending callbacks.
        unsafe { &*context.cast::<Context>() }.dispatch(action);
    }
    0
}

#[cfg(test)]
mod tests;
