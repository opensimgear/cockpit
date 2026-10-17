use super::*;
use std::{
    collections::VecDeque,
    io::{Read, Write},
    sync::Mutex,
    time::Duration,
};
fn port() -> PortInfo {
    PortInfo {
        path: "COM8".into(),
        serial_number: Some("00112233445566778899AABB".into()),
        vendor_id: Some("1209".into()),
        product_id: Some("0001".into()),
        manufacturer: Some("OpenSimGear".into()),
        friendly_name: Some("Device Template v0.1.0".into()),
    }
}
fn config() -> Value {
    serde_json::json!({"generation":1,"savedGeneration":1,"owner":0,"claimExpiresMs":0,"dirty":false,"pendingApply":false,"view":"active","valid":true,"config":{"name":"Panel with spaces","expanderMask":0,"buttons":[[0,0,0]],"axes":[],"hats":[],"encoders":[],"selectors":[]},"layout":{"buttons":1,"axes":0,"hats":0,"reportBytes":1},"descriptorHex":"0501"})
}
struct Script {
    bytes: VecDeque<u8>,
    writes: Arc<Mutex<Vec<u8>>>,
    chunk: usize,
}
impl Read for Script {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.bytes.is_empty() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let count = out.len().min(self.chunk).min(self.bytes.len());
        for byte in &mut out[..count] {
            *byte = self.bytes.pop_front().unwrap();
        }
        Ok(count)
    }
}
impl Write for Script {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writes.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Connection for Script {
    fn set_control_lines(&mut self, _: bool, _: bool) -> Result<(), String> {
        Ok(())
    }
}
fn console(bytes: Vec<u8>) -> (Console, Arc<Mutex<Vec<u8>>>) {
    let writes = Arc::new(Mutex::new(vec![]));
    (
        Console::new(
            Box::new(Script {
                bytes: bytes.into(),
                writes: writes.clone(),
                chunk: 7,
            }),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap(),
        writes,
    )
}
#[test]
fn candidates_and_role_identity_require_safe_metadata_and_current_firmware_uid() {
    let good = port();
    assert!(candidate(&good));
    for mutate in ["gateway", "pico", "path", "manufacturer", "serial", "ids"] {
        let mut bad = good.clone();
        match mutate {
            "gateway" => bad.friendly_name = Some("OpenSimGear CAN Gateway".into()),
            "pico" => bad.serial_number = Some("OSG-PICO-0123456789ABCDEF".into()),
            "path" => bad.path = "/tmp/unrelated".into(),
            "manufacturer" => bad.manufacturer = Some("Other".into()),
            "serial" => bad.serial_number = Some("0".repeat(24)),
            _ => bad.vendor_id = None,
        };
        assert!(!candidate(&bad));
    }
    let line="model=device-template board=0.1.0 profile=Panel with spaces module=4f53474400112233445566778899aabb protocol=3 buttons=1 axes=0 hats=0";
    let id = identity(line, &good).unwrap();
    assert_eq!(id.profile, "Panel with spaces");
    assert_eq!(id.module_id, "4f53474400112233445566778899aabb");
    for bad in [
        line.replace("model=device-template", "model=stm32-can-gateway"),
        line.replace("protocol=3", "protocol=4"),
        line.replace(
            "4f53474400112233445566778899aabb",
            "4f53474400112233445566778899aabc",
        ),
        line.replace("buttons=1", "buttons=129"),
        line.replace(" hats=0", ""),
    ] {
        assert!(identity(&bad, &good).is_err());
    }
    let zero = line.replace("buttons=1", "buttons=0");
    assert_eq!(identity(&zero, &good).unwrap().button_count, 0);
}
#[test]
fn status_carries_nonzero_boot_even_without_a_can_route() {
    let line = "can=cyphal node=0 route=0 lease=0 boot=100 usb=1 rx=0 tx=0 rejected=0 recoveries=0";
    let state = status(line).unwrap();
    assert_eq!(state.boot, 100);
    assert_eq!(state.route, 0);
    for bad in [
        line.replace("boot=100", "boot=0"),
        line.replace(" boot=100", ""),
        line.replace("can=cyphal", "can=unknown"),
        format!("{line} boot=101"),
    ] {
        assert!(status(&bad).is_err());
    }
}
#[test]
fn config_layout_and_generation_are_not_guessed() {
    let valid = config();
    validate_config(&valid).unwrap();
    for (pointer, value) in [
        ("/generation", serde_json::json!(0)),
        ("/claimExpiresMs", serde_json::json!(u64::MAX)),
        ("/layout/buttons", serde_json::json!(2)),
        ("/layout/reportBytes", serde_json::json!(0)),
        ("/valid", serde_json::json!("yes")),
        ("/descriptorHex", serde_json::json!("XX")),
    ] {
        let mut invalid = valid.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        assert!(validate_config(&invalid).is_err());
    }
    let mut zero = valid;
    zero["config"]["buttons"] = serde_json::json!([]);
    zero["layout"]["buttons"] = serde_json::json!(0);
    zero["layout"]["reportBytes"] = serde_json::json!(0);
    zero["descriptorHex"] = serde_json::json!("");
    validate_config(&zero).unwrap();
}
#[test]
fn fragmented_error_ack_is_drained_before_the_next_bare_json_response() {
    let bytes = format!(
        "{CONSOLE_BANNER}\r\nERR writer_busy\n{}\r\n{{\"ok\":true,\"schemaVersion\":1}}\r\n",
        config()
    )
    .into_bytes();
    let (mut console, writes) = console(bytes);
    let Reply::Config { error, snapshot } = console
        .exchange(
            "config claim 12",
            ReplyKind::Config,
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(error.as_deref(), Some("writer_busy"));
    assert_eq!(snapshot, config());
    console.finish_handshake();
    assert!(matches!(
        console
            .exchange(
                "action state",
                ReplyKind::Json,
                Instant::now() + Duration::from_secs(1)
            )
            .unwrap(),
        Reply::Json(_)
    ));
    assert_eq!(&*writes.lock().unwrap(), b"config claim 12\naction state\n");
}
#[test]
fn invalid_commands_never_write_and_bad_or_late_frames_are_rejected() {
    let (mut invalid, writes) = console(vec![]);
    for command in ["config get\nreset", &"x".repeat(384), "config é"] {
        assert!(invalid
            .exchange(
                command,
                ReplyKind::Json,
                Instant::now() + Duration::from_millis(2)
            )
            .is_err());
    }
    assert!(writes.lock().unwrap().is_empty());
    for bytes in [
        b"not-json\n".to_vec(),
        vec![b'x'; MAX_JSON_BYTES + 2],
        b"OK\n{bad}\n".to_vec(),
    ] {
        let (mut bad, _) = console(bytes);
        assert!(bad
            .exchange(
                "config get",
                ReplyKind::Config,
                Instant::now() + Duration::from_millis(3)
            )
            .is_err());
    }
    let (mut late, _) = console(format!("{CONSOLE_BANNER}\r\n").into_bytes());
    late.finish_handshake();
    assert!(late
        .exchange(
            "info",
            ReplyKind::Text,
            Instant::now() + Duration::from_millis(3)
        )
        .unwrap_err()
        .contains("restarted"));
    let (mut missing, writes) = console(vec![]);
    assert!(missing
        .exchange(
            "config save 12",
            ReplyKind::Config,
            Instant::now() + Duration::from_millis(3)
        )
        .is_err());
    assert_eq!(&*writes.lock().unwrap(), b"config save 12\n");
}

#[test]
fn explicit_candidate_requires_usb_uid_and_excludes_known_serial_roles() {
    let mut generic = port();
    generic.manufacturer = Some("(Standard port types)".into());
    generic.friendly_name = Some("USB Serial Device (COM8)".into());
    assert!(!candidate(&generic));
    assert!(candidate_for_explicit(&generic));
    for change in [
        "gateway",
        "companion",
        "pico",
        "zero",
        "ffff",
        "path",
        "vid",
        "pid",
    ] {
        let mut bad = generic.clone();
        match change {
            "gateway" => bad.friendly_name = Some("CAN Gateway".into()),
            "companion" => bad.manufacturer = Some("OpenSimGear Companion".into()),
            "pico" => bad.serial_number = Some("OSG-PICO-0123456789abcdef".into()),
            "zero" => bad.serial_number = Some("0".repeat(24)),
            "ffff" => bad.serial_number = Some("f".repeat(24)),
            "path" => bad.path = "/tmp/serial".into(),
            "vid" => bad.vendor_id = Some("0000".into()),
            _ => bad.product_id = None,
        }
        assert!(!candidate_for_explicit(&bad), "{change}");
    }
}

#[test]
fn only_exact_complete_negative_wire_reply_is_unsupported() {
    let (mut codec, _) = console(b"ERR unknown command\r\n".to_vec());
    assert!(matches!(
        codec.exchange(
            "action describe 0",
            ReplyKind::Json,
            Instant::now() + Duration::from_millis(10)
        ),
        Ok(Reply::Unsupported)
    ));
    for text in [
        "ERR unknown command extra\r\n",
        "ERR unknown command",
        "{bad JSON}\r\n",
        "\"ERR unknown command\"\r\n",
    ] {
        let (mut codec, _) = console(text.as_bytes().to_vec());
        assert!(codec
            .exchange(
                "events state",
                ReplyKind::Json,
                Instant::now() + Duration::from_millis(2)
            )
            .is_err());
    }
}
