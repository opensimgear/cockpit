//! Bounded desktop controls use the same native admission and profile Stop fence.
use super::*;
use crate::{profile_desktop_settings as settings, profile_session_desktop as desktop};
use osg_desktop_output::audit;

fn require_audit_checkpoint_idle(status: &crate::profile_session::Status) -> Result<(), String> {
    if status.state != crate::profile_session::State::Stopped || status.owner.is_some() {
        return Err("Stop the profile and finish ownership cleanup before checkpointing the desktop audit.".into());
    }
    Ok(())
}

impl GatewayManager {
    pub(super) fn desktop_request(&self, request: &Value) -> Result<Value, String> {
        let command = request["command"].as_str().ok_or("Choose a desktop output operation.")?;
        let fields: &[&str] = match command {
            "profile_desktop_configure" => &["enabled", "scope"],
            "profile_desktop_status" | "profile_desktop_capture_application" | "profile_desktop_stop" | "profile_desktop_recover" | "profile_desktop_audit_checkpoint" => &[],
            "profile_desktop_audit_marker" => &["phase"],
            _ => return Err("Unknown desktop output operation.".into()),
        };
        if request.as_object().ok_or("Desktop requests must be objects.")?.keys().any(|field|field!="command"&&!fields.contains(&field.as_str())) {
            return Err("Unknown desktop output request field.".into());
        }
        if matches!(command, "profile_desktop_audit_checkpoint" | "profile_desktop_audit_marker") {
            let phase = if command == "profile_desktop_audit_marker" {
                Some(request["phase"].as_u64().filter(|phase| *phase <= 8)
                    .ok_or("Desktop audit phase must be an integer from 0 through 8.")?)
            } else { None };
            if !self.profile_native || !settings::qualified() {
                return Err("Desktop audit requires the native Windows Cockpit.".into());
            }
            if let Some(phase) = phase {
                if !audit::enabled() {
                    return Err("Desktop audit is not enabled by the process configuration.".into());
                }
                let id = audit::next_id().ok_or("Desktop audit cannot allocate a marker identity.")?;
                audit::record(audit::Kind::Marker, &[id, phase]);
                return Ok(json!({"auditMarker":{"id":id.to_string(),"phase":phase}}));
            }
            let coordinator = self.profile_session.lock().unwrap_or_else(|p|p.into_inner());
            require_audit_checkpoint_idle(coordinator.status())?;
            let report = audit::checkpoint(audit::Checkpoint::Stopped, std::time::Duration::from_secs(1));
            if !report.configured {
                return Err("Desktop audit has no process-configured directory and run nonce.".into());
            }
            return Ok(json!({"audit":report}));
        }
        let mut preferences = if self.profile_native { settings::Settings::native() } else { settings::Settings::open(None) };
        let mut captured_application = None;
        match command {
            "profile_desktop_configure" | "profile_desktop_capture_application" => {
                if !self.profile_native { return Err("Change desktop enablement and application scope in the native Cockpit.".into()); }
                // Reconfiguring a machine capability revokes an existing applied
                // run first. Saving settings never constitutes an Apply.
                if command == "profile_desktop_configure" {
                    let mut coordinator = self.profile_session.lock().unwrap_or_else(|p|p.into_inner());
                    let stopped = if coordinator.status().owner.is_some() { coordinator.stop() } else { Ok(()) };
                    drop(coordinator);
                    let pinned = self.automatic_stop_pin();
                    stopped.and(pinned)?;
                    let enabled = request["enabled"].as_bool().ok_or("Desktop enablement must be a boolean.")?;
                    let scope = serde_json::from_value(request["scope"].clone()).map_err(|e|format!("Invalid desktop scope: {e}"))?;
                    preferences.configure(enabled, scope)?;
                } else {
                    let identity = desktop::recent_application()?;
                    let epoch = identity.created.parse::<u64>().map_err(|_|"Invalid observed process creation time.")?;
                    captured_application = Some(preferences.capture_application(json!({"pid":identity.pid,"path":identity.executable,"epoch":epoch}))?);
                }
            }
            "profile_desktop_stop" => {
                let stopped = self.profile_session.lock().unwrap_or_else(|p|p.into_inner()).stop();
                let pinned = self.automatic_stop_pin();
                stopped.and(pinned)?;
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
        let applications = preferences.value.applications.iter().map(|(reference, identity)| {
            let name = identity["path"].as_str().unwrap_or("").rsplit(['\\', '/']).next().unwrap_or("Selected application");
            json!({"reference":reference,"name":name})
        }).collect::<Vec<_>>();
        Ok(json!({"enabled":preferences.value.enabled,"qualified":self.profile_native && settings::qualified(),
            "state":state,"reason":if settings::qualified() { "Keyboard and mouse output is available. Enable it in Settings and choose its application scope in the profile keyboard block." } else { settings::QUALIFICATION },
            "scope":preferences.value.scope,"application":preferences.value.application,"error":preferences.error,"runtime":runtime,
            "applications":applications,"capturedApplicationReference":captured_application,
            "recentApplication":desktop::recent_application().ok()}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile_session::{RunOwner, State, Status};

    #[test]
    fn audit_checkpoint_requires_exact_stopped_state_without_an_owner() {
        let mut status = Status {
            state: State::Stopped, applied_snapshot: None,
            owner: None, degradation: vec![], fault: None,
        };
        assert!(require_audit_checkpoint_idle(&status).is_ok());
        for state in [State::Running, State::Degraded, State::Faulted,
            State::RecoveryNeeded, State::Transitioning, State::AwaitingGame] {
            status.state = state;
            assert!(require_audit_checkpoint_idle(&status).is_err(), "{state:?}");
        }
        status.state = State::Stopped;
        status.owner = Some(RunOwner {
            session: "audit-test-owner".into(), generation: 1,
            profile_id: osg_profiles::DEFAULT_PROFILE_ID.into(),
            profile_revision: 1, game_id: None,
        });
        assert!(require_audit_checkpoint_idle(&status).is_err());
    }
}
