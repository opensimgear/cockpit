//! Bounded parent process transport. No native API calls or ownership decisions.
use crate::{coordinator::Channel, protocol::*};
use std::{
    io::{BufReader, Write},
    path::Path,
    process::{Child, Command as ProcessCommand, Stdio},
    sync::mpsc::{self, Receiver, SyncSender},
    thread,
    time::{Duration, Instant},
};
pub struct ProcessChannel {
    child: Child,
    outgoing: Option<SyncSender<Vec<u8>>>,
    incoming: Receiver<Result<serde_json::Value, String>>,
}
impl ProcessChannel {
    pub fn spawn(executable: &Path) -> Result<Self, String> {
        let mut command = ProcessCommand::new(executable);
        command
            .arg(crate::entry::ARGUMENT)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("Cannot start desktop worker: {e}"))?;
        let mut input = child.stdin.take().ok_or("Missing desktop worker input.")?;
        let output = child
            .stdout
            .take()
            .ok_or("Missing desktop worker output.")?;
        let (outgoing, writes) = mpsc::sync_channel::<Vec<u8>>(32);
        thread::spawn(move || {
            for bytes in writes {
                if input.write_all(&bytes).and_then(|_| input.flush()).is_err() {
                    break;
                }
            }
        });
        let (responses, incoming) = mpsc::sync_channel(32);
        thread::spawn(move || {
            let mut output = BufReader::new(output);
            loop {
                let reply = crate::worker::read_frame(&mut output)
                    .and_then(|b| b.ok_or("Desktop worker exited.".into()))
                    .and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string()));
                let failed = reply.is_err();
                if responses.try_send(reply).is_err() || failed {
                    break;
                }
            }
        });
        let mut channel = Self {
            child,
            outgoing: Some(outgoing),
            incoming,
        };
        let hello = channel.reply(Duration::from_secs(3))?;
        if hello["ok"] != true
            || hello["protocol"] != PROTOCOL
            || hello["backend"] != "desktop-input"
        {
            return Err(hello["error"]
                .as_str()
                .unwrap_or("Desktop worker handshake mismatch.")
                .into());
        }
        Ok(channel)
    }
    fn reply(&mut self, timeout: Duration) -> Result<serde_json::Value, String> {
        self.incoming
            .recv_timeout(timeout)
            .map_err(|_| "Desktop worker died or exceeded its response deadline.".to_string())?
    }
}
impl Channel for ProcessChannel {
    fn request(&mut self, request: &Request) -> Result<Ack, String> {
        let deadline = Instant::now() + Duration::from_millis(100);
        let mut bytes = serde_json::to_vec(request).map_err(|e| e.to_string())?;
        if bytes.len() > FRAME_LIMIT {
            return Err("Desktop parent frame exceeds 64 KiB.".into());
        }
        bytes.push(b'\n');
        self.outgoing
            .as_ref()
            .ok_or("Desktop worker is stopped.")?
            .try_send(bytes)
            .map_err(|_| "Desktop worker outgoing queue unavailable.")?;
        let value = self.reply(deadline.saturating_duration_since(Instant::now()))?;
        if Instant::now() >= deadline {
            return Err("Desktop worker acknowledgement exceeded its deadline.".into());
        }
        serde_json::from_value(value).map_err(|e| e.to_string())
    }
}
impl Drop for ProcessChannel {
    fn drop(&mut self) {
        self.outgoing.take();
        let deadline = Instant::now() + Duration::from_millis(350);
        while Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        // Lease expiry normally cleans up first. A final kill is never proof of release.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
