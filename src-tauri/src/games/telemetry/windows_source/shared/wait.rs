//! High-resolution, interruptible pacing for the shared-memory capture worker.
use std::{
    ptr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_FAILED, WAIT_OBJECT_0},
    System::Threading::{
        CreateEventW, CreateWaitableTimerExW, SetEvent, SetWaitableTimerEx, WaitForMultipleObjects,
        CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, INFINITE, TIMER_ALL_ACCESS,
    },
};

struct Handle(HANDLE);

// SAFETY: these private kernel event and timer handles are thread-agile. Their
// shared Arc ownership keeps them alive across wait/cancel calls.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns the handle and drops it exactly once.
        unsafe { CloseHandle(self.0) };
    }
}

struct Inner {
    cancelled: Handle,
    timer: Handle,
    cancel_requested: AtomicBool,
    cancel_failed: AtomicBool,
}

/// Owns a high-resolution one-shot timer and a persistent cancellation event.
#[derive(Clone)]
pub(in super::super) struct CaptureWait(Arc<Inner>);

impl CaptureWait {
    pub(in super::super) fn new() -> Result<Self, String> {
        // SAFETY: create an unnamed, non-inheritable manual-reset event.
        let cancelled = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
        if cancelled.is_null() {
            return Err(failure("CreateEventW"));
        }
        let cancelled = Handle(cancelled);
        // SAFETY: create an unnamed high-resolution timer with no callback.
        let timer = unsafe {
            CreateWaitableTimerExW(
                ptr::null(),
                ptr::null(),
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_ALL_ACCESS,
            )
        };
        if timer.is_null() {
            return Err(failure("CreateWaitableTimerExW"));
        }
        Ok(Self(Arc::new(Inner {
            cancelled,
            timer: Handle(timer),
            cancel_requested: AtomicBool::new(false),
            cancel_failed: AtomicBool::new(false),
        })))
    }

    /// Waits for the duration unless cancellation is requested.
    pub(in super::super) fn wait(&self, duration: Duration) -> Result<(), String> {
        if self.0.cancel_requested.load(Ordering::Acquire) {
            return Ok(());
        }
        if duration.is_zero() {
            return Ok(());
        }
        let due = -(duration.as_nanos().div_ceil(100).clamp(1, i64::MAX as u128) as i64);
        // SAFETY: this owned timer is used by the single capture worker; the
        // relative due time is a negative 100-nanosecond interval.
        if unsafe { SetWaitableTimerEx(self.0.timer.0, &due, 0, None, ptr::null(), ptr::null(), 0) }
            == 0
        {
            return Err(failure("SetWaitableTimerEx"));
        }
        let handles = [self.0.cancelled.0, self.0.timer.0];
        // SAFETY: both handles remain open through the Arc held by self.
        let result = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
        if self.0.cancel_failed.load(Ordering::Acquire) {
            return Err("Assetto capture cancellation signal failed.".into());
        }
        if result == WAIT_OBJECT_0 || result == WAIT_OBJECT_0 + 1 {
            return Ok(());
        }
        Err(if result == WAIT_FAILED {
            failure("WaitForMultipleObjects")
        } else {
            format!("Assetto capture wait returned unexpected status {result}.")
        })
    }

    /// Signals the persistent event so a parked worker can stop immediately.
    pub(super) fn cancel(&self) {
        self.0.cancel_requested.store(true, Ordering::Release);
        // SAFETY: the Arc keeps the event alive until this call returns.
        if unsafe { SetEvent(self.0.cancelled.0) } == 0 {
            self.0.cancel_failed.store(true, Ordering::Release);
        }
    }
}

#[cfg(test)]
#[path = "wait_tests.rs"]
mod tests;

fn failure(operation: &str) -> String {
    format!(
        "Assetto capture wait {operation}: {}",
        std::io::Error::last_os_error()
    )
}
