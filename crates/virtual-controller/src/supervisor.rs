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
pub struct Worker {
    child: Child,
    outgoing: Option<SyncSender<Vec<u8>>>,
    incoming: Receiver<Result<Value, String>>,
    timeout: Duration,
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
            hello: Value::Null,
        };
        let hello = worker.reply()?;
        if hello["ok"] != true || hello["backend"] != backend || hello["protocol"] != 1 {
            return Err("Output helper handshake does not match the expected backend.".into());
        }
        worker.hello = hello;
        Ok(worker)
    }
    pub fn handshake(&self) -> &Value {
        &self.hello
    }
    fn reply(&mut self) -> Result<Value, String> {
        match self.incoming.recv_timeout(self.timeout) {
            Ok(result) => result,
            Err(_) => Err("Output helper exited or exceeded its response deadline.".into()),
        }
    }
    pub fn request(&mut self, message: &Message) -> Result<(), String> {
        let mut bytes = serde_json::to_vec(message).map_err(|e| e.to_string())?;
        if bytes.len() > FRAME_LIMIT {
            return Err("Output helper request exceeds its frame limit.".into());
        }
        bytes.push(b'\n');
        let result = (|| {
            self.outgoing
                .as_ref()
                .ok_or("Output helper has stopped.")?
                .try_send(bytes)
                .map_err(|_| "Output helper input queue is unavailable.")?;
            let response = self.reply()?;
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
