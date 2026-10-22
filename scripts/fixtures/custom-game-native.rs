//! Owned GUI process for native custom-game acceptance. No hardware or game
//! integration is simulated. The ordinary Cockpit watcher resolves its path.
#![cfg_attr(windows, windows_subsystem = "windows")]
#[cfg(windows)]
mod app {
    use std::{ffi::c_void, fs::OpenOptions, io::Write, ptr, sync::OnceLock};
    type Handle = *mut c_void;
    #[repr(C)]
    struct Class {
        style: u32,
        procedure: Option<unsafe extern "system" fn(Handle, u32, usize, isize) -> isize>,
        class_extra: i32,
        window_extra: i32,
        instance: Handle,
        icon: Handle,
        cursor: Handle,
        background: Handle,
        menu: *const u16,
        name: *const u16,
    }
    #[repr(C)]
    struct Message {
        window: Handle,
        message: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        point: [i32; 2],
        private: u32,
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn RegisterClassW(class: *const Class) -> u16;
        fn CreateWindowExW(
            ex: u32,
            class: *const u16,
            title: *const u16,
            style: u32,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            parent: Handle,
            menu: Handle,
            instance: Handle,
            param: Handle,
        ) -> Handle;
        fn DefWindowProcW(window: Handle, message: u32, wparam: usize, lparam: isize) -> isize;
        fn GetMessageW(message: *mut Message, window: Handle, min: u32, max: u32) -> i32;
        fn TranslateMessage(message: *const Message) -> i32;
        fn DispatchMessageW(message: *const Message) -> isize;
        fn PostQuitMessage(code: i32);
        fn SetTimer(window: Handle, id: usize, interval: u32, callback: Handle) -> usize;
        fn DestroyWindow(window: Handle) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> Handle;
    }
    static ROOT: OnceLock<std::path::PathBuf> = OnceLock::new();
    static STARTED: OnceLock<std::time::Instant> = OnceLock::new();
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }
    fn record(message: u32, key: usize) {
        if let Some(root) = ROOT.get() {
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(root.with_extension("events.jsonl"))
            {
                let _ = writeln!(file, "{{\"message\":{message},\"key\":{key}}}");
            }
        }
    }
    unsafe extern "system" fn procedure(
        window: Handle,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize {
        match message {
            0x100 | 0x101 | 0x104 | 0x105 | 0x201 | 0x202 => record(message, wparam),
            0x113 => {
                if ROOT
                    .get()
                    .is_some_and(|root| root.with_extension("stop").exists())
                    || STARTED
                        .get()
                        .is_some_and(|started| started.elapsed().as_secs() >= 180)
                {
                    unsafe {
                        DestroyWindow(window);
                    }
                }
            }
            2 => {
                unsafe { PostQuitMessage(0) };
                return 0;
            }
            _ => {}
        }
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }
    pub fn run() {
        let executable = std::env::current_exe().unwrap();
        ROOT.set(executable.clone()).unwrap();
        STARTED.set(std::time::Instant::now()).unwrap();
        let class = wide("OpenSimGearCustomGameAcceptance");
        let title = wide("OpenSimGear custom game acceptance");
        unsafe {
            let instance = GetModuleHandleW(ptr::null());
            let spec = Class {
                style: 0,
                procedure: Some(procedure),
                class_extra: 0,
                window_extra: 0,
                instance,
                icon: ptr::null_mut(),
                cursor: ptr::null_mut(),
                background: 6usize as Handle,
                menu: ptr::null(),
                name: class.as_ptr(),
            };
            RegisterClassW(&spec);
            let window = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                0x10cf0000,
                100,
                100,
                640,
                320,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                ptr::null_mut(),
            );
            if window.is_null() {
                std::process::exit(1);
            }
            std::fs::write(
                executable.with_extension("ready"),
                std::process::id().to_string(),
            )
            .unwrap();
            SetTimer(window, 1, 100, ptr::null_mut());
            let mut message: Message = std::mem::zeroed();
            while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
}
fn main() {
    #[cfg(windows)]
    app::run();
}
