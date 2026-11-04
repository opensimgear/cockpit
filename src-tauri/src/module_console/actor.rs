//! Single-reader USB console actor. Unsolicited input never waits for an ACK.
//! Transport faults are terminal: writes are never retried and late replies
//! cannot be borrowed by a later command. Only native capture runs in `Sink`.
use super::{push::MAX_LINE_BYTES, Reply, ReplyKind, CONSOLE_BANNER, MAX_JSON_BYTES};
use crate::transport::duplex::{BlockingReceiver, CancelIo, DeadlineWriter, DuplexParts};
use serde_json::Value;
use std::{
    collections::VecDeque,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{Arc, Condvar, Mutex, MutexGuard},
    thread::{self, JoinHandle},
    time::Instant,
};

#[derive(Debug)]
pub(crate) enum Input {
    Line { line: String, received_at: Instant },
    Lost(String),
}
pub(crate) type Sink = Arc<dyn Fn(Input) + Send + Sync>;

struct Expected { limit: usize, received: usize }
struct Mailbox {
    handshake: bool,
    expected: Option<Expected>,
    replies: VecDeque<String>,
    fault: Option<String>,
}
struct Shared {
    mailbox: Mutex<Mailbox>,
    changed: Condvar,
    delivery: Mutex<()>,
    sink: Sink,
    cancel: Arc<dyn CancelIo>,
}
impl Shared {
    fn lock(&self) -> MutexGuard<'_, Mailbox> {
        self.mailbox.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn fail(&self, error: String) {
        {
            let mut state = self.lock();
            if state.fault.is_some() { return; }
            state.fault = Some(error.clone());
            state.expected = None;
            state.replies.clear();
            self.changed.notify_all();
        }
        self.cancel.cancel();
        let _delivery = self.delivery.lock().unwrap_or_else(|e| e.into_inner());
        let _ = catch_unwind(AssertUnwindSafe(|| (self.sink)(Input::Lost(error))));
    }
    fn input(&self, line: String, received_at: Instant) -> Result<(), String> {
        let delivery = self.delivery.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(error) = &self.lock().fault { return Err(error.clone()); }
        let result = catch_unwind(AssertUnwindSafe(|| {
            (self.sink)(Input::Line { line, received_at });
        }));
        drop(delivery);
        result.map_err(|_| "USB module input capture panicked.".into())
    }
    fn line(&self, line: String, received_at: Instant) -> Result<(), String> {
        if line.starts_with("@events ") || line.starts_with("@meta ") {
            return self.input(line, received_at);
        }
        if line.len() > MAX_JSON_BYTES {
            return Err("USB module response exceeds its byte limit.".into());
        }
        let mut state = self.lock();
        if let Some(error) = &state.fault { return Err(error.clone()); }
        if line == CONSOLE_BANNER {
            return if state.handshake { Ok(()) } else {
                Err("The USB module restarted; establish a fresh connection.".into())
            };
        }
        let expected = state.expected.as_mut()
            .ok_or("USB module returned an unsolicited command reply.")?;
        if expected.received >= expected.limit || state.replies.len() >= 2 {
            return Err("USB module command reply mailbox overflowed.".into());
        }
        state.expected.as_mut().unwrap().received += 1;
        state.replies.push_back(line);
        self.changed.notify_all();
        Ok(())
    }
}
struct Cancellation(Arc<Shared>);
impl CancelIo for Cancellation {
    fn cancel(&self) { self.0.fail("USB module service has stopped.".into()); }
}

pub(crate) struct StreamConsole {
    writer: Box<dyn DeadlineWriter>,
    shared: Arc<Shared>,
    reader: Option<JoinHandle<()>>,
}
impl StreamConsole {
    pub(crate) fn new(parts: DuplexParts, sink: Sink) -> Result<Self, String> {
        let DuplexParts { receiver, mut writer, cancel } = parts;
        let shared = Arc::new(Shared {
            mailbox: Mutex::new(Mailbox { handshake: true, expected: None,
                replies: VecDeque::with_capacity(2), fault: None }),
            changed: Condvar::new(), delivery: Mutex::new(()), sink, cancel,
        });
        if let Err(error) = writer.control_lines(true, false) {
            shared.fail(error.clone());
            let _ = writer.control_lines(false, false);
            return Err(error);
        }
        let incoming = Arc::clone(&shared);
        let reader = thread::Builder::new().name("module-console-rx".into()).spawn(move || {
            let outcome = catch_unwind(AssertUnwindSafe(|| receive(receiver, &incoming)));
            let error = match outcome {
                Ok(Err(error)) => error,
                Err(_) => "USB module reader panicked.".into(),
                Ok(Ok(())) => return,
            };
            incoming.fail(error);
        });
        match reader {
            Ok(reader) => Ok(Self { writer, shared, reader: Some(reader) }),
            Err(error) => {
                let error = format!("Unable to start USB module reader: {error}");
                shared.fail(error.clone());
                let _ = writer.control_lines(false, false);
                Err(error)
            }
        }
    }
    pub(crate) fn finish_handshake(&mut self) { self.shared.lock().handshake = false; }
    pub(crate) fn cancel_handle(&self) -> Arc<dyn CancelIo> {
        Arc::new(Cancellation(Arc::clone(&self.shared)))
    }
    pub(crate) fn exchange(&mut self, command: &str, kind: ReplyKind,
        deadline: Instant) -> Result<Reply, String>
    {
        let maximum = if command.starts_with("config ") { 390 } else { 383 };
        if command.is_empty() || command.len() > maximum || !command.is_ascii()
            || command.bytes().any(|b| b.is_ascii_control())
        { return Err("Invalid USB module command.".into()); }
        let count = if matches!(kind, ReplyKind::Config) { 2 } else { 1 };
        {
            let mut state = self.shared.lock();
            if let Some(error) = &state.fault { return Err(error.clone()); }
            state.expected = Some(Expected { limit: count, received: 0 });
        }
        let outcome = (|| {
            if Instant::now() >= deadline { return Err(timeout()); }
            let mut bytes = Vec::with_capacity(command.len() + 1);
            bytes.extend_from_slice(command.as_bytes());
            bytes.push(b'\n');
            self.writer.write_all(&bytes, deadline)
                .map_err(|e| format!("USB module write failed; its result is unknown: {e}"))?;
            let mut lines = self.replies(1, deadline)?;
            if matches!(kind, ReplyKind::Config) {
                configuration_error(&lines[0])?;
                lines.extend(self.replies(1, deadline)?);
            }
            parse(lines, kind)
        })();
        match outcome {
            Ok(reply) => {
                let mut state = self.shared.lock();
                if let Some(error) = &state.fault { return Err(error.clone()); }
                state.expected = None;
                Ok(reply)
            }
            Err(error) => { self.shared.fail(error.clone()); Err(error) }
        }
    }
    fn replies(&self, count: usize, deadline: Instant) -> Result<Vec<String>, String> {
        let mut state = self.shared.lock();
        loop {
            if let Some(error) = &state.fault { return Err(error.clone()); }
            if state.replies.len() >= count { return Ok(state.replies.drain(..count).collect()); }
            let remaining = deadline.checked_duration_since(Instant::now()).ok_or_else(timeout)?;
            let (next, _) = self.shared.changed.wait_timeout(state, remaining)
                .unwrap_or_else(|e| e.into_inner());
            state = next;
        }
    }
}
impl Drop for StreamConsole {
    fn drop(&mut self) {
        let _ = self.writer.control_lines(false, false);
        self.shared.fail("USB module console was closed.".into());
        if let Some(reader) = self.reader.take() { let _ = reader.join(); }
    }
}
fn timeout() -> String {
    "USB module response timed out; the connection was invalidated.".into()
}
fn receive(mut receiver: Box<dyn BlockingReceiver>, shared: &Shared) -> Result<(), String> {
    let mut chunk = [0; 1024];
    let mut line = Vec::with_capacity(MAX_LINE_BYTES + 1);
    let mut first_fragment = None;
    loop {
        let count = receiver.receive(&mut chunk)?;
        let received_at = Instant::now(); // Before any mailbox or capture lock.
        if count == 0 { return Err("USB module was disconnected.".into()); }
        if count > chunk.len() { return Err("USB module receiver exceeded its buffer.".into()); }
        for byte in &chunk[..count] {
            if first_fragment.is_none() { first_fragment = Some(received_at); }
            if *byte == b'\n' {
                if line.last() == Some(&b'\r') { line.pop(); }
                if line.len() > MAX_LINE_BYTES {
                    return Err("USB module response exceeds its byte limit.".into());
                }
                let text = String::from_utf8(std::mem::take(&mut line))
                    .map_err(|_| "USB module returned invalid UTF-8.")?;
                let timestamp = first_fragment.take().unwrap();
                if !text.is_empty() { shared.line(text, timestamp)?; }
            } else {
                line.push(*byte);
                if line.len() > MAX_LINE_BYTES + usize::from(*byte == b'\r') {
                    return Err("USB module response exceeds its byte limit.".into());
                }
            }
        }
    }
}
fn parse(mut lines: Vec<String>, kind: ReplyKind) -> Result<Reply, String> {
    let first = lines.remove(0);
    match kind {
        ReplyKind::Text => Ok(Reply::Text(first)),
        ReplyKind::Json => {
            if first == "ERR unknown command" { return Ok(Reply::Unsupported); }
            let value: Value = serde_json::from_str(&first)
                .map_err(|_| "USB module returned malformed JSON.")?;
            if !value["ok"].is_boolean() {
                return Err("USB module returned an invalid capability response.".into());
            }
            Ok(Reply::Json(value))
        }
        ReplyKind::Config => {
            let error = configuration_error(&first)?;
            let snapshot: Value = serde_json::from_str(&lines.remove(0))
                .map_err(|_| "USB module returned malformed configuration JSON.")?;
            super::validate_config(&snapshot)?;
            Ok(Reply::Config { error, snapshot })
        }
    }
}
fn configuration_error(line: &str) -> Result<Option<String>, String> {
    if line == "OK" { return Ok(None); }
    line.strip_prefix("ERR ").filter(|s| !s.is_empty() && s.len() <= 63
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        .map(|s| Some(s.to_owned()))
        .ok_or_else(|| "USB module returned an invalid configuration acknowledgement.".into())
}

#[cfg(test)]
#[path = "actor_tests.rs"]
mod tests;
