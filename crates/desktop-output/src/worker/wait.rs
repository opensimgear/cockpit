//! Queue notifications share a wait with an owned one-shot native timer. No
//! process-wide timer-resolution change or polling cadence is introduced.
use std::{sync::mpsc::{Receiver, RecvTimeoutError}, time::Duration};

#[cfg(windows)]
mod native {
    use super::*;
    use std::{ptr, sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Instant};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_FAILED, WAIT_OBJECT_0},
        System::Threading::{CreateEventW, CreateWaitableTimerExW, ResetEvent, SetEvent,
            SetWaitableTimerEx, WaitForMultipleObjects, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
            INFINITE, TIMER_ALL_ACCESS},
    };

    struct Handle(HANDLE);
    // SAFETY: these private handles refer only to thread-agile kernel events or
    // timers. Arc ownership keeps them open throughout every concurrent call.
    unsafe impl Send for Handle {}
    unsafe impl Sync for Handle {}
    impl Drop for Handle {
        fn drop(&mut self) { unsafe { CloseHandle(self.0); } }
    }
    fn failure(operation: &str) -> String {
        format!("Desktop native wait {operation}: {}", std::io::Error::last_os_error())
    }
    struct Event {
        handle: Handle,
        pending: AtomicBool,
        failed: AtomicBool,
    }
    #[derive(Clone)]
    pub(crate) struct Wake(Arc<Event>);
    impl Wake {
        pub(crate) fn notify(&self) {
            self.0.pending.store(true, Ordering::Release);
            // SAFETY: Arc retains this event until every producer has stopped.
            if unsafe { SetEvent(self.0.handle.0) } == 0 {
                self.0.failed.store(true, Ordering::Release);
            }
        }
        fn check(&self) -> Result<(), String> {
            if self.0.failed.load(Ordering::Acquire) {
                Err("Desktop native queue notification failed.".into())
            } else { Ok(()) }
        }
    }
    pub(crate) struct NativeWait { event: Wake, timer: Handle }
    impl NativeWait {
        pub(crate) fn new() -> Result<Self, String> {
            // SAFETY: unnamed non-inheritable auto-reset event, no borrowed data.
            let event = unsafe { CreateEventW(ptr::null(), 0, 0, ptr::null()) };
            if event.is_null() { return Err(failure("CreateEventW")); }
            let event = Wake(Arc::new(Event { handle: Handle(event),
                pending: AtomicBool::new(false), failed: AtomicBool::new(false) }));
            // Explicit failure keeps unsupported hosts from silently using a
            // coarse timer. This flag is supported since Windows 10 1803.
            let timer = unsafe { CreateWaitableTimerExW(ptr::null(), ptr::null(),
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS) };
            if timer.is_null() { return Err(failure("CreateWaitableTimerExW")); }
            Ok(Self { event, timer: Handle(timer) })
        }
        pub(crate) fn waker(&self) -> Wake { self.event.clone() }
        pub(crate) fn receive<T>(&mut self, receiver: &Receiver<T>, duration: Duration)
            -> Result<Result<T, RecvTimeoutError>, String> {
            self.event.check()?;
            if duration.is_zero() { return Ok(Err(RecvTimeoutError::Timeout)); }
            let deadline = Instant::now().checked_add(duration).ok_or("Desktop native wait deadline overflow.")?;
            // Reset BEFORE checking the queue: a producer sends before SetEvent,
            // so a send racing this reset is either read now or wakes the wait.
            if unsafe { ResetEvent(self.event.0.handle.0) } == 0 { return Err(failure("ResetEvent")); }
            let notified = self.event.0.pending.swap(false, Ordering::AcqRel);
            match receiver.try_recv() {
                Ok(value) => return Ok(Ok(value)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return Ok(Err(RecvTimeoutError::Disconnected)),
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
            self.event.check()?;
            // Preserve a failure-only notification that preceded ResetEvent.
            if notified { return Ok(Err(RecvTimeoutError::Timeout)); }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() { return Ok(Err(RecvTimeoutError::Timeout)); }
            let due = relative_due(remaining);
            // Re-arming resets the timer's signal. No APC or global timer state.
            if unsafe { SetWaitableTimerEx(self.timer.0, &due, 0, None, ptr::null(), ptr::null(), 0) } == 0 {
                return Err(failure("SetWaitableTimerEx"));
            }
            let handles = [self.event.0.handle.0, self.timer.0];
            let result = unsafe { WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 0, INFINITE) };
            self.event.check()?;
            if result != WAIT_OBJECT_0 && result != WAIT_OBJECT_0 + 1 {
                return Err(if result == WAIT_FAILED { failure("WaitForMultipleObjects") }
                    else { format!("Desktop native wait returned unexpected status {result}.") });
            }
            self.event.0.pending.swap(false, Ordering::AcqRel);
            Ok(match receiver.try_recv() {
                Ok(value) => Ok(value),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(RecvTimeoutError::Disconnected),
                Err(std::sync::mpsc::TryRecvError::Empty) => Err(RecvTimeoutError::Timeout),
            })
        }
    }
    fn relative_due(remaining: Duration) -> i64 {
        // Windows relative due times are negative 100-nanosecond intervals.
        -(remaining.as_nanos().div_ceil(100).clamp(1, i64::MAX as u128) as i64)
    }

    #[cfg(test)]
    mod tests { include!("wait_tests.rs"); }
}

#[cfg(not(windows))]
mod native {
    use super::*;
    #[derive(Clone)]
    pub(crate) struct Wake;
    impl Wake { pub(crate) fn notify(&self) {} }
    pub(crate) struct NativeWait;
    impl NativeWait {
        pub(crate) fn new() -> Result<Self, String> { Ok(Self) }
        pub(crate) fn waker(&self) -> Wake { Wake }
        pub(crate) fn receive<T>(&mut self, receiver: &Receiver<T>, duration: Duration)
            -> Result<Result<T, RecvTimeoutError>, String> { Ok(receiver.recv_timeout(duration)) }
    }
}
pub(super) use native::NativeWait;
