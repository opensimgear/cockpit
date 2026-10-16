use serde::Serialize;
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
