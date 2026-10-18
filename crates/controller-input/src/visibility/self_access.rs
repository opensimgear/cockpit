//! Permanent, idempotent access for this capture executable only. This is not a
//! device-hiding session and never changes a device, mode, or another application.
use super::{journal::Backend, policy::{self, State}};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub status: &'static str,
    pub ready: bool,
    pub changed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    pub message: String,
}
impl Status {
    pub(super) fn new(status: &'static str, message: impl Into<String>) -> Self {
        Self { status, ready: false, changed: Some(false), executable: None, message: message.into() }
    }
}
impl Default for Status {
    fn default() -> Self {
        Self::new("not-requested", "Automatic HidHide self-access was not requested for this capture service.")
    }
}

pub(super) fn desired(before: &State, image: &str) -> Result<State, String> {
    before.validate()?;
    policy::executable(image)?;
    let mut after = before.clone();
    if before.inverse {
        after.applications.retain(|application| policy::key(application) != policy::key(image));
    } else if !policy::contains(&before.applications, image) {
        after.applications.push(image.into());
    }
    after.validate()?;
    Ok(after)
}

/// Defense at the native setter boundary: even an internally malformed request
/// can only apply the exact current-image membership delta.
pub(super) fn validate_write(before: &State, image: &str, function: u32, input: Option<&[u8]>) -> Result<(), String> {
    if function != 2049 { return Err("Automatic HidHide access may only change its own application entry.".into()); }
    let after = desired(before, image)?;
    if after == *before || super::wire::list(input.ok_or("Missing self-access application list.")?)? != after.applications {
        return Err("Automatic HidHide access rejected a change beyond the current executable.".into());
    }
    Ok(())
}

pub(super) fn reconcile(backend: &mut impl Backend, image: String, guard: impl FnOnce() -> Result<(), String>) -> Status {
    let mut status = Status::new("error", "HidHide self-access could not be verified.");
    status.executable = Some(image.clone());
    let before = match backend.read() { Ok(state) => state, Err(error) => { status.message = error; return status; } };
    let after = match desired(&before, &image) { Ok(state) => state, Err(error) => { status.message = error; return status; } };
    if before == after {
        status.status = "unchanged";
        status.ready = true;
        status.message = "HidHide already allows this Cockpit executable to access hidden devices.".into();
        return status;
    }
    if let Err(error) = guard() {
        status.status = "blocked";
        status.message = error;
        return status;
    }
    status.changed = None; // A failed write/readback may already have changed the driver.
    if let Err(error) = backend.change(&before, &after) { status.message = error; return status; }
    match backend.read() {
        Ok(actual) if actual == after => {
            status.status = "configured";
            status.ready = true;
            status.changed = Some(true);
            status.message = "HidHide access for this Cockpit executable was configured and verified.".into();
        }
        Ok(_) => status.message = "HidHide self-access readback changed; the current policy requires attention.".into(),
        Err(error) => status.message = format!("HidHide self-access was written but could not be verified: {error}"),
    }
    status
}

#[cfg(test)]
#[path = "self_access_tests.rs"]
mod tests;
