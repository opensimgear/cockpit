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
    timed_out_reply: Option<u64>,
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
            timed_out_reply: None,
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
        let stopping = matches!(request.command, Command::Stop);
        if self.timed_out_reply.is_some() && !stopping {
            return Err("A timed-out desktop worker accepts only terminal Stop.".into());
        }
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
        if stopping {
            // The writer drains the already queued Stop before closing stdin.
            // The helper exits after Stop, so the channel cannot send another.
            self.outgoing.take();
        }
        await_ack(&self.incoming, &mut self.timed_out_reply, request.sequence.0, stopping, deadline)
    }
    fn has_timed_out_reply(&self) -> bool { self.timed_out_reply.is_some() }
    fn audit_id(&self) -> Option<crate::audit::Id> { self.audit_channel }
}

fn await_ack(
    incoming: &Receiver<Result<serde_json::Value, String>>,
    timed_out_reply: &mut Option<u64>,
    sequence: u64,
    stopping: bool,
    deadline: Instant,
) -> Result<Ack, String> {
    let mut previous = timed_out_reply.take();
    loop {
        let value = match incoming.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(value) => value?,
            Err(error) => {
                if error == mpsc::RecvTimeoutError::Timeout && !stopping {
                    *timed_out_reply = Some(sequence);
                }
                return Err("Desktop worker died or exceeded its response deadline.".into());
            }
        };
        let ack: Ack = serde_json::from_value(value).map_err(|e| e.to_string())?;
        if let Some(late) = previous.take() {
            // Ignore only the exact outstanding delivery reply. Its contents
            // cannot clear possible holds or grant any new output authority.
            if ack.sequence.0 != late {
                return Err("Unexpected reply before desktop Stop acknowledgement.".into());
            }
            continue;
        }
        if Instant::now() >= deadline {
            return Err("Desktop worker acknowledgement exceeded its deadline.".into());
        }
        return Ok(ack);
    }
}
impl Drop for ProcessChannel {
    fn drop(&mut self) {
        self.outgoing.take();
        #[cfg(windows)]
        match wait_for_exit(&mut self.child, 350) {
            Ok(Some(status)) => {
                record_exit(self.audit_channel, self.child.id(), Some(status), 1);
                return;
            }
            Ok(None) => {}
            Err(_) => record_exit(self.audit_channel, self.child.id(), None, 3),
        }
        #[cfg(not(windows))]
        {
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
        }
        // Lease expiry normally cleans up first. A final kill is never proof of release.
        let _ = self.child.kill();
        #[cfg(windows)]
        let reaped = wait_for_exit(&mut self.child, 500);
        #[cfg(not(windows))]
        let reaped = self.child.wait().map(Some);
        match reaped {
            Ok(Some(status)) => record_exit(self.audit_channel, self.child.id(), Some(status), 2),
            Ok(None) | Err(_) => record_exit(self.audit_channel, self.child.id(), None, 3),
        }
    }
}

#[cfg(windows)]
fn wait_for_exit(child: &mut Child, timeout_ms: u32) -> std::io::Result<Option<ExitStatus>> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::Threading::WaitForSingleObject,
    };

    // The owned Child keeps this exact handle valid through the wait.
    match unsafe { WaitForSingleObject(child.as_raw_handle(), timeout_ms) } {
        WAIT_OBJECT_0 => child.wait().map(Some),
        WAIT_TIMEOUT => Ok(None),
        _ => Err(std::io::Error::last_os_error()),
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

    fn reply(sequence: u64) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({
            "sequence": sequence.to_string(), "operationId": null, "pointerBatch": null,
            "ok": true, "attempted": 0, "confirmed": 0, "uncertain": false,
            "cleanupComplete": true, "skipped": false, "scopeRejected": false,
            "scopes": [], "error": null, "held": [], "releasedGuards": [],
        }))
    }

    #[test]
    fn late_delivery_cannot_replace_or_block_the_exact_stop_ack() {
        let (send, incoming) = mpsc::sync_channel(2);
        let mut late = None;
        assert!(await_ack(&incoming, &mut late, 5, false, Instant::now()).is_err());
        assert_eq!(late, Some(5));
        send.send(reply(5)).unwrap();
        send.send(reply(6)).unwrap();
        let ack = await_ack(&incoming, &mut late, 6, true, Instant::now() + Duration::from_secs(1)).unwrap();
        assert_eq!(ack.sequence.0, 6);
        assert!(ack.cleanup_complete);
        assert_eq!(late, None);
    }

    #[test]
    fn late_reply_alone_or_an_unexpected_sequence_cannot_prove_stop() {
        for (first, error) in [(5, "response deadline"), (4, "Unexpected reply"), (6, "Unexpected reply")] {
            let (send, incoming) = mpsc::sync_channel(1);
            let mut late = Some(5);
            send.send(reply(first)).unwrap();
            drop(send);
            let result = await_ack(&incoming, &mut late, 6, true, Instant::now() + Duration::from_secs(1));
            assert!(result.unwrap_err().contains(error));
            assert_eq!(late, None);
        }
    }

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

    #[cfg(windows)]
    #[test]
    fn windows_wait_observes_the_owned_child_exit_or_deadline() {
        use std::os::windows::{
            io::{AsHandle, AsRawHandle},
            process::CommandExt,
        };
        use windows_sys::Win32::{Foundation::WAIT_OBJECT_0, System::Threading::WaitForSingleObject};

        let python = std::env::var_os("OSG_TEST_PYTHON").unwrap_or_else(|| "python".into());
        let spawn = |script, stdin| {
            ProcessCommand::new(&python)
                .args(["-c", script])
                .stdin(stdin)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(0x08000000)
                .spawn()
                .unwrap()
        };
        let channel = |child| ProcessChannel {
            child,
            outgoing: None,
            incoming: mpsc::sync_channel(1).1,
            timed_out_reply: None,
            audit_channel: None,
            outgoing_depth: Arc::new(AtomicUsize::new(0)),
        };
        let mut exited = spawn("raise SystemExit(7)", Stdio::null());
        assert_eq!(
            wait_for_exit(&mut exited, 5_000).unwrap().unwrap().code(),
            Some(7)
        );
        drop(channel(exited));

        let mut blocked = spawn("import sys; sys.stdin.buffer.read(1)", Stdio::piped());
        assert!(wait_for_exit(&mut blocked, 20).unwrap().is_none());
        let retained = blocked.as_handle().try_clone_to_owned().unwrap();
        drop(channel(blocked));
        assert_eq!(
            unsafe { WaitForSingleObject(retained.as_raw_handle(), 0) },
            WAIT_OBJECT_0
        );
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
