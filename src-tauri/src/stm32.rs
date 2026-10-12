//! Physical CAN gateway adapter. CDC observes and changes its registry; HID does
//! not depend on this service, and closing Cockpit must not stop physical inputs.
use crate::transport::{allowed_path, Connection, Factory, NativeFactory, PortInfo};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const CLOSED: &str = "Cockpit physical gateway service has closed.";
const NO_CONNECTION: &str = "Connect the OpenSimGear CAN gateway first.";
const FRAME_LIMIT: usize = 16_384;
const MODULE_LIMIT: usize = 10;
const INVALID: &str = "Unsupported or malformed OpenSimGear CAN gateway snapshot (protocol 2).";

#[derive(Clone)]
struct Timing {
    command: Duration,
    reconnect: Duration,
    settle: Duration,
    poll: Duration,
    heartbeat: Duration,
    ui_timeout: Duration,
    claim_timeout: Duration,
}
impl Default for Timing {
    fn default() -> Self {
        Self {
            command: Duration::from_secs(3),
            reconnect: Duration::from_secs(10),
            settle: Duration::from_millis(400),
            poll: Duration::from_millis(50),
            heartbeat: Duration::from_secs(2),
            ui_timeout: Duration::from_secs(8),
            claim_timeout: Duration::from_secs(15),
        }
    }
}

fn hex_identity(value: &str, digits: usize) -> bool {
    value.len() == digits
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        && value.bytes().any(|byte| byte != b'0')
        && value.bytes().any(|byte| byte != b'f' && byte != b'F')
}
fn number(value: &Value, maximum: u64) -> Option<u64> {
    value.as_u64().filter(|number| *number <= maximum)
}
fn slot(value: &Value) -> Option<i64> {
    value.as_i64().filter(|slot| (-1..=9).contains(slot))
}
fn printable(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}

/// Metadata only selects a candidate. A fresh protocol handshake validates
/// its role and stable UID before its inventory or controls can be used.
fn candidate(port: &PortInfo) -> bool {
    allowed_path(&port.path)
        && port
            .serial_number
            .as_deref()
            .is_some_and(|serial| hex_identity(serial, 24))
        && port
            .friendly_name
            .as_deref()
            .is_some_and(|name| name.to_ascii_lowercase().contains("gateway"))
        && port
            .manufacturer
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case("OpenSimGear"))
}

#[derive(Clone)]
struct Profile {
    buttons: usize,
    axes: usize,
    hats: usize,
    neutral: Vec<u16>,
    usages: Vec<u8>,
    value: Value,
}
fn profile(value: &Value) -> Result<Profile, String> {
    let bad = || INVALID.to_owned();
    let buttons = number(&value["buttons"], 128).ok_or_else(bad)? as usize;
    let axes = number(&value["axes"], 8).ok_or_else(bad)? as usize;
    let hats = number(&value["hats"], 4).ok_or_else(bad)? as usize;
    if buttons + axes + hats == 0
        || number(&value["revision"], 65_535)
            .filter(|revision| *revision > 0)
            .is_none()
    {
        return Err(bad());
    }
    let neutral = value["neutral"].as_array().ok_or_else(bad)?;
    let usages = value["axisUsage"].as_array().ok_or_else(bad)?;
    if neutral.len() != axes || usages.len() != axes {
        return Err(bad());
    }
    let neutral: Vec<u16> = neutral
        .iter()
        .map(|item| {
            number(item, 65_535)
                .map(|value| value as u16)
                .ok_or_else(bad)
        })
        .collect::<Result<_, _>>()?;
    let usages: Vec<u8> = usages
        .iter()
        .map(|item| {
            number(item, 0x38)
                .filter(|value| *value >= 0x30)
                .map(|value| value as u8)
                .ok_or_else(bad)
        })
        .collect::<Result<_, _>>()?;
    if usages.iter().copied().collect::<HashSet<_>>().len() != axes {
        return Err(bad());
    }
    Ok(Profile {
        buttons,
        axes,
        hats,
        neutral,
        usages,
        value: value.clone(),
    })
}

fn report(value: &Value, profile: &Profile) -> Result<(Vec<usize>, Vec<u16>, Vec<u8>), String> {
    let bad = || INVALID.to_owned();
    let text = value.as_str().ok_or_else(bad)?;
    let button_bytes = profile.buttons.div_ceil(8);
    let hat_bytes = profile.hats.div_ceil(2);
    let size = button_bytes + hat_bytes + 2 * profile.axes;
    if text.len() != size * 2 || size > 34 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let bytes: Vec<u8> = text
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).map_err(|_| bad())?;
            u8::from_str_radix(text, 16).map_err(|_| bad())
        })
        .collect::<Result<_, _>>()?;
    if !profile.buttons.is_multiple_of(8)
        && bytes[button_bytes - 1] & (0xff << (profile.buttons % 8)) != 0
    {
        return Err(bad());
    }
    let buttons = (0..profile.buttons)
        .filter(|index| bytes[index / 8] & (1 << (index % 8)) != 0)
        .map(|index| index + 1)
        .collect();
    let hats: Vec<u8> = (0..profile.hats)
        .map(|index| (bytes[button_bytes + index / 2] >> ((index % 2) * 4)) & 0xf)
        .collect();
    if hats.iter().any(|hat| *hat > 8)
        || (!profile.hats.is_multiple_of(2) && bytes[button_bytes + hat_bytes - 1] & 0xf0 != 0)
    {
        return Err(bad());
    }
    let offset = button_bytes + hat_bytes;
    let axes = (0..profile.axes)
        .map(|index| u16::from_le_bytes([bytes[offset + 2 * index], bytes[offset + 2 * index + 1]]))
        .collect();
    Ok((buttons, axes, hats))
}

/// Validate the complete bounded wire snapshot before producing UI state.
fn snapshot(wire: &Value, expected: Option<&str>) -> Result<Value, String> {
    let bad = || INVALID.to_owned();
    let uid = wire["identity"].as_str().ok_or_else(bad)?;
    if wire["gatewayType"] != "stm32-can-gateway"
        || wire["protocol"] != 2
        || !matches!(wire["canProtocol"].as_u64(), Some(2 | 3))
        || wire["maxModules"] != MODULE_LIMIT
        || !hex_identity(uid, 24)
        || !wire["running"].is_boolean()
        || !wire["needsApply"].is_boolean()
        || number(&wire["epoch"], u32::MAX as u64)
            .filter(|epoch| *epoch > 0)
            .is_none()
        || !wire["firmware"].as_str().is_some_and(|s| printable(s, 32))
    {
        return Err(bad());
    }
    if expected.is_some_and(|expected| !expected.eq_ignore_ascii_case(uid)) {
        return Err(
            "A different CAN gateway appeared on the serial port; reconnect the original gateway."
                .into(),
        );
    }
    let input = wire["modules"].as_array().ok_or_else(bad)?;
    if input.len() > MODULE_LIMIT {
        return Err(bad());
    }
    let mut ids = HashSet::new();
    let mut nodes = HashSet::new();
    let mut pending_slots = HashSet::new();
    let mut active_slots = HashSet::new();
    let mut modules = Vec::new();
    for module in input {
        let id = module["identity"].as_str().ok_or_else(bad)?;
        let id = id.to_ascii_lowercase();
        let pending = slot(&module["pendingSlot"]).ok_or_else(bad)?;
        let active = slot(&module["activeSlot"]).ok_or_else(bad)?;
        let node = number(&module["node"], 10).ok_or_else(bad)?;
        if !hex_identity(&id, 32)
            || !ids.insert(id.clone())
            || (node > 0 && !nodes.insert(node))
            || (pending >= 0 && !pending_slots.insert(pending))
            || (active >= 0 && !active_slots.insert(active))
            || number(&module["lease"], u32::MAX as u64).is_none()
            || (pending < 0 && module["pendingProfileMatch"] == true)
            || (active < 0 && module["profileMatch"] == true)
            || [
                "online",
                "directUsb",
                "profileMatch",
                "pendingProfileMatch",
                "hasInput",
            ]
            .iter()
            .any(|key| !module[key].is_boolean())
        {
            return Err(bad());
        }
        let discovered = profile(&module["profile"])?;
        // Snapshot reports always use the observed profile; mismatches against
        // the active USB descriptor produce neutral bytes under this profile.
        let (buttons, axes, hats) = report(&module["report"], &discovered)?;
        let label = format!("CAN device {}", &id[24..]);
        let delivering = wire["running"] == true
            && active >= 0
            && module["online"] == true
            && module["directUsb"] == false
            && module["profileMatch"] == true
            && module["hasInput"] == true;
        if !delivering
            && (!buttons.is_empty()
                || axes != discovered.neutral
                || hats.iter().any(|hat| *hat != 8))
        {
            return Err(bad());
        }
        modules.push(json!({
            "id":id,"name":label,"kind":"panel", "online":module["online"],
            "registered":pending >= 0,"active":delivering,
            "directUsb":module["directUsb"],"profileMatch":module["profileMatch"],
            "pendingProfileMatch":module["pendingProfileMatch"],
            "hasInput":module["hasInput"],"node":node,"lease":module["lease"],
            "pendingSlot":pending,"activeSlot":active,
            "reportId":if active >= 0 {json!(active + 1)} else {Value::Null},
            "buttonCount":discovered.buttons,"axisCount":discovered.axes,"hatCount":discovered.hats,
            "buttons":buttons,"axes":axes,"hats":hats,"axisUsage":discovered.usages,
            "neutral":discovered.neutral,"profile":discovered.value,
            "serialNumber":id,"usbSerialNumber":uid.to_ascii_lowercase(),"hidInterface":Value::Null,
            "settings":{"label":label,"debounce":10,"inverted":false},"canConfigure":false
        }));
    }
    Ok(
        json!({"backend":"stm32","gatewaySerial":uid.to_ascii_lowercase(),
        "gatewayType":wire["gatewayType"],"protocolVersion":2,"canProtocol":wire["canProtocol"],
        "firmwareVersion":wire["firmware"],"epoch":wire["epoch"],"maxModules":MODULE_LIMIT,
        "running":wire["running"],"needsReconnect":wire["needsApply"],"needsApply":wire["needsApply"],
        "reconnecting":false,"connected":true,"modules":modules,
        "hidNaming":{"supported":false,"applied":false}}),
    )
}

fn empty_state() -> Value {
    json!({"backend":"stm32","connected":false,"running":false,"needsReconnect":false,
        "needsApply":false,"reconnecting":false,"modules":[],"port":null,"ports":[],"maxModules":10})
}
struct Claim {
    client: String,
    seen: Instant,
    node: u64,
    lease: u64,
    direct_usb: bool,
}
struct Board {
    info: PortInfo,
    uid: String,
}
struct Gateway {
    factory: Box<dyn Factory>,
    connection: Option<Box<dyn Connection>>,
    board: Option<Board>,
    state: Value,
    buffer: Vec<u8>,
    claims: HashMap<String, Claim>,
    request_id: u32,
    timing: Timing,
    cancel: Arc<AtomicBool>,
}
impl Gateway {
    fn new(factory: Box<dyn Factory>, timing: Timing, cancel: Arc<AtomicBool>) -> Self {
        Self {
            factory,
            connection: None,
            board: None,
            state: empty_state(),
            buffer: Vec::new(),
            claims: HashMap::new(),
            request_id: 0,
            timing,
            cancel,
        }
    }
    fn ports(&mut self) -> Result<Vec<PortInfo>, String> {
        let ports: Vec<_> = self
            .factory
            .ports()?
            .into_iter()
            .filter(candidate)
            .collect();
        // macOS exposes call-in and call-out aliases for one CDC interface.
        let mut by_path: HashMap<String, PortInfo> = HashMap::new();
        for port in ports {
            let key = port
                .path
                .replace("/dev/tty.usbmodem", "/dev/usbmodem")
                .replace("/dev/cu.usbmodem", "/dev/usbmodem");
            if !by_path.contains_key(&key) || port.path.starts_with("/dev/cu.") {
                by_path.insert(key, port);
            }
        }
        let mut ports: Vec<_> = by_path.into_values().collect();
        ports.sort_by(|a, b| a.path.cmp(&b.path));
        self.state["ports"] = json!(ports.iter().map(PortInfo::value).collect::<Vec<_>>());
        Ok(ports)
    }
    fn close(&mut self) {
        if let Some(mut connection) = self.connection.take() {
            let _ = connection.set_control_lines(false, false);
        }
        self.buffer.clear();
        self.claims.clear();
        self.state["connected"] = json!(false);
        self.state["running"] = json!(false);
        self.state["reconnecting"] = json!(false);
        self.state["port"] = Value::Null;
        for module in self.state["modules"].as_array_mut().into_iter().flatten() {
            module["online"] = json!(false);
            module["active"] = json!(false);
            module["buttons"] = json!([]);
            module["axes"] = module["neutral"].clone();
            module["hats"] = json!(vec![8; module["hatCount"].as_u64().unwrap_or(0) as usize]);
        }
    }
    fn accept(&mut self, wire: &Value) -> Result<(), String> {
        let mut state = snapshot(wire, self.board.as_ref().map(|board| board.uid.as_str()))?;
        if self.state["epoch"].is_number() && self.state["epoch"] != state["epoch"] {
            self.claims.clear();
        }
        self.claims.retain(|id, claim| {
            state["modules"].as_array().unwrap().iter().any(|module| {
                module["id"].as_str() == Some(id.as_str())
                    && module["online"] == true
                    && module["node"].as_u64() == Some(claim.node)
                    && module["lease"].as_u64() == Some(claim.lease)
                    && module["directUsb"].as_bool() == Some(claim.direct_usb)
            })
        });
        state["ports"] = self.state["ports"].clone();
        state["port"] = self.state["port"].clone();
        self.state = state;
        Ok(())
    }
    fn exchange(&mut self, command: &str, timeout: Duration) -> Result<(), String> {
        if self.connection.is_none() {
            return Err(NO_CONNECTION.into());
        }
        self.request_id = self.request_id.wrapping_add(1).max(1);
        let id = self.request_id;
        let wire = if command == "snapshot" {
            format!("cockpit {id} snapshot\n")
        } else {
            let epoch = number(&self.state["epoch"], u32::MAX as u64).ok_or(NO_CONNECTION)?;
            format!("cockpit {id} {epoch} {command}\n")
        };
        if wire.len() > 79 {
            return Err("Gateway command is too long.".into());
        }
        let deadline = Instant::now() + timeout;
        let result = (|| {
            let connection = self.connection.as_mut().unwrap();
            connection
                .write_all(wire.as_bytes())
                .map_err(|e| format!("CAN gateway write failed: {e}"))?;
            connection
                .flush()
                .map_err(|e| format!("CAN gateway write failed: {e}"))?;
            loop {
                if self.cancel.load(Ordering::Acquire) {
                    return Err(CLOSED.into());
                }
                if Instant::now() >= deadline {
                    return Err(
                        "CAN gateway response timed out; connection and setup access were closed."
                            .into(),
                    );
                }
                while let Some(newline) = self.buffer.iter().position(|byte| *byte == b'\n') {
                    let line: Vec<_> = self.buffer.drain(..=newline).collect();
                    let line = line.strip_suffix(b"\n").unwrap_or(&line);
                    let line = line.strip_suffix(b"\r").unwrap_or(line);
                    if line.iter().all(u8::is_ascii_whitespace) {
                        continue;
                    }
                    // The human console prints a bounded identity/help banner at
                    // DTR open. Only JSON frames can satisfy machine requests.
                    if line.starts_with(b"gatewayType=") || line == b"type help for commands" {
                        continue;
                    }
                    let value: Value = serde_json::from_slice(line)
                        .map_err(|_| "CAN gateway sent malformed serial data.".to_owned())?;
                    if value["id"].as_u64() != Some(id as u64) {
                        continue;
                    }
                    if !value["ok"].is_boolean() {
                        return Err(INVALID.into());
                    }
                    return Ok(value);
                }
                let mut chunk = [0u8; 4096];
                match connection.read(&mut chunk) {
                    Ok(0) => return Err("CAN gateway was unplugged.".into()),
                    Ok(count) => {
                        if self.buffer.len() + count > FRAME_LIMIT {
                            return Err("CAN gateway serial frame exceeds 16384 bytes.".into());
                        }
                        self.buffer.extend_from_slice(&chunk[..count]);
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            io::ErrorKind::TimedOut
                                | io::ErrorKind::WouldBlock
                                | io::ErrorKind::Interrupted
                        ) => {}
                    Err(e) => return Err(format!("CAN gateway serial connection failed: {e}")),
                }
            }
        })();
        match result {
            Ok(value) => {
                if let Err(error) = self.accept(&value["state"]) {
                    self.close();
                    return Err(error);
                }
                if value["ok"] == false {
                    match value["error"].as_str().filter(|s|printable(s,256)) {
                        Some("stale_epoch")=>Err("Gateway restarted; inventory was refreshed. Review it before retrying the change.".into()),
                        Some("registration_rejected")=>Err("Gateway rejected registration: the module must be online with a compatible profile and a free slot.".into()),
                        Some("flash_save_failed")=>Err("Gateway could not save its registry; its USB layout was not changed.".into()),
                        Some(error)=>Err(format!("CAN gateway rejected the request: {error}")),
                        None=>{self.close();Err(INVALID.into())},
                    }
                } else if !value["error"].is_null() {
                    self.close();
                    Err(INVALID.into())
                } else {
                    Ok(())
                }
            }
            Err(e) => {
                self.close();
                Err(e)
            }
        }
    }
    fn open(
        &mut self,
        info: PortInfo,
        expected: Option<String>,
        deadline: Instant,
    ) -> Result<(), String> {
        self.close();
        if !candidate(&info) {
            return Err("Select an OpenSimGear CAN gateway serial port.".into());
        }
        let expected =
            expected.unwrap_or_else(|| info.serial_number.as_ref().unwrap().to_ascii_lowercase());
        self.board = Some(Board {
            info: info.clone(),
            uid: expected,
        });
        self.connection = Some(self.factory.open(&info, self.timing.poll)?);
        if let Err(e) = self
            .connection
            .as_mut()
            .unwrap()
            .set_control_lines(true, false)
        {
            self.close();
            return Err(e);
        }
        self.state["port"] = json!(info.path);
        self.exchange(
            "snapshot",
            deadline
                .saturating_duration_since(Instant::now())
                .min(self.timing.command),
        )
    }
    fn sleep(&self, duration: Duration) -> Result<(), String> {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            if self.cancel.load(Ordering::Acquire) {
                return Err(CLOSED.into());
            }
            thread::sleep(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(self.timing.poll),
            );
        }
        Ok(())
    }
    fn apply(&mut self) -> Result<(), String> {
        self.exchange("snapshot", self.timing.command)?;
        if self.state["needsApply"] != true {
            return Ok(());
        }
        let board = self.board.as_ref().ok_or(NO_CONNECTION)?;
        let expected = board.uid.clone();
        let previous_vendor = board.info.vendor_id.clone();
        let previous_product = board.info.product_id.clone();
        let target: Vec<(String, i64, Option<Value>)> = self.state["modules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                (
                    m["id"].as_str().unwrap().into(),
                    m["pendingSlot"].as_i64().unwrap(),
                    (m["pendingSlot"].as_i64().unwrap() >= 0 && m["pendingProfileMatch"] == true)
                        .then(|| m["profile"].clone()),
                )
            })
            .collect();
        // Once apply has been sent, losing its acknowledgement is ambiguous. Do
        // not send it again: reconnect and reconcile the committed registry.
        let acknowledgement = self.exchange("apply", self.timing.command);
        if let Err(error) = &acknowledgement {
            if self.connection.is_some() {
                return Err(error.clone());
            }
        }
        self.claims.clear();
        self.state["reconnecting"] = json!(true);
        self.sleep(self.timing.settle)?;
        self.close();
        self.state["reconnecting"] = json!(true);
        let deadline = Instant::now() + self.timing.reconnect;
        let mut last_error = None;
        while Instant::now() < deadline && !self.cancel.load(Ordering::Acquire) {
            let found = self.ports()?.into_iter().find(|port| {
                port.serial_number
                    .as_deref()
                    .is_some_and(|uid| uid.eq_ignore_ascii_case(&expected))
                    && port.vendor_id == previous_vendor
                    && port.product_id == previous_product
            });
            if let Some(port) = found {
                match self.open(port, Some(expected.clone()), deadline) {
                    Ok(()) => {
                        let applied = self.state["needsApply"] == false
                            && target.iter().all(|(id, pending, expected_profile)| {
                                self.state["modules"]
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .find(|m| m["id"].as_str() == Some(id.as_str()))
                                    .map(|m| {
                                        m["activeSlot"].as_i64() == Some(*pending)
                                            && expected_profile.as_ref().is_none_or(|expected| {
                                                m["profile"] == *expected
                                                    && m["profileMatch"] == true
                                            })
                                    })
                                    .unwrap_or(*pending == -1)
                            });
                        if applied {
                            return Ok(());
                        }
                        return Err("Gateway returned, but its requested USB layout is still pending. Review the inventory before retrying apply.".into());
                    }
                    Err(e) => last_error = Some(e),
                }
            }
            self.sleep(self.timing.poll)?;
        }
        self.close();
        Err(format!("CAN gateway did not return after apply. Reconnect it to inspect whether the layout committed; apply was not repeated.{}",last_error.map(|e|format!(" {e}")).unwrap_or_default()))
    }
    fn expire_claims(&mut self, client: Option<&str>) {
        self.claims.retain(|_, claim| {
            if claim.seen.elapsed() > self.timing.claim_timeout {
                return false;
            }
            if client == Some(claim.client.as_str()) {
                claim.seen = Instant::now();
            }
            true
        });
    }
    fn decorated(&self, client: Option<&str>) -> Value {
        let mut state = self.state.clone();
        let mut claims: Vec<_> = self
            .claims
            .iter()
            .filter(|(_, c)| Some(c.client.as_str()) == client)
            .map(|(id, _)| id.clone())
            .collect();
        claims.sort();
        state["claimedModules"] = json!(claims);
        state
    }
    fn handle(&mut self, request: &Value) -> Result<(), String> {
        if self.cancel.load(Ordering::Acquire) {
            return Err(CLOSED.into());
        }
        if !request.is_object() || request.to_string().len() > 8192 {
            return Err("Invalid CAN gateway request.".into());
        }
        let client = request["clientId"]
            .as_str()
            .filter(|id| crate::protocol::identifier(id, 16, 80))
            .ok_or("Missing Cockpit client identity; reload the application.")?;
        let command = request["command"]
            .as_str()
            .ok_or("Missing CAN gateway command.")?;
        let module_id = request["moduleId"]
            .as_str()
            .unwrap_or("")
            .to_ascii_lowercase();
        self.expire_claims(Some(client));
        if matches!(
            command,
            "connect"
                | "disconnect"
                | "register"
                | "unregister"
                | "remove"
                | "apply"
                | "reconnect"
                | "save"
        ) && !self.claims.is_empty()
        {
            return Err("End setup sessions before changing the CAN gateway layout.".into());
        }
        match command {
            "ports"=>{self.ports()?;Ok(())},
            "status" if self.connection.is_none()=>{self.ports()?;Ok(())},
            "status"=>self.exchange("snapshot",self.timing.command),
            "connect"=>{
                let port=self.ports()?.into_iter().find(|p|request["port"]==p.path)
                    .ok_or("Select an available OpenSimGear CAN gateway port.")?;
                self.open(port,None,Instant::now()+self.timing.command)
            },
            "disconnect"=>{self.close();Ok(())},
            "register"|"unregister"|"remove"=>{
                if !hex_identity(&module_id,32) { return Err("Module identity must be 32 hexadecimal digits.".into()); }
                let verb=if command=="register" {"register"} else {"unregister"};
                self.exchange(&format!("{verb} {module_id}"),self.timing.command)
            },
            "save"=>self.exchange("save",self.timing.command),
            "apply"|"reconnect"=>self.apply(),
            "claim"=>{
                if self.claims.get(&module_id).is_some_and(|c|c.client!=client) { return Err("Another Cockpit session owns this module.".into()); }
                self.exchange("snapshot",self.timing.command)?;
                let module=self.state["modules"].as_array().unwrap().iter().find(|m|m["id"]==module_id && m["online"]==true)
                    .ok_or("Reconnect the module before starting a session.")?;
                self.claims.insert(module_id,Claim{client:client.into(),seen:Instant::now(),node:module["node"].as_u64().unwrap(),
                    lease:module["lease"].as_u64().unwrap(),direct_usb:module["directUsb"].as_bool().unwrap()});Ok(())
            },
            "release"=>{if self.claims.get(&module_id).is_some_and(|c|c.client==client) {self.claims.remove(&module_id);}Ok(())},
            "input"|"presence"|"add"|"configure"|"start"|"stop"=>Err("This action is unavailable for physical CAN devices. Inputs come from hardware; device configuration transport is not implemented.".into()),
            _=>Err("Unknown CAN gateway command.".into()),
        }
    }
    fn request(&mut self, request: Value) -> Value {
        let client = request["clientId"].as_str();
        match self.handle(&request) {
            Ok(()) => json!({"ok":true,"state":self.decorated(client)}),
            Err(error) => json!({"ok":false,"error":error,"state":self.decorated(client)}),
        }
    }
    fn heartbeat(&mut self, seen: Instant) {
        if self.connection.is_none() {
            return;
        }
        if seen.elapsed() > self.timing.ui_timeout {
            self.close();
            return;
        }
        self.expire_claims(None);
        let _ = self.exchange("snapshot", self.timing.command);
    }
}

enum Message {
    Request(Value, mpsc::Sender<Value>),
    Shutdown,
}
pub struct Stm32Service {
    sender: mpsc::SyncSender<Message>,
    closed: Arc<AtomicBool>,
    state: Arc<Mutex<Value>>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}
impl Default for Stm32Service {
    fn default() -> Self {
        Self::new()
    }
}
impl Stm32Service {
    pub fn new() -> Self {
        Self::with_factory(Box::new(NativeFactory), Timing::default())
    }
    fn with_factory(factory: Box<dyn Factory>, timing: Timing) -> Self {
        let (sender, receiver) = mpsc::sync_channel(8);
        let closed = Arc::new(AtomicBool::new(false));
        let cancel = closed.clone();
        let state = Arc::new(Mutex::new(empty_state()));
        let latest = state.clone();
        let worker = thread::Builder::new()
            .name("osg-stm32-cdc".into())
            .spawn(move || {
                let mut gateway = Gateway::new(factory, timing, cancel.clone());
                let mut seen = Instant::now();
                let mut heartbeat = Instant::now() + gateway.timing.heartbeat;
                while !cancel.load(Ordering::Acquire) {
                    match receiver.recv_timeout(heartbeat.saturating_duration_since(Instant::now()))
                    {
                        Ok(Message::Request(request, response)) => {
                            seen = Instant::now();
                            let result = gateway.request(request);
                            *latest.lock().unwrap_or_else(|p| p.into_inner()) =
                                gateway.decorated(None);
                            let _ = response.send(result);
                        }
                        Ok(Message::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            gateway.heartbeat(seen);
                            heartbeat = Instant::now() + gateway.timing.heartbeat;
                            *latest.lock().unwrap_or_else(|p| p.into_inner()) =
                                gateway.decorated(None);
                        }
                    }
                }
                gateway.close();
                *latest.lock().unwrap_or_else(|p| p.into_inner()) = gateway.decorated(None);
                while let Ok(Message::Request(_, response)) = receiver.try_recv() {
                    let _ = response
                        .send(json!({"ok":false,"error":CLOSED,"state":gateway.decorated(None)}));
                }
            })
            .expect("could not start physical gateway worker");
        Self {
            sender,
            closed,
            state,
            worker: Mutex::new(Some(worker)),
        }
    }
    pub fn request(&self, request: Value) -> Value {
        let error = |message: &str| json!({"ok":false,"error":message,"state":self.state.lock().unwrap_or_else(|p|p.into_inner()).clone()});
        if self.closed.load(Ordering::Acquire) {
            return error(CLOSED);
        }
        let (sender, receiver) = mpsc::channel();
        if let Err(e) = self.sender.try_send(Message::Request(request, sender)) {
            return error(match e {
                mpsc::TrySendError::Full(_) => "CAN gateway is busy; wait for the current request.",
                mpsc::TrySendError::Disconnected(_) => CLOSED,
            });
        }
        receiver.recv().unwrap_or_else(|_| error(CLOSED))
    }
    pub fn shutdown(&self) {
        self.closed.store(true, Ordering::Release);
        let _ = self.sender.try_send(Message::Shutdown);
        if let Some(worker) = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = worker.join();
        }
    }
}
impl Drop for Stm32Service {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::VecDeque,
        io::{Read, Write},
    };

    const UID: &str = "10112233445566778899aabb";
    const MODULE: &str = "00112233445566778899aabbccddeeff";
    const PATH: &str = "/dev/cu.usbmodemGW1";
    const CLIENT: &str = "cockpit-client-0001";
    const NEUTRAL: &str = "0000880800000080";

    fn state() -> Value {
        json!({"gatewayType":"stm32-can-gateway","protocol":2,"canProtocol":3,
            "firmware":"0.3.0","identity":UID,"epoch":65536,"running":true,"needsApply":false,"maxModules":10,
            "modules":[{"identity":MODULE,"online":true,"directUsb":false,"node":1,"lease":65537,
                "profile":{"buttons":9,"axes":2,"hats":3,"revision":1,"axisUsage":[48,49],"neutral":[0,32768]},
                "pendingSlot":0,"activeSlot":0,"profileMatch":true,"pendingProfileMatch":true,"hasInput":true,"report":"0100870834120080"}]})
    }
    #[test]
    fn preserves_can_contract_version_for_legacy_and_cyphal_gateways() {
        for version in [2, 3] {
            let mut wire = state();
            wire["canProtocol"] = json!(version);
            let decoded = snapshot(&wire, Some(UID)).unwrap();
            assert_eq!(decoded["canProtocol"], version);
        }
        let mut wire = state();
        wire["canProtocol"] = json!(4);
        assert!(snapshot(&wire, None).is_err());
    }
    #[test]
    fn accepts_actual_c_firmware_formatter_conformance_vectors() {
        let fixtures: Value =
            serde_json::from_str(include_str!("../tests/fixtures/stm32-gateway-v2.json")).unwrap();
        for (name, envelope) in fixtures.as_object().unwrap() {
            let parsed = snapshot(&envelope["state"], Some("0123456789abcdef01234567"));
            assert!(parsed.is_ok(), "{name}: {parsed:?}");
            let parsed = parsed.unwrap();
            let module = &parsed["modules"][0];
            if name == "active_report" {
                assert_eq!(module["buttonCount"], 128);
                assert_eq!(module["hatCount"], 4);
                assert_eq!(module["hats"], json!([0, 7, 8, 8]));
                assert_eq!(
                    module["axes"],
                    json!([5139, 5653, 6167, 6681, 7195, 7709, 8223, 8737])
                );
                assert_eq!(module["active"], true);
            } else {
                assert_eq!(module["buttons"], json!([]), "{name}");
                assert_eq!(module["axes"], module["neutral"], "{name}");
                assert_eq!(module["active"], false, "{name}");
            }
            assert_eq!(
                module["pendingProfileMatch"],
                envelope["state"]["modules"][0]["pendingProfileMatch"]
            );
        }
    }
    fn info(path: &str) -> PortInfo {
        PortInfo {
            path: path.into(),
            serial_number: Some(UID.into()),
            vendor_id: Some("1234".into()),
            product_id: Some("abcd".into()),
            manufacturer: Some("OpenSimGear".into()),
            friendly_name: Some("CAN Gateway Rev A".into()),
        }
    }
    fn timing() -> Timing {
        Timing {
            command: Duration::from_millis(15),
            reconnect: Duration::from_millis(45),
            settle: Duration::from_millis(1),
            poll: Duration::from_millis(1),
            heartbeat: Duration::from_millis(5),
            ui_timeout: Duration::from_millis(25),
            claim_timeout: Duration::from_millis(15),
        }
    }
    #[test]
    fn validates_and_decodes_packed_buttons_hats_axes_at_generic_bounds() {
        let decoded = snapshot(&state(), Some(UID)).unwrap();
        assert_eq!(decoded["modules"][0]["buttons"], json!([1]));
        assert_eq!(decoded["modules"][0]["hats"], json!([7, 8, 8]));
        assert_eq!(decoded["modules"][0]["axes"], json!([4660, 32768]));
        assert_eq!(decoded["modules"][0]["reportId"], 1);
        let mut maximum = state();
        maximum["modules"][0]["profile"] = json!({"buttons":128,"axes":8,"hats":4,"revision":65535,
            "axisUsage":[48,49,50,51,52,53,54,55],"neutral":[0,0,0,0,0,0,0,0]});
        maximum["modules"][0]["report"] =
            json!(format!("{}808888{}", "00".repeat(15), "00".repeat(16)));
        let decoded = snapshot(&maximum, None).unwrap();
        assert_eq!(decoded["modules"][0]["buttons"], json!([128]));
        assert_eq!(decoded["modules"][0]["hats"], json!([8, 8, 8, 8]));
    }
    #[test]
    fn rejects_invalid_capabilities_identity_slots_reports_and_unowned_input() {
        let mutations = [
            ("/gatewayType", json!("stm32-can-bringup")),
            ("/protocol", json!(1)),
            ("/canProtocol", json!(1)),
            ("/maxModules", json!(11)),
            ("/identity", json!("000000000000000000000000")),
            ("/epoch", json!(4294967296u64)),
            ("/epoch", json!(0)),
            ("/modules/0/identity", json!("ff".repeat(16))),
            ("/modules/0/profile/buttons", json!(129)),
            ("/modules/0/profile/revision", json!(0)),
            ("/modules/0/profile/hats", json!(5)),
            ("/modules/0/profile/axes", json!(9)),
            ("/modules/0/profile/axisUsage", json!([48, 48])),
            ("/modules/0/profile/neutral", json!([0, 65536])),
            ("/modules/0/pendingSlot", json!(10)),
            ("/modules/0/activeSlot", json!(-2)),
            ("/modules/0/report", json!("0100870934120080")),
            ("/modules/0/report", json!("0180870834120080")),
            ("/modules/0/report", json!("0100878834120080")),
            ("/modules/0/report", json!("00")),
            ("/modules/0/report", json!("xx".repeat(8))),
            ("/modules/0/directUsb", json!(true)),
            ("/modules/0/profileMatch", json!(false)),
            ("/modules/0/hasInput", json!(false)),
            ("/running", json!(false)),
        ];
        for (path, value) in mutations {
            let mut changed = state();
            *changed.pointer_mut(path).unwrap() = value;
            assert!(snapshot(&changed, None).is_err(), "accepted {path}");
        }
        let mut duplicate = state();
        let module = duplicate["modules"][0].clone();
        duplicate["modules"].as_array_mut().unwrap().push(module);
        assert!(snapshot(&duplicate, None).is_err());
        assert!(snapshot(&state(), Some("99992233445566778899aabb"))
            .unwrap_err()
            .contains("different CAN gateway"));
        let mut offline = state();
        offline["modules"][0]["online"] = json!(false);
        offline["modules"][0]["report"] = json!(NEUTRAL);
        let decoded = snapshot(&offline, None).unwrap();
        assert_eq!(decoded["modules"][0]["activeSlot"], 0);
        assert_eq!(decoded["modules"][0]["registered"], true);
        assert_eq!(decoded["modules"][0]["active"], false);
    }

    struct Mock {
        state: Value,
        ports: Vec<PortInfo>,
        writes: Vec<String>,
        opened: Vec<String>,
        lines: Vec<(bool, bool)>,
        fragment: usize,
        stale: bool,
        timeout: Option<String>,
        malformed: bool,
        oversized: bool,
        drop_apply_ack: bool,
        fail_apply: bool,
        keep_pending: bool,
        swap_identity: bool,
        restart_on_register: bool,
        new_path: Option<String>,
    }
    type Context = Arc<Mutex<Mock>>;
    struct MockFactory(Context);
    struct MockConnection {
        context: Context,
        data: VecDeque<u8>,
    }
    impl Factory for MockFactory {
        fn ports(&mut self) -> Result<Vec<PortInfo>, String> {
            Ok(self.0.lock().unwrap().ports.clone())
        }
        fn open(&mut self, port: &PortInfo, _: Duration) -> Result<Box<dyn Connection>, String> {
            self.0.lock().unwrap().opened.push(port.path.clone());
            Ok(Box::new(MockConnection {
                context: self.0.clone(),
                data: b"gatewayType=stm32-can-gateway protocol=2\r\ntype help for commands\r\n"
                    .iter()
                    .copied()
                    .collect(),
            }))
        }
    }
    impl Read for MockConnection {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.data.is_empty() {
                thread::sleep(Duration::from_millis(1));
                return Err(io::ErrorKind::TimedOut.into());
            }
            let count = buffer
                .len()
                .min(self.data.len())
                .min(self.context.lock().unwrap().fragment);
            for byte in buffer.iter_mut().take(count) {
                *byte = self.data.pop_front().unwrap();
            }
            Ok(count)
        }
    }
    impl Write for MockConnection {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let text = std::str::from_utf8(bytes).unwrap();
            let tokens: Vec<_> = text.split_whitespace().collect();
            assert_eq!(tokens[0], "cockpit");
            let id: u32 = tokens[1].parse().unwrap();
            let mut context = self.context.lock().unwrap();
            context.writes.push(text.into());
            let mutation = tokens[2] != "snapshot";
            let command = if mutation { tokens[3] } else { tokens[2] };
            if context.timeout.as_deref() == Some(command) {
                return Ok(bytes.len());
            }
            if context.malformed {
                self.data.extend(b"{broken\n");
                return Ok(bytes.len());
            }
            if context.oversized {
                self.data.extend(vec![b'x'; FRAME_LIMIT + 1]);
                return Ok(bytes.len());
            }
            let mut error = None;
            if context.restart_on_register && command == "register" {
                context.state["epoch"] = json!(131072);
            }
            if mutation
                && tokens[2].parse::<u64>().unwrap() != context.state["epoch"].as_u64().unwrap()
            {
                error = Some("stale_epoch");
            } else {
                match command {
                    "register" => {
                        assert_eq!(tokens[4], MODULE);
                        context.state["modules"][0]["pendingSlot"] = json!(0);
                        context.state["modules"][0]["pendingProfileMatch"] = json!(true);
                        context.state["needsApply"] =
                            json!(context.state["modules"][0]["activeSlot"] != 0);
                    }
                    "unregister" => {
                        assert_eq!(tokens[4], MODULE);
                        context.state["modules"][0]["pendingSlot"] = json!(-1);
                        context.state["modules"][0]["pendingProfileMatch"] = json!(false);
                        context.state["needsApply"] = json!(true);
                    }
                    "apply" if context.fail_apply => error = Some("flash_save_failed"),
                    "apply" => {}
                    "snapshot" | "save" => {}
                    _ => panic!("unexpected command {command}"),
                }
            }
            let ack = context.state.clone();
            if command == "apply" && error.is_none() {
                if !context.keep_pending {
                    context.state["needsApply"] = json!(false);
                    context.state["modules"][0]["activeSlot"] =
                        context.state["modules"][0]["pendingSlot"].clone();
                    if context.state["modules"][0]["activeSlot"] == -1 {
                        context.state["modules"][0]["profileMatch"] = json!(false);
                        context.state["modules"][0]["report"] = json!(NEUTRAL);
                    }
                }
                if let Some(path) = context.new_path.clone() {
                    context.ports[0].path = path;
                }
                if context.swap_identity {
                    context.state["identity"] = json!("99992233445566778899aabb");
                }
                if context.drop_apply_ack {
                    return Ok(bytes.len());
                }
            }
            if context.stale {
                self.data
                    .extend(b"{\"id\":0,\"ok\":true,\"state\":{\"identity\":\"wrong\"}}\n");
            }
            self.data.extend(
                format!(
                    "{}\n",
                    json!({"id":id,"ok":error.is_none(),"error":error,"state":ack})
                )
                .bytes(),
            );
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl Connection for MockConnection {
        fn set_control_lines(&mut self, dtr: bool, rts: bool) -> Result<(), String> {
            // Unlike Pico, physical HID must survive serial close/app exit.
            self.context.lock().unwrap().lines.push((dtr, rts));
            Ok(())
        }
    }
    fn harness() -> (Gateway, Context) {
        let context = Arc::new(Mutex::new(Mock {
            state: state(),
            ports: vec![info(PATH)],
            writes: Vec::new(),
            opened: Vec::new(),
            lines: Vec::new(),
            fragment: 4096,
            stale: false,
            timeout: None,
            malformed: false,
            oversized: false,
            drop_apply_ack: false,
            fail_apply: false,
            keep_pending: false,
            swap_identity: false,
            restart_on_register: false,
            new_path: None,
        }));
        (
            Gateway::new(
                Box::new(MockFactory(context.clone())),
                timing(),
                Arc::new(AtomicBool::new(false)),
            ),
            context,
        )
    }
    fn request(gateway: &mut Gateway, command: &str, mut value: Value) -> Value {
        value["command"] = json!(command);
        value["clientId"] = json!(CLIENT);
        gateway.request(value)
    }
    fn connect(gateway: &mut Gateway) -> Value {
        request(gateway, "connect", json!({"port":PATH}))
    }
    fn stage_remove(gateway: &mut Gateway) {
        assert_eq!(
            request(gateway, "remove", json!({"moduleId":MODULE}))["ok"],
            true
        );
    }

    #[test]
    fn discovery_uses_usb_role_metadata_deduplicates_and_never_opens_unknown_ports() {
        let (mut gateway, context) = harness();
        let mut generic = info("/dev/cu.usbmodemMODULE");
        generic.friendly_name = Some("Generic device".into());
        let mut alien = info("/dev/cu.usbmodemOTHER");
        alien.manufacturer = Some("Other vendor".into());
        context.lock().unwrap().ports.extend([
            info("/dev/tty.usbmodemGW1"),
            generic,
            alien,
            info("/tmp/file"),
        ]);
        let result = request(&mut gateway, "ports", json!({}));
        assert_eq!(result["ok"], true);
        assert_eq!(result["state"]["ports"].as_array().unwrap().len(), 1);
        assert!(context.lock().unwrap().opened.is_empty());
        assert_eq!(
            request(&mut gateway, "connect", json!({"port":"/tmp/file"}))["ok"],
            false
        );
        assert!(context.lock().unwrap().opened.is_empty());
        let result = connect(&mut gateway);
        assert_eq!(result["ok"], true, "{result}");
        assert_eq!(result["state"]["running"], true);
        assert_eq!(context.lock().unwrap().writes.len(), 1);
        assert!(context.lock().unwrap().writes[0].ends_with(" snapshot\n"));
    }
    #[test]
    fn fragmented_stale_frames_cannot_replace_inventory() {
        let (mut gateway, context) = harness();
        context.lock().unwrap().fragment = 7;
        context.lock().unwrap().stale = true;
        let result = connect(&mut gateway);
        assert_eq!(result["ok"], true, "{result}");
        assert_eq!(result["state"]["gatewaySerial"], UID);
        assert_eq!(result["state"]["modules"][0]["id"], MODULE);
    }
    #[test]
    fn registry_mutations_use_epoch_and_rejected_old_epoch_refreshes_inventory() {
        let (mut gateway, context) = harness();
        assert_eq!(connect(&mut gateway)["ok"], true);
        stage_remove(&mut gateway);
        assert_eq!(
            context.lock().unwrap().writes[1],
            format!("cockpit 2 65536 unregister {MODULE}\n")
        );
        context.lock().unwrap().restart_on_register = true;
        let result = request(&mut gateway, "register", json!({"moduleId":MODULE}));
        assert_eq!(result["ok"], false);
        assert_eq!(result["state"]["epoch"], 131072);
        assert_eq!(result["state"]["connected"], true);
        assert_eq!(result["state"]["modules"][0]["pendingSlot"], -1);
        assert_eq!(
            request(&mut gateway, "register", json!({"moduleId":MODULE}))["ok"],
            true
        );
        assert_eq!(
            context.lock().unwrap().writes[3],
            format!("cockpit 4 131072 register {MODULE}\n")
        );
    }
    #[test]
    fn apply_reconnects_by_uid_to_new_path_and_never_repeats_lost_ack() {
        for lost_ack in [false, true] {
            let (mut gateway, context) = harness();
            assert_eq!(connect(&mut gateway)["ok"], true);
            stage_remove(&mut gateway);
            {
                let mut mock = context.lock().unwrap();
                mock.drop_apply_ack = lost_ack;
                mock.new_path = Some("/dev/cu.usbmodemGW2".into());
            }
            let result = request(&mut gateway, "apply", json!({}));
            assert_eq!(result["ok"], true, "{result}");
            assert_eq!(result["state"]["port"], "/dev/cu.usbmodemGW2");
            assert_eq!(result["state"]["modules"][0]["activeSlot"], -1);
            assert_eq!(result["state"]["needsReconnect"], false);
            assert_eq!(
                context
                    .lock()
                    .unwrap()
                    .writes
                    .iter()
                    .filter(|line| line.trim_end().ends_with(" apply"))
                    .count(),
                1
            );
            assert_eq!(context.lock().unwrap().opened.len(), 2);
        }
    }
    #[test]
    fn apply_flash_failure_and_ambiguous_pending_layout_require_review() {
        let (mut gateway, context) = harness();
        assert_eq!(connect(&mut gateway)["ok"], true);
        stage_remove(&mut gateway);
        context.lock().unwrap().fail_apply = true;
        let result = request(&mut gateway, "apply", json!({}));
        assert_eq!(result["ok"], false);
        assert_eq!(result["state"]["connected"], true);
        assert_eq!(result["state"]["modules"][0]["activeSlot"], 0);
        assert_eq!(context.lock().unwrap().opened.len(), 1);
        context.lock().unwrap().fail_apply = false;
        context.lock().unwrap().keep_pending = true;
        let result = request(&mut gateway, "apply", json!({}));
        assert_eq!(result["ok"], false);
        assert!(result["error"].as_str().unwrap().contains("still pending"));
        assert_eq!(result["state"]["needsApply"], true);
    }
    #[test]
    fn malformed_oversized_and_timeout_responses_close_without_stopping_hardware_hid() {
        for failure in ["malformed", "oversized", "timeout", "identity"] {
            let (mut gateway, context) = harness();
            assert_eq!(connect(&mut gateway)["ok"], true);
            {
                let mut mock = context.lock().unwrap();
                match failure {
                    "malformed" => mock.malformed = true,
                    "oversized" => mock.oversized = true,
                    "timeout" => mock.timeout = Some("snapshot".into()),
                    "identity" => mock.state["identity"] = json!("99992233445566778899aabb"),
                    _ => unreachable!(),
                }
            }
            let result = request(&mut gateway, "status", json!({}));
            assert_eq!(result["ok"], false, "{failure}: {result}");
            assert_eq!(result["state"]["connected"], false);
            assert_eq!(result["state"]["modules"][0]["buttons"], json!([]));
            assert_eq!(result["state"]["modules"][0]["axes"], json!([0, 32768]));
            assert_eq!(result["state"]["modules"][0]["hats"], json!([8, 8, 8]));
            assert_eq!(context.lock().unwrap().state["running"], true);
            assert!(context.lock().unwrap().lines.contains(&(false, false)));
            assert!(context
                .lock()
                .unwrap()
                .writes
                .iter()
                .all(|wire| !wire.contains("stop")));
        }
    }
    #[test]
    fn incompatible_identity_after_apply_cannot_take_over_and_apply_is_not_replayed() {
        let (mut gateway, context) = harness();
        assert_eq!(connect(&mut gateway)["ok"], true);
        stage_remove(&mut gateway);
        context.lock().unwrap().swap_identity = true;
        let result = request(&mut gateway, "apply", json!({}));
        assert_eq!(result["ok"], false);
        assert_eq!(result["state"]["connected"], false);
        assert_eq!(result["state"]["gatewaySerial"], UID);
        assert_eq!(
            context
                .lock()
                .unwrap()
                .writes
                .iter()
                .filter(|wire| wire.trim_end().ends_with(" apply"))
                .count(),
            1
        );
    }
    #[test]
    fn setup_claims_gate_registry_and_route_epoch_or_ui_loss_releases_access() {
        let (mut gateway, context) = harness();
        assert_eq!(connect(&mut gateway)["ok"], true);
        assert_eq!(
            request(&mut gateway, "claim", json!({"moduleId":MODULE}))["ok"],
            true
        );
        assert_eq!(
            request(&mut gateway, "remove", json!({"moduleId":MODULE}))["ok"],
            false
        );
        let mut other = json!({"command":"claim","moduleId":MODULE,"clientId":"other-client-0001"});
        assert_eq!(gateway.request(other.clone())["ok"], false);
        context.lock().unwrap().state["epoch"] = json!(131072);
        assert_eq!(
            request(&mut gateway, "status", json!({}))["state"]["claimedModules"],
            json!([])
        );
        assert_eq!(
            request(&mut gateway, "claim", json!({"moduleId":MODULE}))["ok"],
            true
        );
        context.lock().unwrap().state["modules"][0]["lease"] = json!(131074);
        let result = request(&mut gateway, "status", json!({}));
        assert_eq!(result["state"]["claimedModules"], json!([]));
        assert_eq!(result["state"]["connected"], true);
        assert_eq!(
            request(&mut gateway, "claim", json!({"moduleId":MODULE}))["ok"],
            true
        );
        {
            let mut mock = context.lock().unwrap();
            mock.state["modules"][0]["online"] = json!(false);
            mock.state["modules"][0]["report"] = json!(NEUTRAL);
        }
        let result = request(&mut gateway, "status", json!({}));
        assert_eq!(result["state"]["claimedModules"], json!([]));
        assert_eq!(result["state"]["connected"], true);
        context.lock().unwrap().state["modules"][0]["online"] = json!(true);
        other["command"] = json!("claim");
        assert_eq!(gateway.request(other)["ok"], true);
        gateway.heartbeat(Instant::now() - Duration::from_millis(100));
        assert!(gateway.connection.is_none());
        assert!(gateway.claims.is_empty());
        assert_eq!(context.lock().unwrap().state["running"], true);
    }
    #[test]
    fn physical_adapter_rejects_simulator_mutations_without_writing_them() {
        let (mut gateway, context) = harness();
        assert_eq!(connect(&mut gateway)["ok"], true);
        for command in ["input", "presence", "add", "configure", "start", "stop"] {
            let result = request(&mut gateway, command, json!({"moduleId":MODULE}));
            assert_eq!(result["ok"], false);
            assert!(result["error"].as_str().unwrap().contains("unavailable"));
        }
        assert_eq!(context.lock().unwrap().writes.len(), 1);
        let result = request(&mut gateway, "disconnect", json!({}));
        assert_eq!(result["ok"], true);
        assert_eq!(context.lock().unwrap().state["running"], true);
    }
    #[test]
    fn service_shutdown_is_bounded_and_releases_serial_without_stop_command() {
        let (_, context) = harness();
        let service = Stm32Service::with_factory(Box::new(MockFactory(context.clone())), timing());
        let result = service.request(json!({"command":"connect","port":PATH,"clientId":CLIENT}));
        assert_eq!(result["ok"], true, "{result}");
        let start = Instant::now();
        service.shutdown();
        assert!(start.elapsed() < Duration::from_secs(1));
        assert_eq!(context.lock().unwrap().state["running"], true);
        assert_eq!(
            service.request(json!({"command":"status","clientId":CLIENT}))["ok"],
            false
        );
    }
}
