use super::{state::Observation, Shared};
use std::{
    cell::RefCell,
    collections::VecDeque,
    ptr,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::{
        LibraryLoader::GetModuleHandleW,
        RemoteDesktop::*,
        Threading::{GetCurrentProcess, GetCurrentThreadId},
    },
    UI::{
        Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK},
        WindowsAndMessaging::*,
    },
};

const DRAIN: u32 = WM_APP + 91;
const RAW_LIMIT: usize = 256;
// Documented WM_WTSSESSION_CHANGE reason; absent from windows-sys 0.61.2.
const WTS_SESSION_DESKTOP_READY: u32 = 0x0f;
#[derive(Clone, Copy)]
enum Raw {
    Foreground(usize),
    DesktopSwitch,
    Session(usize),
}
#[derive(Default)]
struct Inbox {
    hwnd: usize,
    queue: VecDeque<Raw>,
    failure: Option<String>,
}
thread_local! { static INBOX: RefCell<Inbox> = RefCell::new(Inbox::default()); }
fn failure(operation: &str) -> String {
    format!(
        "Desktop scope {operation}: {}",
        std::io::Error::last_os_error()
    )
}
fn enqueue(raw: Raw) {
    INBOX.with(|slot| {
        let mut inbox = slot.borrow_mut();
        if inbox.failure.is_some() {
            return;
        }
        if inbox.queue.len() >= RAW_LIMIT {
            inbox.failure = Some("Desktop scope notification queue overflow.".into());
        } else {
            inbox.queue.push_back(raw);
        }
        if inbox.hwnd != 0 && unsafe { PostMessageW(inbox.hwnd as HWND, DRAIN, 0, 0) } == 0 {
            inbox.failure = Some(failure("notification wake failed"));
            // Ensure the owner leaves its wait even if posting to the HWND failed.
            unsafe {
                PostQuitMessage(1);
            }
        }
    });
}
unsafe extern "system" fn event(
    _: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    // No inspection, message pumping, subscriber callbacks, or cross-process
    // calls here. Preserve the original HWND before any later event can run.
    match event {
        EVENT_SYSTEM_FOREGROUND => enqueue(Raw::Foreground(hwnd as usize)),
        EVENT_SYSTEM_DESKTOPSWITCH => enqueue(Raw::DesktopSwitch),
        _ => {}
    }
}
unsafe extern "system" fn window(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_WTSSESSION_CHANGE => {
            enqueue(Raw::Session(wparam));
            0
        }
        DRAIN => 0,
        WM_CLOSE => {
            unsafe {
                PostQuitMessage(0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

pub(super) struct Owner {
    pub(super) shared: Arc<Shared>,
    hwnd: usize,
    thread_id: u32,
    completion: Mutex<mpsc::Receiver<()>>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
}
impl Owner {
    pub(super) fn open(shared: Arc<Shared>, deadline: Instant) -> Result<Arc<Self>, String> {
        let observer = shared.clone();
        let (ready, initialization) = mpsc::sync_channel(1);
        let (finished, completion) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("desktop-scope-events".into())
            .spawn(move || {
                let exited = super::lifetime::Exit::enter(observer.clone());
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(&observer, &ready)
                }));
                match result {
                    Ok(Ok(())) => {
                        if !observer.closing() && observer.terminal().is_none() {
                            observer.fail(
                                "Desktop scope notification thread stopped unexpectedly.".into(),
                            );
                        }
                    }
                    Ok(Err(error)) => {
                        observer.fail(error.clone());
                        let _ = ready.try_send(Err(error));
                    }
                    Err(_) => {
                        let error = "Desktop scope notification thread panicked.".to_string();
                        observer.fail(error.clone());
                        let _ = ready.try_send(Err(error));
                    }
                }
                // run() and its unwind have dropped all Resources before this
                // signal. Terminal callbacks also finish before a replacement.
                drop(exited);
                let _ = finished.send(());
            })
            .map_err(|error| { shared.lifetime.finish(); error.to_string() })?;
        let (hwnd, thread_id) = match initialization.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(result) => result?,
            Err(_) => {
                // The registry already tracks Shared weakly. A slow initializer
                // cannot overlap a second observer after this bounded failure.
                shared.close();
                return Err("Desktop scope notification initialization did not complete.".into());
            }
        };
        Ok(Arc::new(Self {
            shared,
            hwnd,
            thread_id,
            completion: Mutex::new(completion),
            thread: Mutex::new(Some(thread)),
        }))
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.shared.close();
        // The registry mutex is not held here. A wake callback may retire its
        // own last lease, so joining the current native owner is forbidden.
        let current = unsafe { GetCurrentThreadId() };
        let posted = self.shared.lifetime.with_live_resources(|| {
            if unsafe { PostMessageW(self.hwnd as HWND, WM_CLOSE, 0, 0) } == 0 {
                unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0); }
            }
        });
        if !posted || current == self.thread_id {
            return;
        }
        if self
            .completion
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .recv_timeout(Duration::from_millis(100))
            .is_ok()
        {
            if let Some(thread) = self
                .thread
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
            {
                let _ = thread.join();
            }
        }
        // On a bounded-stop timeout, detach. The thread retains Shared and all
        // native resources until it can perform same-thread cleanup.
    }
}

struct Resources<'a> {
    lifetime: Option<&'a super::lifetime::Lifetime>,
    class: Vec<u16>,
    registered: bool,
    instance: windows_sys::Win32::Foundation::HINSTANCE,
    hwnd: HWND,
    foreground: HWINEVENTHOOK,
    desktop: HWINEVENTHOOK,
    wts: bool,
}
impl<'a> Resources<'a> {
    fn open(lifetime: Option<&'a super::lifetime::Lifetime>) -> Result<Self, String> {
        let class = format!("OpenSimGear.ScopeEvents.{}", unsafe {
            GetCurrentThreadId()
        })
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        let mut resources = Self {
            lifetime,
            class,
            registered: false,
            instance,
            hwnd: ptr::null_mut(),
            foreground: ptr::null_mut(),
            desktop: ptr::null_mut(),
            wts: false,
        };
        let definition = WNDCLASSW {
            lpfnWndProc: Some(window),
            hInstance: instance,
            lpszClassName: resources.class.as_ptr(),
            ..unsafe { std::mem::zeroed() }
        };
        if unsafe { RegisterClassW(&definition) } == 0 {
            return Err(failure("RegisterClassW"));
        }
        resources.registered = true;
        resources.hwnd = unsafe {
            CreateWindowExW(
                0,
                resources.class.as_ptr(),
                ptr::null(),
                0,
                0,
                0,
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                ptr::null(),
            )
        };
        if resources.hwnd.is_null() {
            return Err(failure("CreateWindowExW"));
        }
        INBOX.with(|slot| slot.borrow_mut().hwnd = resources.hwnd as usize);
        // Do not skip our own process: focusing Cockpit must revoke app scope.
        resources.foreground = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                ptr::null_mut(),
                Some(event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if resources.foreground.is_null() {
            return Err(failure("SetWinEventHook foreground"));
        }
        resources.desktop = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_DESKTOPSWITCH,
                EVENT_SYSTEM_DESKTOPSWITCH,
                ptr::null_mut(),
                Some(event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if resources.desktop.is_null() {
            return Err(failure("SetWinEventHook desktop"));
        }
        if unsafe { WTSRegisterSessionNotification(resources.hwnd, NOTIFY_FOR_THIS_SESSION) } == 0 {
            return Err(failure("WTSRegisterSessionNotification"));
        }
        resources.wts = true;
        Ok(resources)
    }
    fn cleanup(&mut self) -> Result<(), String> {
        // Fence owner stop posts before any native identifier can be reused,
        // including partial initialization and unwinding through Drop.
        if let Some(lifetime) = self.lifetime { lifetime.begin_teardown(); }
        let mut error = None;
        if self.wts {
            if unsafe { WTSUnRegisterSessionNotification(self.hwnd) } == 0 {
                error = Some(failure("WTSUnRegisterSessionNotification"));
            }
            self.wts = false;
        }
        for hook in [&mut self.foreground, &mut self.desktop] {
            if !hook.is_null() {
                if unsafe { UnhookWinEvent(*hook) } == 0 {
                    error.get_or_insert_with(|| failure("UnhookWinEvent"));
                }
                *hook = ptr::null_mut();
            }
        }
        if !self.hwnd.is_null() {
            if unsafe { DestroyWindow(self.hwnd) } == 0 {
                error.get_or_insert_with(|| failure("DestroyWindow"));
            }
            self.hwnd = ptr::null_mut();
        }
        INBOX.with(|slot| *slot.borrow_mut() = Inbox::default());
        if self.registered {
            if unsafe { UnregisterClassW(self.class.as_ptr(), self.instance) } == 0 {
                error.get_or_insert_with(|| failure("UnregisterClassW"));
            }
            self.registered = false;
        }
        error.map_or(Ok(()), Err)
    }
}
impl Drop for Resources<'_> {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn session_active() -> Result<bool, String> {
    let mut buffer = ptr::null_mut();
    let mut bytes = 0;
    if unsafe {
        WTSQuerySessionInformationW(
            ptr::null_mut(),
            WTS_CURRENT_SESSION,
            WTSConnectState,
            &mut buffer,
            &mut bytes,
        )
    } == 0
    {
        return Err(failure("WTSQuerySessionInformationW"));
    }
    let value = if buffer.is_null() || bytes < std::mem::size_of::<WTS_CONNECTSTATE_CLASS>() as u32
    {
        Err("Desktop scope session state has invalid bounds.".into())
    } else {
        Ok(unsafe { *buffer.cast::<WTS_CONNECTSTATE_CLASS>() } == WTSActive)
    };
    unsafe {
        WTSFreeMemory(buffer.cast());
    }
    value
}
fn inspect(hwnd: HWND, active: bool) -> Observation {
    let observed = (|| {
        if !active || !super::super::interactive_desktop()? {
            return Ok(None);
        }
        let Some(current) = super::super::foreground_identity_for(hwnd)? else {
            return Ok(None);
        };
        let handle = super::super::open(current.pid)?;
        if super::super::integrity(handle.0)?
            > super::super::integrity(unsafe { GetCurrentProcess() })?
        {
            return Err(
                "Foreground application has higher integrity; run the target unelevated.".into(),
            );
        }
        Ok(Some(current))
    })();
    match observed {
        Ok(foreground) => Observation {
            foreground,
            error: None,
        },
        Err(error) => Observation::failed(error),
    }
}
fn fresh(active: &mut bool) -> Observation {
    match session_active() {
        Ok(value) => {
            *active = value;
            inspect(unsafe { GetForegroundWindow() }, value)
        }
        Err(error) => {
            *active = false;
            Observation::failed(error)
        }
    }
}
fn drain(shared: &Shared, active: &mut bool) -> Result<(), String> {
    loop {
        let raw = INBOX.with(|slot| {
            let mut inbox = slot.borrow_mut();
            if let Some(error) = &inbox.failure {
                return Err(error.clone());
            }
            Ok(inbox.queue.pop_front())
        })?;
        let Some(raw) = raw else { return Ok(()) };
        match raw {
            Raw::Foreground(hwnd) => shared.observe(inspect(hwnd as HWND, *active), false),
            Raw::DesktopSwitch => shared.observe(fresh(active), true),
            Raw::Session(reason) => match reason as u32 {
                WTS_SESSION_LOCK
                | WTS_SESSION_LOGOFF
                | WTS_CONSOLE_DISCONNECT
                | WTS_REMOTE_DISCONNECT => {
                    *active = false;
                    shared.observe(Observation::unavailable(), true);
                }
                WTS_SESSION_UNLOCK
                | WTS_SESSION_LOGON
                | WTS_CONSOLE_CONNECT
                | WTS_REMOTE_CONNECT
                | WTS_SESSION_DESKTOP_READY => shared.observe(fresh(active), false),
                _ => {}
            },
        }
        if shared.terminal().is_some() || shared.closing() {
            return Ok(());
        }
    }
}
fn run(
    shared: &Shared,
    ready: &mpsc::SyncSender<Result<(usize, u32), String>>,
) -> Result<(), String> {
    let mut resources = Resources::open(Some(&shared.lifetime))?;
    // All three registrations precede the first snapshot. Old queued callback
    // records remain intact; none can be discarded by this current observation.
    let mut active = false;
    shared.observe(fresh(&mut active), false);
    drain(shared, &mut active)?;
    ready
        .send(Ok((resources.hwnd as usize, unsafe {
            GetCurrentThreadId()
        })))
        .map_err(|_| "Desktop scope initialization receiver retired.".to_string())?;
    let result = (|| {
        loop {
            let mut message: MSG = unsafe { std::mem::zeroed() };
            let received = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
            if received == -1 {
                return Err(failure("GetMessageW"));
            }
            if received == 0 {
                break;
            }
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            drain(shared, &mut active)?;
            if shared.terminal().is_some() || shared.closing() {
                break;
            }
        }
        // Overflow/wake failures can terminate GetMessage through WM_QUIT.
        drain(shared, &mut active)
    })();
    result.and(resources.cleanup())
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;
