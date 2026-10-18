//! Bounded explicit native API experiment; its observations are not game qualification.
#[cfg(windows)]
#[path = "support/worker_probe.rs"]
mod worker_probe;
#[cfg(windows)]
mod experiment {
    use osg_desktop_output::{
        protocol::{native_count, Adapter, NativeEvent, Scope},
        windows::{now_us, process_identity, Native},
    };
    use osg_mapping::desktop::MouseButton;
    use std::{
        ptr,
        sync::Mutex,
        thread,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::*,
        Graphics::Gdi::ClientToScreen,
        System::{
            LibraryLoader::GetModuleHandleW,
            Threading::{AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId},
        },
        UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    };
    pub(super) static OBSERVATIONS: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());
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
    /// Explicit acceptance-window focus, including launches from a background
    /// terminal. Scope is still independently verified before native submission.
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
    unsafe extern "system" fn receive(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
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
        ) {
            let mut observations = OBSERVATIONS.lock().unwrap();
            if observations.len() < 4096 {
                observations.push(serde_json::json!({"atUs":now_us().to_string(),"message":message,"wparam":wparam.to_string(),"lparam":lparam.to_string(),"scanCode":((lparam as u64)>>16)&255,"extended":((lparam as u64)>>24)&1,"injectedMarker":GetMessageExtraInfo() as usize==0x4f534744}));
            }
            return 0;
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
        let class = wide("OpenSimGearDesktopReceiver");
        let title = wide("OpenSimGear keyboard and mouse acceptance receiver");
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
            let mut rect = RECT::default();
            GetClientRect(window, &mut rect);
            let mut point = POINT {
                x: (rect.right - rect.left) / 2,
                y: (rect.bottom - rect.top) / 2,
            };
            ClientToScreen(window, &mut point);
            SetCursorPos(point.x, point.y);
            window
        };
        wait(Duration::from_millis(150));
        if args[0] == "--receiver-only" {
            use std::io::Write;
            let identity = process_identity(unsafe { GetCurrentProcessId() })?;
            println!(
                "{}",
                serde_json::json!({"identity":identity,"window":(window as usize).to_string()})
            );
            std::io::stdout().flush().map_err(|e| e.to_string())?;
            let output = std::path::Path::new(&args[1]);
            let focus = output.with_extension("focus");
            let stop = output.with_extension("stop");
            let deadline = Instant::now() + Duration::from_secs(300);
            while Instant::now() < deadline && !stop.exists() {
                if focus.exists() {
                    let _ = std::fs::remove_file(&focus);
                    unsafe {
                        foreground(window);
                    }
                }
                wait(Duration::from_millis(50));
                let value = serde_json::json!({"identity":identity,"observations":*OBSERVATIONS.lock().unwrap()});
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
