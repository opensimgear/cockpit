//! Parent-side bounded IPC and process liveness. No driver calls run on the UI thread.
use crate::{
    feeder::Message,
    worker::{read_frame, FRAME_LIMIT},
};
use serde_json::Value;
use std::{
    io::{BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, SyncSender},
    thread,
    time::{Duration, Instant},
};
const RESPONSE_DEADLINE_ERROR: &str = "Output helper exited or exceeded its response deadline.";
pub struct Worker {
    child: Child,
    outgoing: Option<SyncSender<Vec<u8>>>,
    incoming: Receiver<Result<Value, String>>,
    timeout: Duration,
    activation_timeout: Duration,
    hello: Value,
}
impl Worker {
    pub fn spawn(
        program: &Path,
        args: &[String],
        backend: &str,
        timeout: Duration,
    ) -> Result<Self, String> {
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("Cannot start output helper: {e}"))?;
        let mut input = child.stdin.take().ok_or("Missing helper input pipe.")?;
        let output = child.stdout.take().ok_or("Missing helper output pipe.")?;
        let (outgoing, writes) = mpsc::sync_channel::<Vec<u8>>(32);
        thread::spawn(move || {
            for bytes in writes {
                if input.write_all(&bytes).and_then(|_| input.flush()).is_err() {
                    break;
                }
            }
        });
        let (replies, incoming) = mpsc::sync_channel(32);
        thread::spawn(move || {
            let mut output = BufReader::new(output);
            loop {
                let result = match read_frame(&mut output) {
                    Ok(Some(bytes)) => {
                        serde_json::from_slice::<Value>(&bytes).map_err(|e| e.to_string())
                    }
                    Ok(None) => Err("Output helper exited.".into()),
                    Err(e) => Err(e.to_string()),
                };
                let failed = result.is_err();
                if replies.try_send(result).is_err() || failed {
                    break;
                }
            }
        });
        let mut worker = Self {
            child,
            outgoing: Some(outgoing),
            incoming,
            timeout,
            activation_timeout: timeout,
            hello: Value::Null,
        };
        let hello = worker.reply_before(Instant::now() + timeout)?;
        if hello["ok"] != true || hello["backend"] != backend || hello["protocol"] != 1 {
            return Err("Output helper handshake does not match the expected backend.".into());
        }
        worker.hello = hello;
        Ok(worker)
    }
    pub fn response_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }
    pub fn handshake(&self) -> &Value {
        &self.hello
    }
    /// The read-only inspector has a separate readiness handshake. Its one
    /// inventory request shares the caller's absolute observation deadline.
    pub(crate) fn inspect_once(&mut self, nonce: &str, deadline: Instant) -> Result<Value, String> {
        let result = (|| {
            if self.hello["backend"] != "vjoy-inspect-ready" {
                return Err("This helper is not a ready read-only inspector.".into());
            }
            let mut bytes = serde_json::to_vec(&serde_json::json!({"nonce":nonce}))
                .map_err(|e| e.to_string())?;
            bytes.push(b'\n');
            if Instant::now() >= deadline {
                return Err(RESPONSE_DEADLINE_ERROR.into());
            }
            self.outgoing
                .as_ref()
                .ok_or("Output helper has stopped.")?
                .try_send(bytes)
                .map_err(|_| "Output helper input queue is unavailable.")?;
            self.reply_before(deadline)
        })();
        if result.is_err() {
            self.terminate();
        }
        result
    }
    fn reply_before(&mut self, deadline: Instant) -> Result<Value, String> {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(RESPONSE_DEADLINE_ERROR)?;
        let result = self
            .incoming
            .recv_timeout(remaining)
            .map_err(|_| RESPONSE_DEADLINE_ERROR)?;
        // recv_timeout can return an already queued reply after this caller was
        // descheduled. A queued acknowledgement does not extend its deadline.
        if Instant::now() >= deadline {
            return Err(RESPONSE_DEADLINE_ERROR.into());
        }
        result
    }
    pub fn request(&mut self, message: &Message) -> Result<(), String> {
        // Include serialization, enqueueing and any scheduler delay in one
        // absolute budget. Native acquisition retains its separate startup limit.
        let timeout = if matches!(message, Message::Start { .. } | Message::Recover { .. }) {
            self.activation_timeout.max(self.timeout)
        } else {
            self.timeout
        };
        let deadline = Instant::now() + timeout;
        let mut bytes = serde_json::to_vec(message).map_err(|e| e.to_string())?;
        if bytes.len() > FRAME_LIMIT {
            return Err("Output helper request exceeds its frame limit.".into());
        }
        bytes.push(b'\n');
        let result = (|| {
            if Instant::now() >= deadline {
                return Err(RESPONSE_DEADLINE_ERROR.into());
            }
            self.outgoing
                .as_ref()
                .ok_or("Output helper has stopped.")?
                .try_send(bytes)
                .map_err(|_| "Output helper input queue is unavailable.")?;
            // Native slot acquisition can exceed the report/renewal budget even
            // when the driver is healthy. Keep Start/Recover bounded by the
            // original startup budget; steady-state reports retain the short lease budget.
            let response = self.reply_before(deadline)?;
            if response["token"]
                != serde_json::to_value(message.token()).map_err(|e| e.to_string())?
            {
                return Err("Stale output helper acknowledgement.".into());
            }
            if response["ok"] != true {
                return Err(response["error"]
                    .as_str()
                    .unwrap_or("Output helper rejected a request.")
                    .chars()
                    .take(256)
                    .collect());
            }
            if Instant::now() >= deadline {
                return Err(RESPONSE_DEADLINE_ERROR.into());
            }
            Ok(())
        })();
        if result.is_err() {
            self.terminate();
        }
        result
    }
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
    /// PID of the original retained child, never an independently reopened process.
    /// Callers must separately verify liveness on this same worker.
    pub fn process_id(&self) -> u32 {
        self.child.id()
    }
    pub fn terminate(&mut self) {
        self.outgoing.take();
        if self.alive() {
            let _ = self.child.kill();
            let deadline = Instant::now() + Duration::from_millis(500);
            while self.alive() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(5));
            }
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.terminate();
    }
}
#[cfg(test)]
#[path = "supervisor_tests.rs"]
mod tests;
