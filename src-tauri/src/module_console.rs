//! The existing standalone module CDC grammar. No request IDs or replay are
//! invented here; settings use ACK + JSON, actions/events use bare JSON.
use crate::transport::{Connection, PortInfo};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

pub(crate) const CONSOLE_BANNER: &str = "OpenSimGear generic Cyphal device console; type help";
pub(crate) const MAX_JSON_BYTES: usize = 4096;
const MAX_BUFFER_BYTES: usize = MAX_JSON_BYTES + 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UsbIdentity {
    pub model: String,
    pub board: String,
    pub profile: String,
    pub module_id: String,
    pub serial: String,
    pub protocol: u32,
    pub button_count: u32,
    pub axis_count: u32,
    pub hat_count: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ModuleStatus {
    pub boot: u32,
    pub direct_usb: bool,
    pub node: u32,
    pub route: u32,
    pub lease: u32,
}
pub(crate) fn serial(value: &str) -> Option<String> {
    (value.len() == 24
        && value.bytes().all(|b| b.is_ascii_hexdigit())
        && value.bytes().any(|b| b != b'0')
        && value.bytes().any(|b| b != b'f' && b != b'F'))
    .then(|| value.to_ascii_lowercase())
}
/// A user-selected USB port may expose generic Windows usbser metadata. This
/// gate permits only plausible MCU UID ports; the read-only info handshake is
/// still authoritative for the module role and its derived stable identity.
pub(crate) fn candidate_for_explicit(port: &PortInfo) -> bool {
    let hex = |value: &Option<String>| {
        value.as_deref().is_some_and(|v| {
            let v = v
                .strip_prefix("0x")
                .or_else(|| v.strip_prefix("0X"))
                .unwrap_or(v);
            v.len() == 4
                && v.bytes().all(|b| b.is_ascii_hexdigit())
                && u16::from_str_radix(v, 16).is_ok_and(|id| id != 0)
        })
    };
    crate::transport::allowed_path(&port.path)
        && port.serial_number.as_deref().and_then(serial).is_some()
        && hex(&port.vendor_id)
        && hex(&port.product_id)
        && [&port.friendly_name, &port.manufacturer]
            .into_iter()
            .all(|field| {
                field.as_deref().is_none_or(|value| {
                    let value = value.to_ascii_lowercase();
                    !value.contains("gateway") && !value.contains("companion")
                })
            })
}
pub(crate) fn candidate(port: &PortInfo) -> bool {
    candidate_for_explicit(port)
        && port
            .manufacturer
            .as_deref()
            .is_some_and(|s| s.eq_ignore_ascii_case("OpenSimGear"))
        && port
            .friendly_name
            .as_deref()
            .is_some_and(|s| s.to_ascii_lowercase().starts_with("device template"))
}

fn integer(value: &str, max: u32) -> Result<u32, String> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Invalid USB module numeric metadata.".into());
    }
    value
        .parse::<u32>()
        .ok()
        .filter(|n| *n <= max)
        .ok_or_else(|| "Invalid USB module numeric metadata.".into())
}
pub(crate) fn identity(line: &str, port: &PortInfo) -> Result<UsbIdentity, String> {
    if line.len() > 512 || line.chars().any(char::is_control) {
        return Err("Invalid USB module identity response.".into());
    }
    let rest = line
        .strip_prefix("model=device-template board=")
        .ok_or("The serial device is not a compatible standalone OSG module.")?;
    let (board, rest) = rest
        .split_once(" profile=")
        .ok_or("Invalid USB module board metadata.")?;
    let (profile, rest) = rest
        .rsplit_once(" module=")
        .ok_or("Missing USB module identity.")?;
    if board.is_empty()
        || board.len() > 31
        || !board
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        || profile.trim().is_empty()
        || profile.len() > 63
    {
        return Err("Invalid USB module identity metadata.".into());
    }
    let (module, rest) = rest
        .split_once(" protocol=")
        .ok_or("Missing USB module protocol.")?;
    let (protocol, rest) = rest
        .split_once(" buttons=")
        .ok_or("Missing USB module control counts.")?;
    let (buttons, rest) = rest
        .split_once(" axes=")
        .ok_or("Missing USB module axes.")?;
    let (axes, hats) = rest
        .split_once(" hats=")
        .ok_or("Missing USB module hats.")?;
    let serial = port
        .serial_number
        .as_deref()
        .and_then(serial)
        .ok_or("Missing reliable USB module serial.")?;
    // The shipped starter firmware derives both identities from the same MCU UID.
    // A display name or a matching VID/PID alone never proves this relationship.
    if module.to_ascii_lowercase() != format!("4f534744{serial}") || integer(protocol, 3)? != 3 {
        return Err("USB module role, protocol or serial identity does not match its port.".into());
    }
    Ok(UsbIdentity {
        model: "device-template".into(),
        board: board.into(),
        profile: profile.into(),
        module_id: module.to_ascii_lowercase(),
        serial,
        protocol: 3,
        button_count: integer(buttons, 128)?,
        axis_count: integer(axes, 8)?,
        hat_count: integer(hats, 4)?,
    })
}
pub(crate) fn status(line: &str) -> Result<ModuleStatus, String> {
    if line.len() > 512 || line.chars().any(char::is_control) {
        return Err("Invalid USB module status response.".into());
    }
    let mut fields = BTreeMap::new();
    for field in line.split(' ') {
        let (key, value) = field
            .split_once('=')
            .ok_or("Invalid USB module status field.")?;
        if fields.insert(key, value).is_some() {
            return Err("Duplicate USB module status field.".into());
        }
    }
    if fields.get("can") != Some(&"cyphal") {
        return Err("Incompatible USB module CAN protocol.".into());
    }
    let number = |key, max| {
        integer(
            fields
                .get(key)
                .copied()
                .ok_or("Missing USB module status field.")?,
            max,
        )
    };
    let boot = number("boot", u32::MAX)?;
    if boot == 0 {
        return Err("USB module boot session is invalid.".into());
    }
    for key in ["rx", "tx", "rejected", "recoveries"] {
        number(key, u32::MAX)?;
    }
    Ok(ModuleStatus {
        boot,
        direct_usb: number("usb", 1)? == 1,
        node: number("node", 127)?,
        route: number("route", 127)?,
        lease: number("lease", u32::MAX)?,
    })
}
pub(crate) fn validate_config(value: &Value) -> Result<(), String> {
    crate::runtime::validate(value)?;
    for (key, minimum) in [
        ("generation", 1),
        ("savedGeneration", 0),
        ("claimExpiresMs", 0),
    ] {
        if !value[key]
            .as_u64()
            .is_some_and(|n| n >= minimum && n <= u32::MAX as u64)
        {
            return Err("Invalid USB module configuration generation or claim expiry.".into());
        }
    }
    if !value["valid"].is_boolean() {
        return Err("Invalid USB module configuration validity.".into());
    }
    if value["valid"] == true {
        let count = |key: &str| {
            value["config"][key]
                .as_array()
                .map(Vec::len)
                .ok_or("Invalid USB module layout.")
        };
        let buttons = count("buttons")? + count("encoders")? * 2 + count("selectors")? * 3;
        let axes = count("axes")?;
        let hats = count("hats")?;
        if value["layout"]["buttons"].as_u64() != Some(buttons as u64)
            || value["layout"]["axes"].as_u64() != Some(axes as u64)
            || value["layout"]["hats"].as_u64() != Some(hats as u64)
            || value["layout"]["reportBytes"].as_u64()
                != Some((buttons.div_ceil(8) + axes * 2 + hats.div_ceil(2)) as u64)
            || !value["descriptorHex"].as_str().is_some_and(|s| {
                s.len() <= 512
                    && s.len() % 2 == 0
                    && s.bytes().all(|b| b.is_ascii_hexdigit())
                    && (buttons + axes + hats == 0 || !s.is_empty())
            })
        {
            return Err("USB module layout does not match its active configuration.".into());
        }
    }
    Ok(())
}
pub(crate) enum ReplyKind {
    Text,
    Config,
    Json,
}
#[derive(Debug)]
pub(crate) enum Reply {
    Text(String),
    Config {
        error: Option<String>,
        snapshot: Value,
    },
    Json(Value),
    /// Complete existing firmware negative reply; no capability data was supplied.
    Unsupported,
}

pub(crate) struct Console {
    connection: Box<dyn Connection>,
    buffer: Vec<u8>,
    handshake: bool,
    cancel: Arc<AtomicBool>,
}
impl Console {
    pub fn new(
        mut connection: Box<dyn Connection>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        connection.set_control_lines(true, false)?;
        Ok(Self {
            connection,
            buffer: vec![],
            handshake: true,
            cancel,
        })
    }
    pub fn finish_handshake(&mut self) {
        self.handshake = false;
    }
    fn line(&mut self, deadline: Instant) -> Result<String, String> {
        loop {
            if self.cancel.load(Ordering::Acquire) {
                return Err("USB module service has stopped.".into());
            }
            if Instant::now() >= deadline {
                return Err(
                    "USB module response timed out; the connection was invalidated.".into(),
                );
            }
            if let Some(newline) = self.buffer.iter().position(|b| *b == b'\n') {
                if newline > MAX_JSON_BYTES {
                    return Err("USB module response exceeds its byte limit.".into());
                }
                let mut line: Vec<u8> = self.buffer.drain(..=newline).collect();
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                let line =
                    String::from_utf8(line).map_err(|_| "USB module returned invalid UTF-8.")?;
                if line.is_empty() {
                    continue;
                }
                if line == CONSOLE_BANNER {
                    if self.handshake {
                        continue;
                    }
                    return Err("The USB module restarted; establish a fresh connection.".into());
                }
                return Ok(line);
            }
            if self.buffer.len() > MAX_JSON_BYTES {
                return Err("USB module response exceeds its byte limit.".into());
            }
            let mut chunk = [0u8; 1024];
            match self.connection.read(&mut chunk) {
                Ok(0) => return Err("USB module was disconnected.".into()),
                Ok(n) => {
                    if self.buffer.len() + n > MAX_BUFFER_BYTES {
                        return Err("USB module response exceeds its byte limit.".into());
                    }
                    self.buffer.extend_from_slice(&chunk[..n]);
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut
                            | io::ErrorKind::WouldBlock
                            | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(format!("USB module read failed: {error}")),
            }
        }
    }
    pub fn exchange(
        &mut self,
        command: &str,
        kind: ReplyKind,
        deadline: Instant,
    ) -> Result<Reply, String> {
        let maximum = if command.starts_with("config ") {
            390
        } else {
            383
        };
        if command.is_empty()
            || command.len() > maximum
            || !command.is_ascii()
            || command.bytes().any(|b| b.is_ascii_control())
        {
            return Err("Invalid USB module command.".into());
        }
        self.connection
            .write_all(command.as_bytes())
            .and_then(|_| self.connection.write_all(b"\n"))
            .and_then(|_| self.connection.flush())
            .map_err(|error| format!("USB module write failed; its result is unknown: {error}"))?;
        let line = self.line(deadline)?;
        match kind {
            ReplyKind::Text => Ok(Reply::Text(line)),
            ReplyKind::Json => {
                if line == "ERR unknown command" {
                    return Ok(Reply::Unsupported);
                }
                let value: Value = serde_json::from_str(&line)
                    .map_err(|_| "USB module returned malformed JSON.")?;
                if !value["ok"].is_boolean() {
                    return Err("USB module returned an invalid capability response.".into());
                }
                Ok(Reply::Json(value))
            }
            ReplyKind::Config => {
                let error = if line == "OK" {
                    None
                } else {
                    Some(
                        line.strip_prefix("ERR ")
                            .filter(|s| {
                                !s.is_empty()
                                    && s.len() <= 63
                                    && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                            })
                            .ok_or("USB module returned an invalid configuration acknowledgement.")?
                            .to_owned(),
                    )
                };
                // An error ACK is still followed by a snapshot; consume both.
                let snapshot: Value = serde_json::from_str(&self.line(deadline)?)
                    .map_err(|_| "USB module returned malformed configuration JSON.")?;
                validate_config(&snapshot)?;
                Ok(Reply::Config { error, snapshot })
            }
        }
    }
}
impl Drop for Console {
    fn drop(&mut self) {
        let _ = self.connection.set_control_lines(false, false);
    }
}

#[cfg(test)]
#[path = "module_console_tests.rs"]
mod tests;
