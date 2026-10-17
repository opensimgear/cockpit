//! Cleanup can need a fresh read-only CAN connection after process death.
//! This exception never grants a writer, output lease or new profile authority.
use crate::profile_session::{State, Status};
use serde_json::Value;

const DENIED: &str = "Only a disconnected, unowned STM32 transport and passive action reads may be opened for explicit profile recovery.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Operation {
    Connect,
    Describe,
    State,
    Ownership,
}

pub(crate) fn operation(command: &str) -> Option<Operation> {
    match command {
        "connect" => Some(Operation::Connect),
        "action_describe" => Some(Operation::Describe),
        "action_state" => Some(Operation::State),
        "action_ownership" => Some(Operation::Ownership),
        _ => None,
    }
}

pub(crate) fn recovery_only(status: &Status) -> bool {
    status.state == State::RecoveryNeeded
        && status.owner.is_none()
        && status.applied_snapshot.is_none()
}

/// Unknown connection state is not a disconnected transport. These values are
/// captured from the existing native adapter, never accepted from a request.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Transport {
    pub connected: Option<bool>,
    pub configuration_claims: bool,
    pub output_activity: bool,
}

/// Validate again at dispatch. Connect holds the Manager configuration gate;
/// existing candidate/path and wire UID checks still follow this admission.
pub(crate) fn admit(
    status: &Status,
    request: &Value,
    transport: Option<Transport>,
) -> Result<(), String> {
    let command = request["command"].as_str().ok_or(DENIED)?;
    let op = operation(command).ok_or(DENIED)?;
    if !recovery_only(status) {
        return Err(DENIED.into());
    }
    let fields = request.as_object().ok_or(DENIED)?;
    if fields.keys().any(|key| {
        !matches!(
            key.as_str(),
            "command" | "clientId" | "gatewayId" | "backend"
        ) && !match op {
            Operation::Connect => key == "port",
            Operation::Ownership => key == "moduleId" || key == "offset",
            _ => key == "moduleId",
        }
    }) {
        return Err(DENIED.into());
    }
    let gateway = request["gatewayId"].as_str().ok_or(DENIED)?;
    let serial = gateway.strip_prefix("stm32:").ok_or(DENIED)?;
    if super::canonical_id("stm32", serial).as_deref() != Some(gateway)
        || request.get("backend").is_some_and(|value| value != "stm32")
    {
        return Err(DENIED.into());
    }
    if op == Operation::Connect {
        if !request["port"].as_str().is_some_and(|port| {
            !port.is_empty() && port.len() <= 1024 && !port.chars().any(char::is_control)
        }) || transport.is_some_and(|state| {
            state.connected != Some(false) || state.configuration_claims || state.output_activity
        }) {
            return Err(DENIED.into());
        }
    } else {
        let module = request["moduleId"].as_str().ok_or(DENIED)?;
        if module.len() != 32
            || !module
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || !module.bytes().any(|byte| byte != b'0')
            || !module.bytes().any(|byte| byte != b'f')
            || transport.is_none_or(|state| state.connected != Some(true))
            || request.get("offset").is_some_and(|offset| {
                op != Operation::Ownership || offset.as_u64().is_none_or(|offset| offset > 16)
            })
        {
            return Err(DENIED.into());
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "profile_recovery_transport_tests.rs"]
mod tests;
