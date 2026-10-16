use serde::Serialize;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub struct Lifecycle(AtomicU64);
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleSnapshot {
    pub invalidation_epoch: u64,
}

impl Lifecycle {
    pub fn snapshot(&self) -> LifecycleSnapshot {
        LifecycleSnapshot {
            invalidation_epoch: self.0.load(Ordering::SeqCst),
        }
    }
    pub fn invalidate(&self) -> LifecycleSnapshot {
        self.0.fetch_add(1, Ordering::SeqCst);
        self.snapshot()
    }
}

pub fn input_command(request: &Value) -> bool {
    matches!(
        request["type"].as_str(),
        Some("input" | "arm" | "keepalive")
    )
}

pub fn reason(event: &tauri::WindowEvent) -> Option<&'static str> {
    match event {
        tauri::WindowEvent::Focused(false) => {
            Some("Native window lost focus; Connect and Start explicitly")
        }
        tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed => {
            Some("Native window closed; Connect and Start explicitly")
        }
        tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. } => {
            Some("Native layout changed; Connect and Start explicitly")
        }
        #[cfg(mobile)]
        tauri::WindowEvent::Suspended | tauri::WindowEvent::Resumed => {
            Some("Native application activity changed; Connect and Start explicitly")
        }
        _ => None,
    }
}

#[cfg(windows)]
pub fn desktop_input_available() -> bool {
    use windows_sys::Win32::System::StationsAndDesktops::{
        CloseDesktop, GetUserObjectInformationW, OpenInputDesktop, DESKTOP_READOBJECTS, UOI_NAME,
    };
    // Read-only query: never switch desktops or send input. Secure/locked desktops
    // are not an input destination for this application, even if JS is still ticking.
    unsafe {
        let desktop = OpenInputDesktop(0, 0, DESKTOP_READOBJECTS);
        if desktop.is_null() {
            return false;
        }
        let mut name = [0u16; 256];
        let mut needed = 0u32;
        let ok = GetUserObjectInformationW(
            desktop as _,
            UOI_NAME,
            name.as_mut_ptr() as _,
            std::mem::size_of_val(&name) as u32,
            &mut needed,
        );
        let _ = CloseDesktop(desktop);
        if ok == 0 || needed as usize > std::mem::size_of_val(&name) {
            return false;
        }
        String::from_utf16_lossy(
            &name[..name
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(name.len())],
        )
        .eq_ignore_ascii_case("default")
    }
}
#[cfg(not(windows))]
pub fn desktop_input_available() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifecycle_changes_never_rearm_or_reuse_an_epoch() {
        let lifecycle = Lifecycle::default();
        assert_eq!(lifecycle.snapshot().invalidation_epoch, 0);
        assert_eq!(lifecycle.invalidate().invalidation_epoch, 1);
        assert_eq!(lifecycle.invalidate().invalidation_epoch, 2);
    }
    #[test]
    fn only_input_authority_requires_foreground_admission() {
        for command in ["arm", "input", "keepalive"] {
            assert!(input_command(&serde_json::json!({"type":command})));
        }
        for command in [
            "status",
            "pair-start",
            "disconnect",
            "publication",
            "forget",
            "neutralize",
        ] {
            assert!(!input_command(&serde_json::json!({"type":command})));
        }
        assert!(!input_command(&serde_json::json!({})));
    }
    #[test]
    fn native_focus_and_layout_events_always_disarm() {
        assert!(reason(&tauri::WindowEvent::Focused(false)).is_some());
        assert!(reason(&tauri::WindowEvent::Destroyed).is_some());
        assert!(
            reason(&tauri::WindowEvent::Resized(tauri::PhysicalSize::new(
                900, 600
            )))
            .is_some()
        );
        assert!(reason(&tauri::WindowEvent::Focused(true)).is_none());
    }
}
