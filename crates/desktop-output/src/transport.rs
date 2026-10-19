//! Bounded parent process transport. No native API calls or ownership decisions.
use crate::{coordinator::Channel, protocol::*};
use std::{
    io::{BufReader, Write},
    path::Path,
    process::{Child, Command as ProcessCommand, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender}, Arc,
    },
    thread,
    time::{Duration, Instant},
};
pub struct ProcessChannel {
    child: Child,
    outgoing: Option<SyncSender<(Vec<u8>, u64)>>,
    incoming: Receiver<Result<serde_json::Value, String>>,
    audit_channel: Option<crate::audit::Id>,
    outgoing_depth: Arc<AtomicUsize>,
}
impl ProcessChannel {
    pub fn spawn(executable: &Path) -> Result<Self, String> {
        let mut command = ProcessCommand::new(executable);
        let audit_channel = crate::audit::next_id();
        if let Some(channel) = audit_channel {
            crate::audit::configure_child(&mut command, channel);
        }
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
        if let Some(channel) = audit_channel {
            crate::audit::record(crate::audit::Kind::ChannelOpened, &[channel, child.id() as u64]);
        }
        let mut input = child.stdin.take().ok_or("Missing desktop worker input.")?;
        let output = child
            .stdout
            .take()
            .ok_or("Missing desktop worker output.")?;
        let (outgoing, writes) = mpsc::sync_channel::<(Vec<u8>, u64)>(32);
        let outgoing_depth = Arc::new(AtomicUsize::new(0));
        let writer_depth = outgoing_depth.clone();
        thread::spawn(move || {
            for (bytes, sequence) in writes {
                if let Some(channel) = audit_channel {
                    let depth = writer_depth.fetch_sub(1, Ordering::AcqRel).saturating_sub(1);
                    crate::audit::record(crate::audit::Kind::RequestDequeued, &[channel, sequence, depth as u64]);
                }
                if input.write_all(&bytes).and_then(|_| input.flush()).is_err() {
                    if audit_channel.is_some() {
                        crate::audit::record(crate::audit::Kind::QueueFault, &[1, 2, writer_depth.load(Ordering::Acquire) as u64]);
                    }
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
            audit_channel,
            outgoing_depth,
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
        let outgoing = self.outgoing.as_ref().ok_or("Desktop worker is stopped.")?;
        let queued_at = crate::audit::stamp();
        let queued_depth = if self.audit_channel.is_some() {
            self.outgoing_depth.fetch_add(1, Ordering::AcqRel) + 1
        } else { 0 };
        let queued = outgoing.try_send((bytes, request.sequence.0));
        if queued.is_err() {
            if self.audit_channel.is_some() {
                self.outgoing_depth.fetch_sub(1, Ordering::AcqRel);
                crate::audit::record(crate::audit::Kind::QueueFault, &[1, 1, queued_depth as u64]);
            }
            return Err("Desktop worker outgoing queue unavailable.".into());
        }
        if let (Some(at), Some(channel)) = (queued_at, self.audit_channel) {
            crate::audit::record_at(at, crate::audit::Kind::RequestQueued,
                &[channel, request.sequence.0, queued_depth as u64]);
        }
        let value = self.reply(deadline.saturating_duration_since(Instant::now()))?;
        if Instant::now() >= deadline {
            return Err("Desktop worker acknowledgement exceeded its deadline.".into());
        }
        serde_json::from_value(value).map_err(|e| e.to_string())
    }
    fn audit_id(&self) -> Option<crate::audit::Id> { self.audit_channel }
}
impl Drop for ProcessChannel {
    fn drop(&mut self) {
        self.outgoing.take();
        let deadline = Instant::now() + Duration::from_millis(350);
        let mut reported_wait_error = false;
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    record_exit(self.audit_channel, self.child.id(), Some(status), 1);
                    return;
                }
                Err(_) if !reported_wait_error => {
                    record_exit(self.audit_channel, self.child.id(), None, 3);
                    reported_wait_error = true;
                }
                _ => {}
            }
            thread::sleep(Duration::from_millis(5));
        }
        // Lease expiry normally cleans up first. A final kill is never proof of release.
        let _ = self.child.kill();
        match self.child.wait() {
            Ok(status) => record_exit(self.audit_channel, self.child.id(), Some(status), 2),
            Err(_) => record_exit(self.audit_channel, self.child.id(), None, 3),
        }
    }
}
fn exit_fields(channel: u64, pid: u32, status: Option<ExitStatus>, reason: u64) -> [u64; 5] {
    [channel, pid as u64,
        status.and_then(|status| status.code()).map(|code| code as u32 as u64).unwrap_or(u64::MAX),
        u64::from(reason == 1 && status.is_some_and(|status| status.success())), reason]
}
fn record_exit(channel: Option<crate::audit::Id>, pid: u32, status: Option<ExitStatus>, reason: u64) {
    if let Some(channel) = channel {
        crate::audit::record(crate::audit::Kind::ChannelExited, &exit_fields(channel, pid, status, reason));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    fn status(code: i32) -> ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(code as u32)
    }
    #[cfg(unix)]
    fn status(code: i32) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(code << 8)
    }

    #[test]
    fn exact_exit_proof_distinguishes_success_diagnostic_failure_kill_and_wait_error() {
        assert_eq!(exit_fields(7, 42, Some(status(0)), 1), [7, 42, 0, 1, 1]);
        assert_eq!(exit_fields(7, 42, Some(status(87)), 1), [7, 42, 87, 0, 1]);
        assert_eq!(exit_fields(7, 42, Some(status(0)), 2), [7, 42, 0, 0, 2]);
        assert_eq!(exit_fields(7, 42, None, 3), [7, 42, u64::MAX, 0, 3]);
        #[cfg(windows)]
        assert_eq!(exit_fields(7, 42, Some(status(-1)), 1), [7, 42, u32::MAX as u64, 0, 1]);
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(exit_fields(7, 42, Some(ExitStatus::from_raw(9)), 1), [7, 42, u64::MAX, 0, 1]);
        }
    }
}
