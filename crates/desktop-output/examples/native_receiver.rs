//! Bounded explicit native API experiment; its observations are not game qualification.
#[cfg(windows)]
#[path = "support/worker_probe.rs"]
mod worker_probe;
#[cfg(windows)]
mod experiment {
    use osg_desktop_output::{
        protocol::{native_count, Adapter, NativeEvent, ProcessIdentity, Scope},
        windows::{eligible_scope, now_us, process_alive, process_identity, Native},
    };
    use osg_mapping::desktop::MouseButton;
    use std::{
        ptr,
        sync::{
            atomic::{AtomicBool, Ordering},
            Mutex,
        },
        thread,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::*,
        Graphics::Gdi::{ClientToScreen, ScreenToClient},
        System::{
            LibraryLoader::GetModuleHandleW,
            Threading::{AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId},
        },
        UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    };
    pub(super) static OBSERVATIONS: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());
    const RECEIVER_CLASS: &str = "OpenSimGearDesktopReceiver";
    const ALLOW_RECEIVER_FOREGROUND: u32 = WM_APP + 97;
    static RECEIVE_ONLY: AtomicBool = AtomicBool::new(false);
    struct FocusChange {
        method: &'static str,
        before: usize,
        after: usize,
        capture_before: usize,
        capture_after: usize,
    }
    struct Probe {
        native: Native,
        possible: Vec<NativeEvent>,
    }
    fn same_control(a: &NativeEvent, b: &NativeEvent) -> bool {
        match (a, b) {
            (NativeEvent::Key { usage: a, .. }, NativeEvent::Key { usage: b, .. }) => a == b,
            (
                NativeEvent::MouseButton { button: a, .. },
                NativeEvent::MouseButton { button: b, .. },
            ) => a == b,
            _ => false,
        }
    }
    impl Adapter for Probe {
        fn eligible(&mut self, scope: &Scope) -> Result<bool, String> {
            self.native.eligible(scope)
        }
        fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String> {
            for event in events {
                if matches!(
                    event,
                    NativeEvent::Key { down: true, .. }
                        | NativeEvent::MouseButton { down: true, .. }
                ) && !self.possible.iter().any(|e| same_control(e, event))
                {
                    self.possible.push(event.clone());
                }
            }
            let count = self.native.submit(events)?;
            if count == native_count(events) {
                for event in events {
                    if matches!(
                        event,
                        NativeEvent::Key { down: false, .. }
                            | NativeEvent::MouseButton { down: false, .. }
                    ) {
                        self.possible.retain(|e| !same_control(e, event));
                    }
                }
            }
            Ok(count)
        }
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            let releases: Vec<_> = self
                .possible
                .iter()
                .rev()
                .filter_map(|e| match e {
                    NativeEvent::Key { usage, .. } => Some(NativeEvent::Key {
                        usage: *usage,
                        down: false,
                        repeat: false,
                    }),
                    NativeEvent::MouseButton { button, .. } => Some(NativeEvent::MouseButton {
                        button: *button,
                        down: false,
                    }),
                    _ => None,
                })
                .collect();
            let _ = self.native.submit(&releases);
        }
    }
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }
    /// Pre-hold fallback for launches from a background terminal. Do not use
    /// this while the experiment holds input: AttachThreadInput resets key state.
    /// Scope is still independently verified before native submission.
    pub(super) unsafe fn foreground(window: HWND) {
        let thread = GetCurrentThreadId();
        let other = GetWindowThreadProcessId(GetForegroundWindow(), ptr::null_mut());
        let attached = other != 0 && other != thread && AttachThreadInput(thread, other, 1) != 0;
        ShowWindow(window, SW_RESTORE);
        SetWindowPos(
            window,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
        SetForegroundWindow(window);
        SetActiveWindow(window);
        SetFocus(window);
        if attached {
            AttachThreadInput(thread, other, 0);
        }
    }
    unsafe fn receiver_identity(window: HWND) -> Result<ProcessIdentity, String> {
        if window.is_null() || IsWindow(window) == 0 || GetAncestor(window, GA_ROOT) != window {
            return Err("Cooperative focus requires an existing top-level receiver window.".into());
        }
        let mut class = [0u16; 64];
        let length = GetClassNameW(window, class.as_mut_ptr(), class.len() as i32);
        if length <= 0 || String::from_utf16_lossy(&class[..length as usize]) != RECEIVER_CLASS {
            return Err("Cooperative focus requires the exact receiver window class.".into());
        }
        let mut pid = 0;
        if GetWindowThreadProcessId(window, &mut pid) == 0 || pid == 0 {
            return Err("Cannot identify the cooperative receiver process.".into());
        }
        let identity = process_identity(pid)?;
        let mut current_pid = 0;
        if !process_alive(&identity)?
            || GetWindowThreadProcessId(window, &mut current_pid) == 0
            || current_pid != pid
        {
            return Err("The cooperative receiver process changed or exited.".into());
        }
        Ok(identity)
    }
    unsafe fn allow_receiver_foreground(window: HWND, target: HWND, pid: u32) -> bool {
        if !RECEIVE_ONLY.load(Ordering::Relaxed)
            || GetForegroundWindow() != window
            || target == window
            || pid == 0
            || pid == ASFW_ANY
        {
            return false;
        }
        let (Ok(source), Ok(target)) = (receiver_identity(window), receiver_identity(target))
        else {
            return false;
        };
        source.pid == GetCurrentProcessId()
            && target.pid == pid
            && source.pid != target.pid
            && source.executable.eq_ignore_ascii_case(&target.executable)
            && AllowSetForegroundWindow(target.pid) != 0
    }
    /// The foreground test receiver grants only this exact receiver process a
    /// bounded cooperative handoff. No input queues are joined and no input is
    /// generated; the observed foreground HWND must still match after activation.
    unsafe fn receiver_foreground(window: HWND) -> Result<FocusChange, String> {
        let before = GetForegroundWindow();
        let capture_before = GetCapture() as usize;
        let target = receiver_identity(window)?;
        if target.pid != GetCurrentProcessId() {
            return Err("The focus target must belong to this receiver process.".into());
        }
        let method = if before == window {
            "already-foreground"
        } else if let Ok(source) = receiver_identity(before) {
            if source.pid == target.pid
                || !source.executable.eq_ignore_ascii_case(&target.executable)
            {
                return Err(
                    "Cooperative focus requires a separate receiver from the same executable."
                        .into(),
                );
            }
            let mut granted = 0usize;
            if SendMessageTimeoutW(
                before,
                ALLOW_RECEIVER_FOREGROUND,
                window as WPARAM,
                target.pid as LPARAM,
                SMTO_BLOCK | SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
                500,
                &mut granted,
            ) == 0
                || granted != 1
            {
                return Err(
                    "The foreground receiver did not grant the bounded focus handoff.".into(),
                );
            }
            if GetForegroundWindow() != before
                || receiver_identity(before)? != source
                || receiver_identity(window)? != target
            {
                return Err(
                    "Receiver identity or foreground changed during the focus handoff.".into(),
                );
            }
            "cooperative-receiver"
        } else {
            // Startup may leave an unrelated application foreground. Request
            // ordinary activation without joining its input queue; denial fails
            // the experiment instead of changing held-input state.
            "direct-no-attach"
        };
        if before != window {
            SetForegroundWindow(window);
        }
        if GetForegroundWindow() != window {
            return Err("The receiver did not acquire the actual foreground window.".into());
        }
        SetActiveWindow(window);
        SetFocus(window);
        let after = GetForegroundWindow();
        if after != window {
            return Err("The receiver lost foreground during activation.".into());
        }
        Ok(FocusChange {
            method,
            before: before as usize,
            after: after as usize,
            capture_before,
            capture_after: GetCapture() as usize,
        })
    }
    unsafe fn client_cursor(window: HWND) -> Result<serde_json::Value, String> {
        let mut rect = RECT::default();
        let mut point = POINT::default();
        if GetClientRect(window, &mut rect) == 0
            || GetCursorPos(&mut point) == 0
            || ScreenToClient(window, &mut point) == 0
        {
            return Err("Cannot observe the receive-only client cursor.".into());
        }
        Ok(
            serde_json::json!({"x":point.x,"y":point.y,"width":rect.right,"height":rect.bottom,
            "inside":point.x>=0&&point.y>=0&&point.x<rect.right&&point.y<rect.bottom}),
        )
    }
    /// A bounded observer of OS mouse state, not an input ownership inference or
    /// proof that any application received a mouse message. Zero is ambiguous
    /// outside our eligible foreground desktop, so expose no state there.
    unsafe fn mouse_button_state(window: HWND, scope: &Scope, sequence: u64) -> serde_json::Value {
        let swap_buttons = GetSystemMetrics(SM_SWAPBUTTON) != 0;
        let eligible_before = eligible_scope(scope).unwrap_or(false);
        let before = GetForegroundWindow();
        let down = if eligible_before && before == window {
            // Only the high bit is current state; the low transition bit is
            // shared with other processes and must not be used as evidence.
            Some(GetAsyncKeyState(VK_LBUTTON as i32) < 0)
        } else {
            None
        };
        let at_us = now_us();
        let eligible_after = eligible_scope(scope).unwrap_or(false);
        let after = GetForegroundWindow();
        let available = down.is_some()
            && eligible_after
            && after == window
            && (GetSystemMetrics(SM_SWAPBUTTON) != 0) == swap_buttons;
        serde_json::json!({"sequence":sequence.to_string(),"atUs":at_us.to_string(),
            "available":available,"leftButtonDown":if available {down} else {None},
            "swapButtons":swap_buttons,"foregroundBefore":(before as usize).to_string(),
            "foregroundAfter":(after as usize).to_string()})
    }
    unsafe fn center_cursor(window: HWND) -> Result<(), String> {
        let mut rect = RECT::default();
        if GetClientRect(window, &mut rect) == 0 || rect.right <= 0 || rect.bottom <= 0 {
            return Err("The receive-only window has no client area.".into());
        }
        let mut point = POINT {
            x: rect.right / 2,
            y: rect.bottom / 2,
        };
        if ClientToScreen(window, &mut point) == 0 || SetCursorPos(point.x, point.y) == 0 {
            return Err("Cannot center the cursor in the receive-only client area.".into());
        }
        Ok(())
    }
    unsafe extern "system" fn receive(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == ALLOW_RECEIVER_FOREGROUND {
            let Ok(pid) = u32::try_from(lparam) else {
                return 0;
            };
            return allow_receiver_foreground(window, wparam as HWND, pid) as LRESULT;
        }
        if matches!(
            message,
            WM_KEYDOWN
                | WM_KEYUP
                | WM_SYSKEYDOWN
                | WM_SYSKEYUP
                | WM_CHAR
                | WM_MOUSEMOVE
                | WM_LBUTTONDOWN
                | WM_LBUTTONUP
                | WM_RBUTTONDOWN
                | WM_RBUTTONUP
                | WM_MBUTTONDOWN
                | WM_MBUTTONUP
                | WM_XBUTTONDOWN
                | WM_XBUTTONUP
                | WM_MOUSEWHEEL
                | WM_MOUSEHWHEEL
                | WM_NCLBUTTONDOWN
                | WM_NCLBUTTONUP
        ) {
            let mut observations = OBSERVATIONS.lock().unwrap();
            if observations.len() < 4096 {
                observations.push(serde_json::json!({"atUs":now_us().to_string(),"message":message,"wparam":wparam.to_string(),"lparam":lparam.to_string(),"scanCode":((lparam as u64)>>16)&255,"extended":((lparam as u64)>>24)&1,"injectedMarker":GetMessageExtraInfo() as usize==0x4f534744,"captureWindow":(GetCapture() as usize).to_string(),"foregroundWindow":(GetForegroundWindow() as usize).to_string(),"clientCursor":client_cursor(window).ok()}));
            }
            // Observe non-client routing without suppressing its normal Windows
            // behavior. Client input retains the receiver's existing handling.
            if !matches!(message, WM_NCLBUTTONDOWN | WM_NCLBUTTONUP) {
                return 0;
            }
        }
        if message == WM_CLOSE {
            return 0;
        }
        DefWindowProcW(window, message, wparam, lparam)
    }
    unsafe fn pump() {
        let mut message = MSG::default();
        while PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    pub(super) fn wait(duration: Duration) {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            unsafe { pump() };
            thread::sleep(Duration::from_millis(1));
        }
    }
    fn key(usage: u16, down: bool) -> NativeEvent {
        NativeEvent::Key {
            usage,
            down,
            repeat: false,
        }
    }
    pub fn run() -> Result<(), String> {
        if let Some(code) = osg_desktop_output::entry::helper_main() {
            std::process::exit(code)
        }
        let args: Vec<_> = std::env::args().skip(1).collect();
        if args.len() == 2 && args[0] == "--accept-worker-lifecycle" {
            return super::worker_probe::lifecycle(&args[1]);
        }
        if args.first().map(String::as_str) == Some("--osg-desktop-test-parent") {
            return super::worker_probe::parent(&args);
        }
        let focus_target = args.first().map(String::as_str) == Some("--osg-desktop-focus-target");
        if args.len() != 2
            || !matches!(
                args[0].as_str(),
                "--accept-native-input"
                    | "--accept-worker-input"
                    | "--receiver-only"
                    | "--osg-desktop-focus-target"
            )
        {
            return Err("Usage: native_receiver --accept-native-input <evidence.json> (opens a foreground receiver and submits bounded test input).".into());
        }
        RECEIVE_ONLY.store(args[0] == "--receiver-only", Ordering::Relaxed);
        let class = wide(RECEIVER_CLASS);
        let title = wide("OpenSimGear keyboard and mouse acceptance receiver");
        let startup_foreground = unsafe { GetForegroundWindow() as usize };
        let startup_capture = unsafe { GetCapture() as usize };
        let window = unsafe {
            let module = GetModuleHandleW(ptr::null());
            let wc = WNDCLASSW {
                lpfnWndProc: Some(receive),
                hInstance: module,
                lpszClassName: class.as_ptr(),
                ..Default::default()
            };
            if RegisterClassW(&wc) == 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let window = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                100,
                100,
                800,
                500,
                ptr::null_mut(),
                ptr::null_mut(),
                module,
                ptr::null(),
            );
            if window.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            foreground(window);
            center_cursor(window)?;
            window
        };
        wait(Duration::from_millis(150));
        if args[0] == "--receiver-only" {
            use std::io::Write;
            let identity = process_identity(unsafe { GetCurrentProcessId() })?;
            let scope = Scope::Application {
                identity: identity.clone(),
            };
            println!(
                "{}",
                serde_json::json!({"identity":identity,"window":(window as usize).to_string()})
            );
            std::io::stdout().flush().map_err(|e| e.to_string())?;
            let output = std::path::Path::new(&args[1]);
            let focus = output.with_extension("focus");
            let stop = output.with_extension("stop");
            let deadline = Instant::now() + Duration::from_secs(300);
            let mut focus_requests = 0u32;
            let mut mouse_state_sequence = 0u64;
            let mut last_focus = FocusChange {
                method: "startup-fallback",
                before: startup_foreground,
                after: unsafe { GetForegroundWindow() as usize },
                capture_before: startup_capture,
                capture_after: unsafe { GetCapture() as usize },
            };
            while Instant::now() < deadline && !stop.exists() {
                if focus.exists() {
                    if std::fs::metadata(&focus)
                        .map_err(|error| error.to_string())?
                        .len()
                        > 64
                    {
                        return Err("Receive-only focus request exceeds its bound.".into());
                    }
                    let request =
                        std::fs::read_to_string(&focus).map_err(|error| error.to_string())?;
                    if request != "focus" && request != "focus-and-center" {
                        return Err("Unknown receive-only focus request.".into());
                    }
                    unsafe {
                        last_focus = receiver_foreground(window)?;
                        // Moving the pointer during a held-button focus switch
                        // changes OS mouse routing. Center only when the caller
                        // explicitly requests it after verifying no native holds.
                        if request == "focus-and-center" {
                            center_cursor(window)?;
                        }
                    }
                    focus_requests += 1;
                    std::fs::remove_file(&focus).map_err(|error| error.to_string())?;
                }
                wait(Duration::from_millis(50));
                mouse_state_sequence = mouse_state_sequence
                    .checked_add(1)
                    .ok_or("Receive-only mouse observation sequence exhausted.")?;
                let value = serde_json::json!({"identity":identity,"observations":*OBSERVATIONS.lock().unwrap(),
                    "focusRequests":focus_requests,"focusMethod":last_focus.method,
                    "focusBefore":last_focus.before.to_string(),"focusAfter":last_focus.after.to_string(),
                    "focusCaptureBefore":last_focus.capture_before.to_string(),"focusCaptureAfter":last_focus.capture_after.to_string(),
                    "foregroundWindow":unsafe {(GetForegroundWindow() as usize).to_string()},
                    "captureWindow":unsafe {(GetCapture() as usize).to_string()},
                    "mouseButtonState":unsafe {mouse_button_state(window, &scope, mouse_state_sequence)},
                    "clientCursor":unsafe {client_cursor(window)?}});
                std::fs::write(
                    output,
                    serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
            }
            let _ = std::fs::remove_file(&stop);
            unsafe {
                DestroyWindow(window);
            }
            return Ok(());
        }
        if focus_target {
            use std::io::Write;
            println!("{}", window as usize);
            std::io::stdout().flush().map_err(|e| e.to_string())?;
            wait(Duration::from_millis(1000));
            std::fs::write(
                &args[1],
                serde_json::to_vec(&*OBSERVATIONS.lock().unwrap()).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            unsafe {
                DestroyWindow(window);
            }
            return Ok(());
        }
        if args[0] == "--accept-worker-input" {
            let scope = Scope::Application {
                identity: process_identity(unsafe { GetCurrentProcessId() })?,
            };
            let result = super::worker_probe::run(window, scope, &args[1]);
            unsafe {
                DestroyWindow(window);
            }
            return result;
        }
        let mut native = Probe {
            native: Native::open()?,
            possible: vec![],
        };
        let scope = Scope::Application {
            identity: process_identity(unsafe { GetCurrentProcessId() })?,
        };
        let original_layout = unsafe { GetKeyboardLayout(0) };
        let mut layouts = Vec::new();
        let mut submissions = Vec::new();
        for (layout_name, layout_code) in [("US", "00000409"), ("German", "00000407")] {
            let code = wide(layout_code);
            let layout = unsafe { LoadKeyboardLayoutW(code.as_ptr(), KLF_NOTELLSHELL) };
            if layout.is_null() {
                layouts.push(serde_json::json!({"layout":layout_name,"available":false}));
                continue;
            }
            unsafe {
                ActivateKeyboardLayout(layout, 0);
            }
            let mut name = [0u16; 9];
            unsafe {
                GetKeyboardLayoutNameW(name.as_mut_ptr());
            }
            layouts.push(serde_json::json!({"layout":layout_name,"available":true,"actual":String::from_utf16_lossy(&name[..8])}));
            let mut cases = vec![
                ("physical-y", vec![key(0x1c, true)], vec![key(0x1c, false)]),
                (
                    "ctrl-g",
                    vec![key(0xe0, true), key(0x0a, true)],
                    vec![key(0x0a, false), key(0xe0, false)],
                ),
                (
                    "altgr-q",
                    vec![key(0xe6, true), key(0x14, true)],
                    vec![key(0x14, false), key(0xe6, false)],
                ),
                (
                    "keypad-enter",
                    vec![key(0x58, true)],
                    vec![key(0x58, false)],
                ),
                ("extended-up", vec![key(0x52, true)], vec![key(0x52, false)]),
                ("iso-key", vec![key(0x64, true)], vec![key(0x64, false)]),
            ];
            for button in [
                MouseButton::Left,
                MouseButton::Right,
                MouseButton::Middle,
                MouseButton::X1,
                MouseButton::X2,
            ] {
                cases.push((
                    "mouse-button",
                    vec![NativeEvent::MouseButton { button, down: true }],
                    vec![NativeEvent::MouseButton {
                        button,
                        down: false,
                    }],
                ));
            }
            cases.push((
                "both-wheel-directions",
                vec![NativeEvent::Wheel {
                    horizontal: 1,
                    vertical: 1,
                }],
                vec![NativeEvent::Wheel {
                    horizontal: -1,
                    vertical: -1,
                }],
            ));
            for (case, press, release) in cases {
                let observation_start = OBSERVATIONS.lock().unwrap().len();
                for (phase, events) in [("press", press), ("release", release)] {
                    if !native.eligible(&scope)? {
                        unsafe {
                            ActivateKeyboardLayout(original_layout, 0);
                            DestroyWindow(window);
                        }
                        return Err("Receiver lost foreground eligibility; experiment cancelled before further input.".into());
                    }
                    let started = Instant::now();
                    let count = native.submit(&events)?;
                    submissions.push(serde_json::json!({"layout":layout_name,"case":case,"phase":phase,"requested":native_count(&events),"returned":count,"submitElapsedUs":started.elapsed().as_micros().to_string(),"events":events,"observationStart":observation_start}));
                    if count != native_count(&events) {
                        return Err(
                            "Short native test submission; explicit cleanup inspection required."
                                .into(),
                        );
                    }
                    wait(Duration::from_millis(30));
                }
            }
            let observation_start = OBSERVATIONS.lock().unwrap().len();
            for tick in 0..32 {
                if !native.eligible(&scope)? {
                    return Err("Receiver lost foreground during sustained pointer test.".into());
                }
                let events = [NativeEvent::Pointer {
                    dx: if tick < 16 { 1 } else { -1 },
                    dy: 0,
                }];
                let count = native.submit(&events)?;
                submissions.push(serde_json::json!({"layout":layout_name,"case":"sustained-pointer-125hz","tick":tick,"requested":1,"returned":count,"observationStart":observation_start}));
                wait(Duration::from_millis(8));
            }
        }
        unsafe {
            ActivateKeyboardLayout(original_layout, 0);
        }
        wait(Duration::from_millis(50));
        let observations = OBSERVATIONS.lock().unwrap().clone();
        let evidence = serde_json::json!({"experiment":"J0 bounded native receiver","nativeSubmissionOnly":true,"gameQualified":false,"productionGate":osg_desktop_output::QUALIFIED,"higherIntegrityTarget":"not tested","physicalKeyboardOverlap":"not tested","layouts":layouts,"submissions":submissions,"receiverObservations":observations});
        std::fs::write(
            &args[1],
            serde_json::to_vec_pretty(&evidence).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        unsafe {
            DestroyWindow(window);
        }
        println!(
            "Recorded {} submissions and {} receiver observations to {}.",
            submissions.len(),
            observations.len(),
            args[1]
        );
        Ok(())
    }
}
fn main() {
    #[cfg(windows)]
    if let Err(error) = experiment::run() {
        eprintln!("{error}");
        std::process::exit(1)
    }
    #[cfg(not(windows))]
    {
        eprintln!("Native receiver requires Windows.");
        std::process::exit(1)
    }
}
