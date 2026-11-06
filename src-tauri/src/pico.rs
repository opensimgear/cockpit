//! Current Pico Debug gateway. Discovery does not open the serial port.
use crate::{
    protocol::identifier,
    transport::{Connection, Factory, NativeFactory, PortInfo},
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
mod workspace;

const CLOSED: &str = "Cockpit native gateway service has closed.";
const NO_CONNECTION: &str = "Connect the flashed Pico gateway first.";
const RESPONSE_LIMIT: usize = 65_536;
pub(crate) const FIRMWARE_REQUIRED: &str =
    "Pico protocol 1 is unsupported. Flash current Pico Debug firmware (protocol 3) and reconnect.";

pub(crate) fn candidate(port: &PortInfo) -> bool {
    crate::transport::allowed_path(&port.path)
        && port.serial_number.as_deref().is_some_and(|serial| {
            serial
                .strip_prefix("OSG-PICO-")
                .is_some_and(|id| identifier(id, 4, 64))
        })
        && port
            .vendor_id
            .as_deref()
            .is_some_and(|id| id.trim_start_matches("0x").eq_ignore_ascii_case("cafe"))
        && port
            .product_id
            .as_deref()
            .is_some_and(|id| id.trim_start_matches("0x").eq_ignore_ascii_case("4010"))
}

#[derive(Clone)]
struct SharedFactory(Arc<Mutex<Box<dyn Factory>>>);
impl Factory for SharedFactory {
    fn gateway_duplex(&mut self, port: &PortInfo) -> Option<Result<crate::transport::duplex::DuplexParts, String>> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).gateway_duplex(port)
    }

    fn ports(&mut self) -> Result<Vec<PortInfo>, String> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).ports()
    }
    fn open(&mut self, port: &PortInfo, timeout: Duration) -> Result<Box<dyn Connection>, String> {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .open(port, timeout)
    }
}
/// Pico connections require current Debug firmware and the shared CDC3 adapter.
pub struct PicoService {
    factory: SharedFactory,
    selected: Mutex<Option<crate::stm32::Stm32Service>>,
    closed: AtomicBool,
    endpoint_activity: Arc<crate::output_leases::LeaseActivity>,
    workspace: Arc<crate::stm32::workspace::Cache>,
}
impl Default for PicoService {
    fn default() -> Self {
        Self::new()
    }
}
impl PicoService {
    pub(crate) fn output_activity(&self)->bool{self.endpoint_activity.active()}
    pub(crate) fn configuration_claims(&self) -> Vec<crate::module_configuration_routes::WriterPin> {
        match self.selected.lock().unwrap_or_else(|p|p.into_inner()).as_ref() {
            Some(service) => service.configuration_claims(),
            _ => vec![],
        }
    }
    pub(crate) fn cached_state(&self) -> Value {
        match self.selected.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            Some(service) => service.cached_state(),
            None => empty_state(),
        }
    }
    pub(crate) fn try_cached_state(&self) -> Option<Value> {
        let selected = match self.selected.try_lock() {
            Ok(selected) => selected,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return None,
        };
        match selected.as_ref() {
            Some(service) => service.try_cached_state(),
            None => Some(empty_state()),
        }
    }

    pub fn new() -> Self {
        Self::with_factory(Box::new(NativeFactory))
    }
    fn with_factory(factory: Box<dyn Factory>) -> Self {
        Self {
            factory: SharedFactory(Arc::new(Mutex::new(factory))),
            selected: Mutex::new(None),
            closed: AtomicBool::new(false),
            endpoint_activity:Arc::default(),
            workspace: Arc::new(crate::stm32::workspace::Cache::new(empty_state())),
        }
    }
    fn probe(&self, request: &Value) -> Result<(), String> {
        let mut factory = self.factory.clone();
        let port = factory
            .ports()?
            .into_iter()
            .find(|port| candidate(port) && request["port"] == port.path)
            .ok_or("Select an available flashed Pico gateway port.")?;
        let serial = port.serial_number.as_deref().unwrap();
        if request["gatewayId"]
            .as_str()
            .is_some_and(|id| id != format!("pico:{serial}"))
        {
            return Err("Gateway identity does not match the selected port.".into());
        }
        let mut connection = factory.open(&port, Duration::from_millis(50))?;
        let result = (|| {
            connection.set_control_lines(true, false)?;
            connection
                .write_all(b"hello\n")
                .map_err(|error| error.to_string())?;
            connection.flush().map_err(|error| error.to_string())?;
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut buffer = Vec::new();
            while Instant::now() < deadline {
                if self.closed.load(Ordering::Acquire) {
                    return Err(CLOSED.into());
                }
                while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
                    let line: Vec<_> = buffer.drain(..=newline).collect();
                    let line = std::str::from_utf8(&line)
                        .map_err(|_| "Pico sent malformed handshake data.")?
                        .trim();
                    if line.starts_with("gatewayType=") {
                        let fields: HashMap<_, _> = line
                            .split_whitespace()
                            .filter_map(|field| field.split_once('='))
                            .collect();
                        let uid = format!(
                            "00000000{}",
                            serial
                                .strip_prefix("OSG-PICO-")
                                .unwrap()
                                .to_ascii_lowercase()
                        );
                        if fields.get("gatewayType") == Some(&"pico-debug-gateway")
                            && fields.get("protocol") == Some(&"3")
                            && fields.get("canProtocol") == Some(&"3")
                            && fields.get("serial") == Some(&serial)
                            && fields
                                .get("identity")
                                .is_some_and(|identity| identity.eq_ignore_ascii_case(&uid))
                            && fields.get("debug") == Some(&"1")
                            && fields.get("maxVirtual") == Some(&"8")
                        {
                            return Ok(());
                        }
                        return Err(
                            "Unsupported Pico gateway handshake; check its debug firmware.".into(),
                        );
                    }
                    if line.starts_with('{') {
                        let reply: Value = serde_json::from_str(line)
                            .map_err(|_| "Pico sent malformed handshake data.")?;
                        if reply["state"]["protocolVersion"] == 1 {
                            return Err(FIRMWARE_REQUIRED.into());
                        }
                        return Err("Unsupported Pico gateway handshake.".into());
                    }
                }
                let mut bytes = [0; 4096];
                match connection.read(&mut bytes) {
                    Ok(0) => return Err("Pico was unplugged during its handshake.".into()),
                    Ok(count) if buffer.len() + count <= RESPONSE_LIMIT => {
                        buffer.extend_from_slice(&bytes[..count])
                    }
                    Ok(_) => return Err("Pico handshake exceeds its frame limit.".into()),
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::TimedOut
                                | io::ErrorKind::WouldBlock
                                | io::ErrorKind::Interrupted
                        ) => {}
                    Err(error) => return Err(error.to_string()),
                }
            }
            Err("Pico did not return a supported firmware handshake.".into())
        })();
        let _ = connection.set_control_lines(false, false);
        result
    }
    pub fn request(&self, request: Value) -> Value {
        let mut selected = self.selected.lock().unwrap_or_else(|p| p.into_inner());
        let error = |message: String| json!({"ok":false,"error":message,"state":empty_state()});
        if self.closed.load(Ordering::Acquire) {
            return error(CLOSED.into());
        }
        if !request.is_object()
            || !request["clientId"]
                .as_str()
                .is_some_and(|id| identifier(id, 16, 80))
        {
            return error("Missing Cockpit client identity. Reload Cockpit.".into());
        }
        if request["command"] == "connect" {
            if let Some(service) = selected.as_ref() {
                return service.request(request);
            }
            match self.probe(&request) {
                Ok(()) => {
                    *selected = Some(crate::stm32::Stm32Service::pico_debug_with_workspace(
                        Box::new(self.factory.clone()),
                        self.endpoint_activity.clone(),
                        self.workspace.clone(),
                    ));
                }
                Err(message) => return error(message),
            }
        }
        if let Some(service) = selected.as_ref() {
            let disconnect = request["command"] == "disconnect";
            let response = service.request(request);
            if disconnect && response["ok"] == true {
                if let Some(service) = selected.take() {
                    service.shutdown();
                }
            }
            return response;
        }
        if matches!(request["command"].as_str(), Some("ports" | "status")) {
            return match self.factory.clone().ports() {
                Ok(ports) => {
                    let mut state = empty_state();
                    state["ports"] = json!(ports
                        .iter()
                        .filter(|port| candidate(port))
                        .map(PortInfo::value)
                        .collect::<Vec<_>>());
                    self.workspace.commit(state.clone(), Default::default());
                    json!({"ok":true,"state":state})
                }
                Err(message) => error(message),
            };
        }
        error(NO_CONNECTION.into())
    }
    pub fn shutdown(&self) {
        self.closed.store(true, Ordering::Release);
        if let Some(service) = self
            .selected
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            service.shutdown();
        }
    }
}
impl Drop for PicoService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn empty_state() -> Value {
    json!({"backend":"pico","connected":false,"running":false,
        "needsReconnect":false,"reconnecting":false,"modules":[],"port":null,"ports":[]})
}

#[cfg(test)]
pub(crate) use tests::stream_fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    const PATH: &str = "COM42";
    const BANNER: &str = "gatewayType=pico-debug-gateway protocol=3 canProtocol=3 firmware=0.1.0 board=pico_w identity=000000000123456789abcdef serial=OSG-PICO-0123456789abcdef epoch=1 maxModules=32 debug=1 maxVirtual=8\r\n";

    fn info(path: &str) -> PortInfo {
        PortInfo {
            path: path.into(),
            serial_number: Some("OSG-PICO-0123456789abcdef".into()),
            vendor_id: Some("cafe".into()),
            product_id: Some("4010".into()),
            manufacturer: None,
            friendly_name: None,
        }
    }

    struct ProbeFactory {
        response: Vec<u8>,
        duplex: Option<crate::transport::duplex::DuplexParts>,
    }
    struct ProbeConnection {
        response: std::collections::VecDeque<u8>,
    }
    impl Factory for ProbeFactory {
        fn gateway_duplex(&mut self, _: &PortInfo) -> Option<Result<crate::transport::duplex::DuplexParts, String>> {
            self.duplex.take().map(Ok)
        }
        fn ports(&mut self) -> Result<Vec<PortInfo>, String> {
            Ok(vec![info(PATH)])
        }
        fn open(&mut self, _: &PortInfo, _: Duration) -> Result<Box<dyn Connection>, String> {
            Ok(Box::new(ProbeConnection {
                response: self.response.iter().copied().collect(),
            }))
        }
    }
    pub(crate) fn stream_fixture(parts: crate::transport::duplex::DuplexParts) -> PicoService {
        PicoService::with_factory(Box::new(ProbeFactory {
            response: BANNER.as_bytes().to_vec(),
            duplex: Some(parts),
        }))
    }
    impl Read for ProbeConnection {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            let count = bytes.len().min(self.response.len()).min(11);
            for byte in &mut bytes[..count] {
                *byte = self.response.pop_front().unwrap();
            }
            Ok(count)
        }
    }
    impl Write for ProbeConnection {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            assert_eq!(bytes, b"hello\n");
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl Connection for ProbeConnection {
        fn set_control_lines(&mut self, _: bool, _: bool) -> Result<(), String> {
            Ok(())
        }
    }
    #[test]
    fn modern_probe_checks_role_version_and_serial_identity_before_selection() {
        let banner = BANNER;
        for (response, accepted) in [
            (banner.to_owned(), true),
            (banner.replace("protocol=3 ", "protocol=2 "), false),
            (
                banner.replace("pico-debug-gateway", "stm32-can-gateway"),
                false,
            ),
            (
                banner.replace(
                    "identity=000000000123456789abcdef",
                    "identity=00000000123456789abcdef0",
                ),
                false,
            ),
            (banner.replace("maxVirtual=8", "maxVirtual=9"), false),
            ("{broken-json\n".into(), false),
            ("unsupported\n".into(), false),
        ] {
            let service = PicoService::with_factory(Box::new(ProbeFactory {
                response: response.into_bytes(),
                duplex: None,
            }));
            let request = json!({"port":PATH,"gatewayId":"pico:OSG-PICO-0123456789abcdef"});
            assert_eq!(service.probe(&request).ok(), accepted.then_some(()));
        }
    }

    #[test]
    fn protocol_one_is_rejected_before_a_gateway_worker_is_created() {
        let service = PicoService::with_factory(Box::new(ProbeFactory {
            response: br#"{"ok":true,"state":{"protocolVersion":1}}
"#.to_vec(),
            duplex: None,
        }));
        let result = service.request(json!({"command":"connect","port":PATH,"clientId":"pico-probe-test-client"}));
        assert_eq!(result["ok"], false);
        assert_eq!(result["error"], FIRMWARE_REQUIRED);
        assert!(service.selected.lock().unwrap().is_none());
        assert_eq!(service.cached_state()["connected"], false);
        assert!(service.configuration_claims().is_empty());
    }
}
