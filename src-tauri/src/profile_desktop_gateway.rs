//! Bounded desktop controls use the same native admission and profile pause fence.
use super::*;
use crate::{profile_desktop_settings as settings, profile_session_desktop as desktop};
impl GatewayManager {
    pub(super) fn desktop_request(&self, request: &Value) -> Result<Value, String> {
        let command = request["command"].as_str().ok_or("Choose a desktop output operation.")?;
        let fields: &[&str] = match command {
            "profile_desktop_configure" => &["enabled", "scope"],
            "profile_desktop_status" | "profile_desktop_capture_application" | "profile_desktop_stop" | "profile_desktop_recover" => &[],
            _ => return Err("Unknown desktop output operation.".into()),
        };
        if request.as_object().ok_or("Desktop requests must be objects.")?.keys().any(|field|field!="command"&&!fields.contains(&field.as_str())) {
            return Err("Unknown desktop output request field.".into());
        }
        let mut preferences = if self.profile_native { settings::Settings::native() } else { settings::Settings::open(None) };
        match command {
            "profile_desktop_configure" | "profile_desktop_capture_application" => {
                if !self.profile_native { return Err("Change desktop enablement and application scope in the native Cockpit.".into()); }
                // Reconfiguring a machine capability revokes an existing applied
                // run first. Saving settings never constitutes an Apply.
                let mut coordinator = self.profile_session.lock().unwrap_or_else(|p|p.into_inner());
                if coordinator.status().owner.is_some() { coordinator.pause(true)?; }
                if command == "profile_desktop_configure" {
                    let enabled = request["enabled"].as_bool().ok_or("Desktop enablement must be a boolean.")?;
                    let scope = serde_json::from_value(request["scope"].clone()).map_err(|e|format!("Invalid desktop scope: {e}"))?;
                    preferences.configure(enabled, scope)?;
                } else {
                    let identity = desktop::recent_application()?;
                    let epoch = identity.created.parse::<u64>().map_err(|_|"Invalid observed process creation time.")?;
                    preferences.application(json!({"pid":identity.pid,"path":identity.executable,"epoch":epoch}))?;
                }
            }
            "profile_desktop_stop" => {
                self.profile_session.lock().unwrap_or_else(|p|p.into_inner()).pause(true)?;
                self.automatic_stop_pin()?;
            }
            "profile_desktop_recover" => {
                self.profile_session.lock().unwrap_or_else(|p|p.into_inner()).recover()?;
                self.profile_automatic.retry_event();
            }
            _ => {}
        }
        let runtime = self.profile_session.lock().unwrap_or_else(|p|p.into_inner()).desktop_status();
        let state = if preferences.error.is_some() { "faulted" }
            else if !runtime.is_null() { runtime["state"].as_str().unwrap_or("stopped") }
            else { "stopped" };
        Ok(json!({"enabled":preferences.value.enabled,"qualified":self.profile_native && settings::qualified(),
            "state":state,"reason":if settings::qualified() { "Development acceptance mode; production qualification is incomplete." } else { settings::QUALIFICATION },
            "scope":preferences.value.scope,"application":preferences.value.application,"error":preferences.error,"runtime":runtime,
            "recentApplication":desktop::recent_application().ok()}))
    }
}
