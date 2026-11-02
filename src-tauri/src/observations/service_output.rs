//! Bounded pipe delivery. An unread dev bridge never holds the command reader.
use serde_json::Value;
use std::{
    io::{self, Write},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc,
    },
};

const BYTE_LIMIT: usize = 16 * 1024 * 1024;
const FRAME_LIMIT: usize = 64;

#[derive(Clone)]
pub struct ServiceOutput {
    sender: mpsc::SyncSender<Vec<u8>>,
    bytes: Arc<AtomicUsize>,
    failed: Arc<AtomicBool>,
    fault: mpsc::SyncSender<()>,
}

impl ServiceOutput {
    pub fn stdout() -> (Self, mpsc::Receiver<()>) {
        Self::with_writer(io::stdout())
    }

    fn with_writer(mut writer: impl Write + Send + 'static) -> (Self, mpsc::Receiver<()>) {
        let (sender, frames) = mpsc::sync_channel::<Vec<u8>>(FRAME_LIMIT);
        let (fault, failures) = mpsc::sync_channel(1);
        let bytes = Arc::new(AtomicUsize::new(0));
        let failed = Arc::new(AtomicBool::new(false));
        let output = Self {
            sender,
            bytes: bytes.clone(),
            failed: failed.clone(),
            fault: fault.clone(),
        };
        std::thread::spawn(move || {
            while let Ok(frame) = frames.recv() {
                let result = writer.write_all(&frame).and_then(|_| writer.flush());
                bytes.fetch_sub(frame.len(), Ordering::AcqRel);
                if result.is_err() {
                    if !failed.swap(true, Ordering::AcqRel) {
                        let _ = fault.try_send(());
                    }
                    break;
                }
            }
        });
        (output, failures)
    }

    pub fn emit(&self, value: Value) -> Result<(), String> {
        if self.failed.load(Ordering::Acquire) {
            return Err("Gateway output transport is closed.".into());
        }
        let mut buffer = BoundedBuffer(Vec::new());
        if serde_json::to_writer(&mut buffer, &value).is_err() {
            return self.fail();
        }
        let mut frame = buffer.0;
        frame.push(b'\n');
        let length = frame.len();
        if self
            .bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current
                    .checked_add(length)
                    .filter(|total| *total <= BYTE_LIMIT)
            })
            .is_err()
        {
            return self.fail();
        }
        if self.sender.try_send(frame).is_err() {
            self.bytes.fetch_sub(length, Ordering::AcqRel);
            return self.fail();
        }
        Ok(())
    }

    fn fail(&self) -> Result<(), String> {
        if !self.failed.swap(true, Ordering::AcqRel) {
            let _ = self.fault.try_send(());
        }
        Err("Gateway output transport exceeded its bounded delivery budget.".into())
    }
}

struct BoundedBuffer(Vec<u8>);
impl Write for BoundedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > BYTE_LIMIT.saturating_sub(1).saturating_sub(self.0.len()) {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "Gateway envelope exceeds byte budget.",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Stalled(mpsc::Receiver<()>, mpsc::SyncSender<()>);
    impl Write for Stalled {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let _ = self.1.try_send(());
            let _ = self.0.recv();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn blocked_pipe_is_isolated_and_faults_at_a_bounded_queue() {
        let (release, blocked) = mpsc::channel();
        let (entered, started) = mpsc::sync_channel(1);
        let (output, fault) = ServiceOutput::with_writer(Stalled(blocked, entered));
        output.emit(serde_json::json!({"first":true})).unwrap();
        started
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        for _ in 0..FRAME_LIMIT {
            output.emit(serde_json::json!({"queued":true})).unwrap();
        }
        assert!(output.emit(serde_json::json!({"overflow":true})).is_err());
        fault
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        assert!(output.bytes.load(Ordering::Acquire) <= BYTE_LIMIT);
        assert!(output.emit(serde_json::json!({"late":true})).is_err());
        drop(release);
    }
    #[test]
    fn byte_limit_and_actual_writer_failure_are_explicit() {
        let (output, fault) = ServiceOutput::with_writer(io::sink());
        assert!(output
            .emit(serde_json::json!("x".repeat(BYTE_LIMIT)))
            .is_err());
        fault
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let (output, fault) = ServiceOutput::with_writer(Broken);
        output.emit(serde_json::json!({"test":true})).unwrap();
        fault
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        assert!(output.emit(serde_json::json!({"late":true})).is_err());
    }
}
