use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{Emitter, Manager};

/// Lifecycle epoch invalidates local preview if JavaScript missed an event while suspended.
/// This app has no network session, controller lease, or native output to retain.
#[derive(Default)]
struct Lifecycle(AtomicU64);

impl Lifecycle {
    fn invalidate(&self) -> u64 {
        self.0.fetch_add(1, Ordering::SeqCst) + 1
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LifecycleSnapshot {
    invalidation_epoch: u64,
}

#[tauri::command]
fn companion_lifecycle(state: tauri::State<'_, Lifecycle>) -> LifecycleSnapshot {
    LifecycleSnapshot {
        invalidation_epoch: state.0.load(Ordering::SeqCst),
    }
}

fn invalidate(window: &tauri::Window) {
    let lifecycle = window.state::<Lifecycle>();
    let invalidation_epoch = lifecycle.invalidate();
    let _ = window.emit(
        "companion-lifecycle-invalidated",
        LifecycleSnapshot { invalidation_epoch },
    );
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Lifecycle::default())
        .invoke_handler(tauri::generate_handler![companion_lifecycle])
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::Focused(false) | tauri::WindowEvent::Destroyed => {
                invalidate(window)
            }
            #[cfg(mobile)]
            tauri::WindowEvent::Suspended | tauri::WindowEvent::Resumed => invalidate(window),
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error running OpenSimGear Companion");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_native_invalidation_advances_epoch_without_rearming_anything() {
        let lifecycle = Lifecycle::default();
        assert_eq!(lifecycle.0.load(Ordering::SeqCst), 0);
        assert_eq!(lifecycle.invalidate(), 1);
        assert_eq!(lifecycle.invalidate(), 2);
        assert_eq!(lifecycle.0.load(Ordering::SeqCst), 2);
    }
}
