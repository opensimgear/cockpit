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
    mod receiver_focus {
        include!("support/receiver_focus.rs");
    }
    mod receiver_cursor {
        include!("support/receiver_cursor.rs");
    }
    pub(super) use self::receiver_focus::foreground;
    use windows_sys::Win32::{
        Foundation::*,
        System::{
            LibraryLoader::GetModuleHandleW,
            Threading::GetCurrentProcessId,
        },
        UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    };
    pub(super) static OBSERVATIONS: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());
    use self::receiver_focus::{FocusChange, RECEIVER_CLASS, ALLOW_RECEIVER_FOREGROUND};
    static RECEIVE_ONLY: AtomicBool = AtomicBool::new(false);
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
            return receiver_focus::allow_receiver_foreground(window, wparam as HWND, pid) as LRESULT;
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
                observations.push(serde_json::json!({"atUs":now_us().to_string(),"message":message,"wparam":wparam.to_string(),"lparam":lparam.to_string(),"scanCode":((lparam as u64)>>16)&255,"extended":((lparam as u64)>>24)&1,"injectedMarker":GetMessageExtraInfo() as usize==0x4f534744,"captureWindow":(GetCapture() as usize).to_string(),"foregroundWindow":(GetForegroundWindow() as usize).to_string(),"clientCursor":receiver_cursor::client_cursor(window).ok()}));
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
        let keyboard_receiver_only =
            args.first().map(String::as_str) == Some("--keyboard-receiver-only");
        let receiver_only = args.first().map(String::as_str) == Some("--receiver-only");
        if args.len() != 2
            || !matches!(
                args[0].as_str(),
                "--accept-native-input"
                    | "--accept-worker-input"
                    | "--receiver-only"
                    | "--keyboard-receiver-only"
                    | "--osg-desktop-focus-target"
            )
        {
            return Err("Usage: native_receiver --accept-native-input <evidence.json> (opens a foreground receiver and submits bounded test input).".into());
        }
        RECEIVE_ONLY.store(receiver_only || keyboard_receiver_only, Ordering::Relaxed);
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
            if keyboard_receiver_only {
                // This mode isolates keyboard reception: use ordinary Windows
                // activation only, with no input-queue attachment or pointer move.
                SetForegroundWindow(window);
            } else {
                receiver_focus::foreground(window);
                receiver_cursor::center_cursor(window)?;
            }
            window
        };
        wait(Duration::from_millis(150));
        if receiver_only || keyboard_receiver_only {
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
                    if keyboard_receiver_only && request == "focus-and-center" {
                        return Err(
                            "Keyboard-only receiver does not accept focus-and-center requests."
                                .into(),
                        );
                    }
                    unsafe {
                        last_focus = receiver_focus::receiver_foreground(window)?;
                        // Moving the pointer during a held-button focus switch
                        // changes OS mouse routing. Center only when the caller
                        // explicitly requests it after verifying no native holds.
                        if request == "focus-and-center" {
                            receiver_cursor::center_cursor(window)?;
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
                    "mouseButtonState":unsafe {receiver_cursor::mouse_button_state(window, &scope, mouse_state_sequence)},
                    "clientCursor":unsafe {receiver_cursor::client_cursor(window)?}});
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
