use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent};

/// Manual reset event is created before the worker starts, so shutdown cannot
/// race queue creation or be lost while the worker is idle.
pub struct Wake(HANDLE);

unsafe impl Send for Wake {}
unsafe impl Sync for Wake {}

impl Wake {
    pub fn new() -> Result<Self, String> {
        let handle = unsafe { CreateEventW(null(), 1, 0, null()) };
        if handle.is_null() {
            Err(win_error("Create controller capture wake event"))
        } else {
            Ok(Self(handle))
        }
    }

    pub fn signal(&self) -> Result<(), String> {
        if unsafe { SetEvent(self.0) } == 0 {
            Err(win_error("Wake controller capture worker"))
        } else {
            Ok(())
        }
    }
}

impl Drop for Wake {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WaitOutcome {
    Wake,
    Messages,
}

fn wait_for_work(wake: &Wake) -> Result<WaitOutcome, String> {
    let result = unsafe {
        MsgWaitForMultipleObjectsEx(1, &wake.0, u32::MAX, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
    };
    match result {
        WAIT_OBJECT_0 => Ok(WaitOutcome::Wake),
        result if result == WAIT_OBJECT_0 + 1 => Ok(WaitOutcome::Messages),
        WAIT_FAILED => Err(win_error("Wait for controller input")),
        _ => Err(format!("Unexpected controller wait result: {result}")),
    }
}

fn is_device_change(message: u32) -> bool {
    message == WM_INPUT_DEVICE_CHANGE
}

pub(super) fn run(shared: Shared, stop: Arc<AtomicBool>, wake: Arc<Wake>) -> Result<(), String> {
    let api = Api::native();
    unsafe {
        let class = wide(&format!("OpenSimGearInput-{}", uuid::Uuid::new_v4()));
        let instance = GetModuleHandleW(null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            return Err(win_error("Register controller input window"));
        }
        let window = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            instance,
            null(),
        );
        if window.is_null() {
            let error = win_error("Create controller input window");
            let cleanup = if UnregisterClassW(class.as_ptr(), instance) == 0 {
                Some(win_error("Unregister controller input window"))
            } else {
                None
            };
            return Err(cleanup.map_or(error.clone(), |e| format!("{error}; {e}")));
        }
        let registrations: Vec<_> = [4, 5, 8]
            .iter()
            .map(|usage| RAWINPUTDEVICE {
                usUsagePage: 1,
                usUsage: *usage,
                dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
                hwndTarget: window,
            })
            .collect();
        if RegisterRawInputDevices(
            registrations.as_ptr(),
            3,
            size_of::<RAWINPUTDEVICE>() as u32,
        ) == 0
        {
            let error = win_error("Register background controller capture");
            let cleanup = cleanup_window(window, &class, instance, false);
            return Err(append_cleanup(error, cleanup));
        }

        let mut devices = BTreeMap::new();
        shared
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .backend_status = "ready".into();
        if let Err(error) = enumerate(&api, &mut devices, &shared) {
            shared.lock().unwrap_or_else(|p| p.into_inner()).diagnostic = error;
        }
        let mut result = Ok(());
        while !stop.load(Ordering::Acquire) {
            match wait_for_work(&wake) {
                Ok(WaitOutcome::Wake) => break,
                Ok(WaitOutcome::Messages) => (),
                Err(error) => {
                    result = Err(error);
                    break;
                }
            }
            let mut rescan = false;
            let mut message = MSG::default();
            for _ in 0..256 {
                if PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) == 0 {
                    break;
                }
                if message.message == WM_INPUT {
                    // Arrival is a descriptor boundary. Install it before the
                    // next report, including a device that only emits changes.
                    if rescan {
                        if let Err(error) = enumerate(&api, &mut devices, &shared) {
                            shared.lock().unwrap_or_else(|p| p.into_inner()).diagnostic = error;
                        }
                        rescan = false;
                    }
                    if let Err(error) = input(&api, message.lParam as HRAWINPUT, &devices, &shared)
                    {
                        let mut registry = shared.lock().unwrap_or_else(|p| p.into_inner());
                        let ids: Vec<_> = devices
                            .values()
                            .filter_map(|d| registry.id_for_path(&d.path))
                            .collect();
                        for id in ids {
                            registry.fault(&id, &error);
                        }
                    }
                } else if is_device_change(message.message) {
                    if message.wParam == GIDC_REMOVAL as usize {
                        if let Some(device) = devices.remove(&(message.lParam as usize)) {
                            let mut registry = shared.lock().unwrap_or_else(|p| p.into_inner());
                            if let Some(id) = registry.id_for_path(&device.path) {
                                registry.disconnect(&id);
                            }
                        }
                    }
                    rescan = true;
                }
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            if rescan {
                if let Err(error) = enumerate(&api, &mut devices, &shared) {
                    shared.lock().unwrap_or_else(|p| p.into_inner()).diagnostic = error;
                }
            }
        }
        let cleanup = cleanup_window(window, &class, instance, true);
        match (result, cleanup) {
            (Err(error), cleanup) => Err(append_cleanup(error, cleanup)),
            (Ok(()), Some(error)) => Err(error),
            (Ok(()), None) => Ok(()),
        }
    }
}

unsafe fn cleanup_window(
    window: HWND,
    class: &[u16],
    instance: HINSTANCE,
    registered: bool,
) -> Option<String> {
    let mut errors = Vec::new();
    if registered {
        let removal: Vec<_> = [4, 5, 8]
            .iter()
            .map(|usage| RAWINPUTDEVICE {
                usUsagePage: 1,
                usUsage: *usage,
                dwFlags: RIDEV_REMOVE,
                hwndTarget: null_mut(),
            })
            .collect();
        if RegisterRawInputDevices(removal.as_ptr(), 3, size_of::<RAWINPUTDEVICE>() as u32) == 0 {
            errors.push(win_error("Remove background controller capture"));
        }
    }
    if DestroyWindow(window) == 0 {
        errors.push(win_error("Destroy controller input window"));
    }
    if UnregisterClassW(class.as_ptr(), instance) == 0 {
        errors.push(win_error("Unregister controller input window"));
    }
    (!errors.is_empty()).then(|| errors.join("; "))
}

fn append_cleanup(primary: String, cleanup: Option<String>) -> String {
    cleanup.map_or(primary.clone(), |error| {
        format!("{primary}; cleanup failed: {error}")
    })
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
