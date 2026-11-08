use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    time::{Duration, Instant},
};
const RETRY: Duration = Duration::from_secs(15);

fn candidate_port(row: &Value) -> Option<&str> {
    // ESP32 devices and common USB bridges still require a nonce-bound probe.
    if ![0x303a, 0x1a86, 0x10c4, 0x0403].contains(&row["vid"].as_u64().unwrap_or(0)) {
        return None;
    }
    row["port"].as_str()
}

#[derive(Default)]
pub(crate) struct Detection {
    attempted: BTreeMap<String, Instant>,
}
impl Detection {
    pub(crate) fn candidates(&mut self, rows: &[Value]) -> Vec<String> {
        self.candidates_at(rows, Instant::now())
    }
    fn candidates_at(&mut self, rows: &[Value], now: Instant) -> Vec<String> {
        let present: BTreeSet<_> = rows.iter().filter_map(|p| p["port"].as_str()).collect();
        self.attempted
            .retain(|port, _| present.contains(port.as_str()));
        let mut result = Vec::new();
        for row in rows {
            if result.len() >= 4 {
                break;
            }
            let Some(port) = candidate_port(row) else {
                continue;
            };
            if self
                .attempted
                .get(port)
                .is_some_and(|at| now.saturating_duration_since(*at) < RETRY)
            {
                continue;
            }
            self.attempted.insert(port.into(), now);
            result.push(port.to_owned());
        }
        result
    }

    pub(crate) fn next_retry(&self, pending: &[Value]) -> Option<Duration> {
        self.next_retry_at(pending, Instant::now())
    }

    fn next_retry_at(&self, pending: &[Value], now: Instant) -> Option<Duration> {
        pending.iter().filter_map(candidate_port).map(|port| {
            self.attempted.get(port).map_or(Duration::ZERO, |at| RETRY.saturating_sub(now.saturating_duration_since(*at)))
        }).min()
    }
}
fn paired_identity(
    value: &Value,
    nonce: &str,
    paired: &[Value],
    direct_only: bool,
) -> Option<String> {
    let id = value["companionId"].as_str()?;
    (value["requestId"] == nonce
        && value["protocolVersion"] == 2
        && value["deviceType"] == "esp32"
        && value["setupSaved"] == true
        && (!direct_only || (value["connectionMode"] == "serial" && value["statusEvents"] == true))
        && paired.iter().any(|client| client["companionId"] == id))
    .then(|| id.to_owned())
}
pub(crate) fn probe(path: &str, paired: &[Value]) -> Result<Option<String>, String> {
    inspect_rates(path, paired, true)
}
pub(crate) fn inspect(path: &str, paired: &[Value]) -> Result<Option<String>, String> {
    inspect_rates(path, paired, false)
}
fn inspect_rates(path: &str, paired: &[Value], direct_only: bool) -> Result<Option<String>, String> {
    let mut port = serialport::new(path, 115200)
        .timeout(Duration::from_millis(40))
        .open()
        .map_err(|e| e.to_string())?;
    let _ = port.write_data_terminal_ready(false);
    let _ = port.write_request_to_send(false);
    for baud in [115200, 921600] {
        port.set_baud_rate(baud).map_err(|e| e.to_string())?;
        let _ = port.clear(serialport::ClearBuffer::Input);
        if let Some(id) = probe_stream(&mut port, paired, Duration::from_millis(800), direct_only)? {
            return Ok(Some(id));
        }
    }
    Ok(None)
}
fn probe_stream(
    port: &mut impl ReadWrite,
    paired: &[Value],
    budget: Duration,
    direct_only: bool,
) -> Result<Option<String>, String> {
    let nonce = uuid::Uuid::new_v4().to_string();
    let mut request = serde_json::to_vec(&json!({"command":"status","requestId":nonce}))
        .map_err(|e| e.to_string())?;
    // A probe at another baud can leave an incomplete record in the board's
    // line buffer. Terminate it before sending the nonce-bound status request.
    request.insert(0,b'\n');
    request.push(b'\n');
    port.write_all(&request).map_err(|e| e.to_string())?;
    let start = Instant::now();
    let mut line = Vec::new();
    let mut overflow = false;
    let mut bytes = [0u8; 512];
    while start.elapsed() < budget {
        match port.read(&mut bytes) {
            Ok(0) => break,
            Ok(n) => {
                for b in &bytes[..n] {
                    if *b == b'\n' {
                        if !overflow {
                            if let Ok(value) = serde_json::from_slice::<Value>(&line) {
                                if let Some(id) =
                                    paired_identity(&value, &nonce, paired, direct_only)
                                {
                                    return Ok(Some(id));
                                }
                            }
                        }
                        line.clear();
                        overflow = false;
                    } else if line.len() < 8192 {
                        line.push(*b)
                    } else {
                        overflow = true
                    }
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(None)
}
trait ReadWrite: Read + Write {}
impl<T: Read + Write> ReadWrite for T {}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_saved_paired_direct_companions_are_eligible() {
        let paired = vec![json!({"companionId":"screen"})];
        let valid = json!({"requestId":"fresh","protocolVersion":2,"deviceType":"esp32","setupSaved":true,"connectionMode":"serial","companionId":"screen","statusEvents":true});
        assert_eq!(
            paired_identity(&valid, "fresh", &paired, true).as_deref(),
            Some("screen")
        );
        let mut wifi = valid.clone();
        wifi["connectionMode"] = json!("wifi");
        assert!(paired_identity(&wifi, "fresh", &paired, false).is_some());
        let mut legacy = valid.clone();
        legacy.as_object_mut().unwrap().remove("statusEvents");
        assert!(paired_identity(&legacy, "fresh", &paired, true).is_none());
        assert!(paired_identity(&legacy, "fresh", &paired, false).is_some(),
            "Explicit firmware update preparation can still identify the paired board");
        for (key, value) in [
            ("requestId", json!("stale")),
            ("companionId", json!("stranger")),
            ("setupSaved", json!(false)),
            ("connectionMode", json!("wifi")),
            ("deviceType", json!("gateway")),
            ("protocolVersion", json!(1)),
            ("statusEvents", json!(false)),
        ] {
            let mut invalid = valid.clone();
            invalid[key] = value;
            assert!(paired_identity(&invalid, "fresh", &paired, true).is_none());
        }
        assert!(paired_identity(&valid, "fresh", &[], true).is_none());
    }
    #[test]
    fn retry_waits_only_for_pending_candidates_and_keeps_batch_capacity() {
        let mut detection = Detection::default();
        let now = Instant::now();
        let mut rows: Vec<_> = (1..=5).map(|port| json!({"port":format!("COM{port}"),"vid":0x303a})).collect();
        rows.push(json!({"port":"COM99","vid":0xffff}));
        assert_eq!(detection.candidates_at(&rows, now).len(), 4);
        assert_eq!(detection.next_retry_at(&rows, now), Some(Duration::ZERO));
        assert_eq!(detection.candidates_at(&rows, now), vec!["COM5"]);
        assert_eq!(detection.next_retry_at(&rows, now + Duration::from_secs(4)), Some(Duration::from_secs(11)));
        assert_eq!(detection.next_retry_at(&rows[5..], now), None);
        assert_eq!(detection.next_retry_at(&[], now), None);
        let mut detection = Detection::default();
        assert_eq!(detection.next_retry(&rows), Some(Duration::ZERO));
        assert_eq!(detection.candidates(&rows).len(), 4);
        assert_eq!(detection.candidates(&rows), vec!["COM5"]);
        assert!(detection.next_retry(&rows).is_some_and(|delay| delay > Duration::ZERO && delay <= RETRY));
        assert_eq!(detection.next_retry(&[]), None);
    }

    #[test]
    fn redetection_handles_restart_unplug_replug_and_changed_com_ports() {
        let mut detection = Detection::default();
        let now = Instant::now();
        let rows = vec![
            json!({"port":"COM5","vid":0x1a86}),
            json!({"port":"COM9","vid":0x9999}),
        ];
        assert_eq!(detection.candidates_at(&rows, now), vec!["COM5"]);
        assert!(detection
            .candidates_at(&rows, now + Duration::from_secs(2))
            .is_empty());
        assert_eq!(
            detection.candidates_at(&rows, now + Duration::from_secs(16)),
            vec!["COM5"]
        );
        detection.candidates_at(&[], now + Duration::from_secs(17));
        assert_eq!(
            detection.candidates_at(&rows, now + Duration::from_secs(18)),
            vec!["COM5"]
        );
        assert_eq!(Detection::default().candidates_at(&rows, now), vec!["COM5"]);
        assert_eq!(
            detection.candidates_at(&[json!({"port":"COM7","vid":0x303a})], now),
            vec!["COM7"]
        );
    }
    struct Device {
        data: std::io::Cursor<Vec<u8>>,
        wrong: bool,
        writes: Vec<Value>,
    }
    impl Write for Device {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            assert_eq!(bytes.first(),Some(&b'\n'));
            let request: Value = serde_json::from_slice(bytes).unwrap();
            self.writes.push(request.clone());
            let mut reply = json!({"requestId":request["requestId"],"protocolVersion":2,"deviceType":"esp32","setupSaved":true,"connectionMode":"serial","statusEvents":true,"companionId":if self.wrong{"stranger"}else{"screen"}});
            reply["status"] = json!("offline");
            let mut data = serde_json::to_vec(&reply).unwrap();
            data.push(b'\n');
            self.data = std::io::Cursor::new(data);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Read for Device {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            self.data.read(b)
        }
    }
    #[test]
    fn discovery_only_sends_status_and_rejects_foreign_device() {
        for wrong in [false, true] {
            let mut device = Device {
                data: std::io::Cursor::new(vec![]),
                wrong,
                writes: vec![],
            };
            let id = probe_stream(
                &mut device,
                &[json!({"companionId":"screen"})],
                Duration::from_secs(1),
                true,
            )
            .unwrap();
            assert_eq!(id.is_some(), !wrong);
            assert_eq!(device.writes.len(), 1);
            assert_eq!(device.writes[0]["command"], "status");
        }
    }
}
