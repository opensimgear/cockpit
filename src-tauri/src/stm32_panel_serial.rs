//! Splits a parked gateway USB reader into ordered panel records and command replies.
use crate::{
    stm32_panel_wire::{decode, Record},
    transport::duplex::{BlockingReceiver, CancelIo, DeadlineWriter, DuplexParts},
};
use serde_json::Value;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
    time::Instant,
};

const FRAME_LIMIT: usize = 65_536;
const RESPONSE_TIMEOUT: &str =
    "CAN gateway response timed out; connection and setup access were closed.";

pub(crate) struct GatewaySerial {
    writer: Box<dyn DeadlineWriter>,
    cancel: Arc<dyn CancelIo>,
    reader: Option<JoinHandle<()>>,
    replies: mpsc::Receiver<Result<Value, String>>,
    supports_panel: Arc<AtomicBool>,
}

impl GatewaySerial {
    pub(crate) fn open(
        parts: DuplexParts,
        mut panel: impl FnMut(Record) + Send + 'static,
    ) -> Result<Self, String> {
        let DuplexParts {
            receiver,
            mut writer,
            cancel,
        } = parts;
        writer.control_lines(true, false)?;
        let (send, replies) = mpsc::sync_channel(4);
        let supports_panel = Arc::new(AtomicBool::new(false));
        let reader_support = supports_panel.clone();
        let reader = thread::Builder::new()
            .name("osg-stm32-cdc-rx".into())
            .spawn(move || {
                let result = receive(
                    receiver,
                    |value| {
                        send.try_send(Ok(value))
                            .map_err(|_| "CAN gateway reply mailbox is unavailable.".to_owned())
                    },
                    &mut panel,
                    &reader_support,
                );
                panel(Record::Reset);
                if let Err(error) = result {
                    let _ = send.try_send(Err(error));
                }
            });
        match reader {
            Ok(reader) => Ok(Self {
                writer,
                cancel,
                reader: Some(reader),
                replies,
                supports_panel,
            }),
            Err(error) => {
                let _ = writer.control_lines(false, false);
                cancel.cancel();
                Err(format!("Could not start the CAN gateway reader: {error}"))
            }
        }
    }

    pub(crate) fn supports_panel(&self) -> bool {
        self.supports_panel.load(Ordering::Acquire)
    }

    pub(crate) fn send(&mut self, line: &[u8], deadline: Instant) -> Result<(), String> {
        self.writer.write_all(line, deadline)
    }

    pub(crate) fn reply(&self, id: u32, deadline: Instant) -> Result<Value, String> {
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(RESPONSE_TIMEOUT.into());
            }
            let value = self
                .replies
                .recv_timeout(remaining)
                .map_err(|error| match error {
                    mpsc::RecvTimeoutError::Timeout => RESPONSE_TIMEOUT,
                    mpsc::RecvTimeoutError::Disconnected => "CAN gateway reader stopped.",
                })??;
            if value["id"].as_u64() == Some(u64::from(id)) {
                return Ok(value);
            }
        }
    }
}

impl Drop for GatewaySerial {
    fn drop(&mut self) {
        let _ = self.writer.control_lines(false, false);
        self.cancel.cancel();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

pub(crate) fn receive(
    mut receiver: Box<dyn BlockingReceiver>,
    mut reply: impl FnMut(Value) -> Result<(), String>,
    mut panel: impl FnMut(Record),
    supports_panel: &AtomicBool,
) -> Result<(), String> {
    let mut pending = Vec::new();
    let mut chunk = [0; 4096];
    loop {
        let count = receiver.receive(&mut chunk)?;
        if count == 0 {
            return Err("CAN gateway was unplugged.".into());
        }
        for bytes in chunk[..count].split_inclusive(|byte| *byte == b'\n') {
            if pending.len() + bytes.len() > FRAME_LIMIT {
                return Err("CAN gateway serial frame exceeds 65536 bytes.".into());
            }
            pending.extend_from_slice(bytes);
            if !bytes.ends_with(b"\n") {
                continue;
            }
            let line = pending.strip_suffix(b"\n").unwrap_or(&pending);
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.starts_with(b"gatewayType=") {
                if line
                    .split(|byte| *byte == b' ')
                    .any(|word| word == b"panelStream=1")
                {
                    supports_panel.store(true, Ordering::Release);
                }
            } else if !line.iter().all(u8::is_ascii_whitespace) && line != b"type help for commands"
            {
                let value: Value = serde_json::from_slice(line)
                    .map_err(|_| "CAN gateway sent malformed serial data.".to_owned())?;
                if value.get("panelStream").is_some() {
                    panel(decode(&value).ok_or("CAN gateway sent an invalid panel record.")?);
                } else {
                    reply(value)?;
                }
            }
            pending.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::time::Duration;

    struct Fragments(VecDeque<&'static [u8]>);
    impl BlockingReceiver for Fragments {
        fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, String> {
            let Some(part) = self.0.pop_front() else {
                return Ok(0);
            };
            buffer[..part.len()].copy_from_slice(part);
            Ok(part.len())
        }
    }

    #[test]
    fn routes_interleaved_records_across_read_boundaries() {
        let input = Fragments(VecDeque::from([
            &b"gatewayType=stm32-can-gateway panelStream=1\r\ntype help for com"[..],
            &b"mands\r\n{\"panelStream\":\"re"[..],
            &b"set\"}\r\n{\"id\":8,\"ok\":true}\r\n"[..],
        ]));
        let mut replies = Vec::new();
        let mut panels = Vec::new();
        let supports_panel = AtomicBool::new(false);
        let error = receive(
            Box::new(input),
            |value| {
                replies.push(value);
                Ok(())
            },
            |event| panels.push(event),
            &supports_panel,
        );
        assert_eq!(error.unwrap_err(), "CAN gateway was unplugged.");
        assert_eq!(replies, vec![serde_json::json!({"id":8,"ok":true})]);
        assert!(matches!(panels.as_slice(), [Record::Reset]));
        assert!(supports_panel.load(Ordering::Acquire));
    }

    #[test]
    fn parked_reader_delivers_events_before_any_command() {
        struct Rx(mpsc::Receiver<Vec<u8>>);
        impl BlockingReceiver for Rx {
            fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, String> {
                let bytes = self.0.recv().map_err(|_| "closed")?;
                buffer[..bytes.len()].copy_from_slice(&bytes);
                Ok(bytes.len())
            }
        }
        struct Cancel(mpsc::Sender<Vec<u8>>);
        impl CancelIo for Cancel {
            fn cancel(&self) {
                let _ = self.0.send(Vec::new());
            }
        }
        struct Tx(mpsc::Sender<Vec<u8>>);
        impl DeadlineWriter for Tx {
            fn write_all(&mut self, bytes: &[u8], _: Instant) -> Result<(), String> {
                assert_eq!(bytes, b"cockpit 8 snapshot\n");
                self.0
                    .send(b"{\"id\":8,\"ok\":true}\n".to_vec())
                    .map_err(|e| e.to_string())
            }
            fn control_lines(&mut self, _: bool, _: bool) -> Result<(), String> {
                Ok(())
            }
        }
        let (device, input) = mpsc::channel();
        let (events, observed) = mpsc::channel();
        let mut serial = GatewaySerial::open(
            DuplexParts {
                receiver: Box::new(Rx(input)),
                writer: Box::new(Tx(device.clone())),
                cancel: Arc::new(Cancel(device.clone())),
            },
            move |event| {
                let _ = events.send(event);
            },
        )
        .unwrap();
        // Payload captured from the canonical firmware encoder.
        device.send(br#"{"panelStream":"event","moduleId":"0123456789abcdef0123456789abcdef","sourceNode":42,"payload":"0704000000050000000600000008000000090000000a0000000b0000000201820a000000"}
"#.to_vec()).unwrap();
        let Ok(Record::Event { packet, .. }) = observed.recv_timeout(Duration::from_secs(1)) else {
            panic!("parked reader did not deliver the panel event");
        };
        assert_eq!(packet.cursor, 10);
        assert_eq!(
            packet.payload,
            crate::stm32_panel_wire::Payload::Edges(vec![crate::stm32_panel_wire::Edge {
                control: 2,
                pressed: true,
                captured_at_ms: 10
            },])
        );
        let deadline = Instant::now() + Duration::from_secs(1);
        serial.send(b"cockpit 8 snapshot\n", deadline).unwrap();
        assert_eq!(serial.reply(8, deadline).unwrap()["ok"], true);
    }
}
