use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{Manager, WebviewUrl, WebviewWindow};

const LABEL: &str = "profile-activation";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ActivationProgress {
    operation_id: String,
    profile_name: String,
    title: String,
    phase: ActivationPhase,
    message: String,
    details: Vec<String>,
    can_cancel: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum ActivationPhase {
    Preparing,
    Activating,
    Cancelling,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActivationWindowSnapshot {
    progress: ActivationProgress,
    cancel_requested: bool,
}

#[derive(Default)]
pub(crate) struct ActivationWindowState(Mutex<Option<ActivationWindowSnapshot>>);

impl ActivationProgress {
    fn validate(&self) -> Result<(), String> {
        if self.operation_id.is_empty()
            || self.operation_id.len() > 128
            || !self.operation_id.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || self.profile_name.len() > 256
            || self.title.len() > 160
            || self.message.len() > 4096
            || self.details.len() > 32
            || self.details.iter().any(|line| line.len() > 4096)
        {
            return Err("Invalid profile activation progress.".into());
        }
        Ok(())
    }
}

impl ActivationWindowState {
    fn start(&self, progress: ActivationProgress) {
        *self.0.lock().unwrap() = Some(ActivationWindowSnapshot {
            progress,
            cancel_requested: false,
        });
    }

    fn snapshot(&self) -> Option<ActivationWindowSnapshot> {
        self.0.lock().unwrap().clone()
    }

    fn update(&self, progress: ActivationProgress, reset_cancel_request: bool) -> bool {
        let mut state = self.0.lock().unwrap();
        let Some(current) = state.as_mut().filter(|s| s.progress.operation_id == progress.operation_id) else {
            return false;
        };
        current.progress = progress;
        if reset_cancel_request {
            current.cancel_requested = false;
        }
        true
    }

    fn request_cancel(&self, operation_id: &str) -> bool {
        let mut state = self.0.lock().unwrap();
        let Some(current) = state.as_mut().filter(|s| s.progress.operation_id == operation_id && s.progress.can_cancel) else {
            return false;
        };
        current.cancel_requested = true;
        true
    }

    fn finish(&self, operation_id: &str) -> bool {
        let mut state = self.0.lock().unwrap();
        if state.as_ref().is_some_and(|s| s.progress.operation_id == operation_id) {
            *state = None;
            return true;
        }
        false
    }
}

fn require_window(window: &WebviewWindow, expected: &[&str]) -> Result<(), String> {
    if expected.contains(&window.label()) && super::local_document(&window.url().map_err(|e| e.to_string())?) {
        Ok(())
    } else {
        Err("This window cannot control profile activation progress.".into())
    }
}

#[tauri::command]
pub(crate) async fn profile_activation_window_open(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, ActivationWindowState>,
    progress: ActivationProgress,
) -> Result<(), String> {
    require_window(&window, &["main"])?;
    progress.validate()?;
    let operation_id = progress.operation_id.clone();
    state.start(progress);
    let result = (|| {
        if let Some(existing) = app.get_webview_window(LABEL) {
            existing.show().map_err(|e| e.to_string())?;
            existing.set_focus().map_err(|e| e.to_string())?;
            return Ok(());
        }
        // The URL is fixed in native code; no external or caller supplied page is loaded.
        // This is an owned, non-modal OS window, so the main Cockpit remains usable.
        let child = tauri::WebviewWindowBuilder::new(
            &app,
            LABEL,
            WebviewUrl::App("index.html?profile-activation-window".into()),
        )
        .title("Profile activation · OpenSimGear")
        .inner_size(520.0, 330.0)
        .min_inner_size(420.0, 270.0)
        .resizable(true)
        .maximizable(false)
        .minimizable(false)
        .skip_taskbar(true)
        .background_color(tauri::window::Color(20, 25, 31, 255))
        .parent(&window).map_err(|e| e.to_string())?
        .center()
        .build().map_err(|e| e.to_string())?;
        let handle = app.clone();
        child.on_window_event(move |event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state = handle.state::<ActivationWindowState>();
                if state.snapshot().is_some() {
                    // Closing the progress window cancels the exact operation displayed.
                    // Keep it visible until safe cleanup finishes in the main controller.
                    api.prevent_close();
                    // Let the displayed page submit its own operation ID. A close
                    // event queued while another activation replaces this one
                    // must not cancel an operation the user has not seen yet.
                    if let Some(child) = handle.get_webview_window(LABEL) {
                        let _ = child.eval("window.dispatchEvent(new Event('profile-activation-close'))");
                    }
                }
            }
        });
        Ok(())
    })();
    if result.is_err() {
        state.finish(&operation_id);
    }
    result
}

#[tauri::command]
pub(crate) fn profile_activation_window_update(
    window: WebviewWindow,
    state: tauri::State<'_, ActivationWindowState>,
    progress: ActivationProgress,
    reset_cancel_request: Option<bool>,
) -> Result<bool, String> {
    require_window(&window, &["main"])?;
    progress.validate()?;
    Ok(state.update(progress, reset_cancel_request.unwrap_or(false)))
}

#[tauri::command]
pub(crate) fn profile_activation_window_state(
    window: WebviewWindow,
    state: tauri::State<'_, ActivationWindowState>,
) -> Result<Option<ActivationWindowSnapshot>, String> {
    require_window(&window, &["main", LABEL])?;
    Ok(state.snapshot())
}

#[tauri::command]
pub(crate) fn profile_activation_window_cancel(
    window: WebviewWindow,
    state: tauri::State<'_, ActivationWindowState>,
    operation_id: String,
) -> Result<bool, String> {
    require_window(&window, &[LABEL])?;
    Ok(state.request_cancel(&operation_id))
}

#[tauri::command]
pub(crate) async fn profile_activation_window_close(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, ActivationWindowState>,
    operation_id: String,
) -> Result<bool, String> {
    require_window(&window, &["main"])?;
    if !state.finish(&operation_id) {
        return Ok(false);
    }
    if let Some(child) = app.get_webview_window(LABEL) {
        child.destroy().map_err(|e| e.to_string())?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(id: &str) -> ActivationProgress {
        ActivationProgress { operation_id: id.into(), profile_name: "Flight rig".into(), title: "Activating profile".into(), phase: ActivationPhase::Preparing, message: "Preparing companions".into(), details: vec![], can_cancel: true }
    }

    #[test]
    fn stale_window_actions_cannot_change_or_cancel_the_next_activation() {
        let state = ActivationWindowState::default();
        state.start(progress("old-operation"));
        assert!(state.request_cancel("old-operation"));
        state.start(progress("new-operation"));
        assert!(!state.update(progress("old-operation"), false));
        assert!(!state.request_cancel("old-operation"));
        assert!(!state.finish("old-operation"));
        assert!(!state.snapshot().unwrap().cancel_requested);
        assert!(state.request_cancel("new-operation"));
        assert!(state.snapshot().unwrap().cancel_requested);
        assert!(state.finish("new-operation"));
        assert!(state.snapshot().is_none());
    }

    #[test]
    fn progress_updates_preserve_cancel_intent_and_disable_repeated_cancel() {
        let state = ActivationWindowState::default();
        state.start(progress("operation"));
        assert!(state.request_cancel("operation"));
        let mut next = progress("operation");
        next.phase = ActivationPhase::Cancelling;
        next.can_cancel = false;
        assert!(state.update(next, false));
        assert!(state.snapshot().unwrap().cancel_requested);
        assert!(!state.request_cancel("operation"));
    }

    #[test]
    fn a_failed_cancel_request_can_be_retried_without_restarting_activation() {
        let state = ActivationWindowState::default();
        state.start(progress("operation"));
        assert!(state.request_cancel("operation"));
        assert!(!state.update(progress("old-operation"), true));
        assert!(state.snapshot().unwrap().cancel_requested);
        assert!(state.update(progress("operation"), true));
        assert!(!state.snapshot().unwrap().cancel_requested);
        assert!(state.request_cancel("operation"));
    }

    #[test]
    fn bounds_reject_untrusted_or_unbounded_progress() {
        assert!(progress("a-valid-operation_1").validate().is_ok());
        assert!(progress("../other?route=1").validate().is_err());
        let mut value = progress("operation");
        value.details = vec![String::new(); 33];
        assert!(value.validate().is_err());
    }
}
