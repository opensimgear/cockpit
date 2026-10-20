use super::{remaining_timeout, send};
use serde_json::{json, Value};
use serialport::{ClearBuffer, DataBits, FlowControl, Parity, SerialPort, StopBits};
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone, Debug)]
enum WriteStep {
    Take(usize),
    Fail,
    Zero,
}

struct Port {
    baud: u32,
    timeout: Duration,
    timeouts: Vec<Duration>,
    fail_timeout_calls: Vec<usize>,
    calls: Vec<Vec<u8>>,
    wire: Vec<u8>,
    steps: VecDeque<WriteStep>,
    emulate_wire_timeout: bool,
}
impl Default for Port {
    fn default() -> Self {
        Self {
            baud: 115_200,
            timeout: Duration::from_millis(10),
            timeouts: vec![],
            fail_timeout_calls: vec![],
            calls: vec![],
            wire: vec![],
            steps: VecDeque::new(),
            emulate_wire_timeout: false,
        }
    }
}
impl Read for Port {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::ErrorKind::TimedOut.into())
    }
}
impl Write for Port {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.calls.push(bytes.to_vec());
        match self.steps.pop_front() {
            Some(WriteStep::Fail) => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "fixture USB write timed out",
                ))
            }
            Some(WriteStep::Zero) => return Ok(0),
            Some(WriteStep::Take(count)) => {
                let count = count.min(bytes.len());
                self.wire.extend_from_slice(&bytes[..count]);
                return Ok(count);
            }
            None => {}
        }
        if self.emulate_wire_timeout {
            // Windows can put an unknown prefix on the wire before WriteFile
            // reports timeout. The caller must never replay the whole record.
            let capacity = (u128::from(self.baud) * self.timeout.as_millis() / 10_000) as usize;
            if capacity < bytes.len() {
                self.wire.extend_from_slice(&bytes[..capacity]);
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "fixture short serial timeout",
                ));
            }
        }
        self.wire.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl SerialPort for Port {
    fn name(&self) -> Option<String> {
        Some("fixture-port".into())
    }
    fn baud_rate(&self) -> serialport::Result<u32> {
        Ok(self.baud)
    }
    fn data_bits(&self) -> serialport::Result<DataBits> {
        Ok(DataBits::Eight)
    }
    fn flow_control(&self) -> serialport::Result<FlowControl> {
        Ok(FlowControl::None)
    }
    fn parity(&self) -> serialport::Result<Parity> {
        Ok(Parity::None)
    }
    fn stop_bits(&self) -> serialport::Result<StopBits> {
        Ok(StopBits::One)
    }
    fn timeout(&self) -> Duration {
        self.timeout
    }
    fn set_baud_rate(&mut self, baud: u32) -> serialport::Result<()> {
        self.baud = baud;
        Ok(())
    }
    fn set_data_bits(&mut self, _: DataBits) -> serialport::Result<()> {
        Ok(())
    }
    fn set_flow_control(&mut self, _: FlowControl) -> serialport::Result<()> {
        Ok(())
    }
    fn set_parity(&mut self, _: Parity) -> serialport::Result<()> {
        Ok(())
    }
    fn set_stop_bits(&mut self, _: StopBits) -> serialport::Result<()> {
        Ok(())
    }
    fn set_timeout(&mut self, timeout: Duration) -> serialport::Result<()> {
        self.timeouts.push(timeout);
        if self.fail_timeout_calls.contains(&self.timeouts.len()) {
            return Err(serialport::Error::new(
                serialport::ErrorKind::Unknown,
                "fixture timeout configuration failed",
            ));
        }
        self.timeout = timeout;
        Ok(())
    }
    fn write_request_to_send(&mut self, _: bool) -> serialport::Result<()> {
        Ok(())
    }
    fn write_data_terminal_ready(&mut self, _: bool) -> serialport::Result<()> {
        Ok(())
    }
    fn read_clear_to_send(&mut self) -> serialport::Result<bool> {
        Ok(true)
    }
    fn read_data_set_ready(&mut self) -> serialport::Result<bool> {
        Ok(true)
    }
    fn read_ring_indicator(&mut self) -> serialport::Result<bool> {
        Ok(false)
    }
    fn read_carrier_detect(&mut self) -> serialport::Result<bool> {
        Ok(true)
    }
    fn bytes_to_read(&self) -> serialport::Result<u32> {
        Ok(0)
    }
    fn bytes_to_write(&self) -> serialport::Result<u32> {
        Ok(0)
    }
    fn clear(&self, _: ClearBuffer) -> serialport::Result<()> {
        Ok(())
    }
    fn try_clone(&self) -> serialport::Result<Box<dyn SerialPort>> {
        Err(serialport::Error::new(
            serialport::ErrorKind::Unknown,
            "fixture clone unavailable",
        ))
    }
    fn set_break(&self) -> serialport::Result<()> {
        Ok(())
    }
    fn clear_break(&self) -> serialport::Result<()> {
        Ok(())
    }
}
fn status() -> Arc<Mutex<Value>> {
    Arc::new(Mutex::new(json!({})))
}
fn wire(value: &Value, resynchronize: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    if resynchronize {
        bytes.push(b'\n');
    }
    bytes.extend(serde_json::to_vec(value).unwrap());
    bytes.push(b'\n');
    bytes
}
fn io_status(status: &Arc<Mutex<Value>>) -> Value {
    status.lock().unwrap()["serialIo"].clone()
}

#[test]
fn slow_sunton_reply_gets_a_baud_budget_instead_of_the_short_read_timeout() {
    let value = json!({"command":"serial-reply","frame":{"feedback":"x".repeat(2048)}});
    let expected = wire(&value, false);
    let mut old = Port {
        emulate_wire_timeout: true,
        ..Default::default()
    };
    assert_eq!(
        old.write_all(&expected).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    assert!(!old.wire.is_empty() && old.wire.len() < expected.len());
    assert_ne!(old.wire.last(), Some(&b'\n'));
    let mut port = Port {
        emulate_wire_timeout: true,
        ..Default::default()
    };
    let state = status();
    send(&mut port, &value, &state, false).unwrap();
    assert_eq!(port.wire, expected);
    assert_eq!(port.timeout, Duration::from_millis(10));
    assert_eq!(port.timeouts.last(), Some(&Duration::from_millis(10)));
    let minimum = (expected.len() as u128 * 10_000).div_ceil(115_200) + 250;
    let io = io_status(&state);
    assert!(io["lastWriteTimeoutMs"].as_u64().unwrap() >= minimum as u64);
    assert!(io["lastWriteTimeoutMs"].as_u64().unwrap() <= 3000);
    assert_eq!(io["baudRate"], 115_200);
    assert_eq!(io["readTimeoutMs"], 10);
    assert_eq!(io["writesCompleted"], 1);
    assert_eq!(io["writeFailures"], 0);
    assert_eq!(io["lastWriteWrittenBytes"], expected.len());
}

#[test]
fn positive_partial_writes_continue_only_the_unsent_suffix_and_probes_have_one_prefix() {
    for resynchronize in [false, true] {
        let value = json!({"command":"status","requestId":"fixture"});
        let expected = wire(&value, resynchronize);
        let mut port = Port {
            steps: VecDeque::from([WriteStep::Take(3), WriteStep::Take(7)]),
            ..Default::default()
        };
        let state = status();
        send(&mut port, &value, &state, resynchronize).unwrap();
        assert_eq!(port.wire, expected);
        assert_eq!(
            port.calls,
            vec![
                expected.clone(),
                expected[3..].to_vec(),
                expected[10..].to_vec()
            ]
        );
        assert_eq!(port.timeout, Duration::from_millis(10));
        assert_eq!(serde_json::from_slice::<Value>(&port.wire).unwrap(), value);
        let io = io_status(&state);
        assert_eq!(io["writeAttempts"], 1);
        assert_eq!(io["writesCompleted"], 1);
        assert_eq!(io["partialWrites"], 2);
        assert_eq!(io["totalWrittenBytes"], expected.len());
        assert_eq!(io["lastWriteBytes"], expected.len());
        assert!(io["lastWriteError"].is_null());
    }
}

#[test]
fn failed_suffix_and_zero_writes_never_restart_or_replay_a_record() {
    let value = json!({"command":"provision-direct","secret":"native-secret-never-log"});
    let expected = wire(&value, false);
    for step in [WriteStep::Fail, WriteStep::Zero] {
        let mut port = Port {
            steps: VecDeque::from([WriteStep::Take(9), step]),
            ..Default::default()
        };
        let state = status();
        assert!(send(&mut port, &value, &state, false).is_err());
        assert_eq!(port.wire, expected[..9]);
        assert_eq!(port.calls, vec![expected.clone(), expected[9..].to_vec()]);
        assert_eq!(port.timeout, Duration::from_millis(10));
        let io = io_status(&state);
        assert_eq!(io["writeAttempts"], 1);
        assert_eq!(io["writesCompleted"], 0);
        assert_eq!(io["writeFailures"], 1);
        assert_eq!(io["lastWriteWrittenBytes"], 9);
        assert_eq!(io["lastWriteFailure"]["writtenBytes"], 9);
        assert_eq!(io["lastWriteFailure"]["bytes"], expected.len());
        let encoded = io.to_string();
        assert!(!encoded.contains("native-secret-never-log"));
        assert!(!encoded.contains("provision-direct"));
        assert!(io["lastWriteError"].is_string());
    }
}

#[test]
fn timeout_configuration_failure_attempts_to_restore_the_read_timeout_without_writing() {
    let mut port = Port {
        fail_timeout_calls: vec![1],
        ..Default::default()
    };
    let state = status();
    assert!(send(&mut port, &json!({"command":"status"}), &state, true).is_err());
    assert!(port.calls.is_empty() && port.wire.is_empty());
    assert_eq!(port.timeouts.len(), 2);
    assert_eq!(port.timeouts[1], Duration::from_millis(10));
    assert_eq!(port.timeout, Duration::from_millis(10));
    let io = io_status(&state);
    assert_eq!(io["writeFailures"], 1);
    assert_eq!(io["lastWriteWrittenBytes"], 0);
}

#[test]
fn failure_to_restore_timeout_is_reported_without_replaying_transmitted_bytes() {
    let value = json!({"command":"status"});
    let expected = wire(&value, true);
    let mut port = Port {
        fail_timeout_calls: vec![2],
        ..Default::default()
    };
    let state = status();
    assert!(send(&mut port, &value, &state, true).is_err());
    assert_eq!(port.wire, expected);
    assert_eq!(port.calls.len(), 1);
    assert_eq!(port.timeouts.last(), Some(&Duration::from_millis(10)));
    let io = io_status(&state);
    assert_eq!(io["writesCompleted"], 1);
    assert_eq!(io["writeFailures"], 1);
    assert_eq!(io["lastWriteWrittenBytes"], expected.len());
    assert!(io["lastWriteError"].is_string());
}

#[test]
fn malformed_or_oversized_records_never_reach_the_serial_port() {
    for value in [
        Value::Null,
        json!([]),
        json!("not an object"),
        json!({"secret":"oversize-secret".repeat(1024)}),
    ] {
        let mut port = Port::default();
        let state = status();
        assert!(send(&mut port, &value, &state, false).is_err());
        assert!(port.calls.is_empty() && port.wire.is_empty() && port.timeouts.is_empty());
        assert!(!state
            .lock()
            .unwrap()
            .to_string()
            .contains("oversize-secret"));
    }
}

#[test]
fn oversized_wire_deadline_is_rejected_and_later_success_preserves_failure_evidence() {
    let value = json!({"command":"status","padding":"x".repeat(7000)});
    let mut port = Port {
        baud: 1200,
        ..Default::default()
    };
    let state = status();
    assert!(send(&mut port, &value, &state, false)
        .unwrap_err()
        .contains("bounded write deadline"));
    let failed = io_status(&state);
    assert_eq!(failed["lastWriteTimeoutMs"], 0);
    assert_eq!(port.timeout, Duration::from_millis(10));
    assert!(port.calls.is_empty() && port.timeouts.is_empty() && port.wire.is_empty());
    port.baud = 115_200;
    send(&mut port, &json!({"command":"status"}), &state, true).unwrap();
    let current = io_status(&state);
    assert_eq!(current["writeAttempts"], 2);
    assert_eq!(current["writesCompleted"], 1);
    assert_eq!(current["writeFailures"], 1);
    assert_eq!(current["lastWriteFailure"], failed["lastWriteFailure"]);
    assert!(current["lastWriteError"].is_null());
}

#[test]
fn unknown_baud_never_writes_with_an_unbounded_deadline() {
    let mut port = Port {
        baud: 0,
        ..Default::default()
    };
    let state = status();
    assert!(send(&mut port, &json!({"command":"status"}), &state, false).is_err());
    assert!(port.calls.is_empty() && port.timeouts.is_empty() && port.wire.is_empty());
    assert_eq!(io_status(&state)["writeFailures"], 1);
}

#[test]
fn fractional_millisecond_remainder_never_disables_the_windows_write_timeout() {
    let deadline = Duration::from_millis(300);
    for elapsed in [
        deadline - Duration::from_nanos(999_999),
        deadline - Duration::from_nanos(1),
        deadline,
        deadline + Duration::from_nanos(1),
    ] {
        assert!(remaining_timeout(deadline, elapsed).is_err());
    }
    for remaining in [
        Duration::from_millis(1),
        Duration::from_nanos(1_999_999),
        deadline,
    ] {
        let configured = remaining_timeout(deadline, deadline - remaining).unwrap();
        assert_eq!(configured, remaining);
        assert!(configured.as_millis() >= 1);
    }
}
