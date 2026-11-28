//! Read-only hidraw boundary. GET workers retain the selected fd through timeout;
//! late completions never become reports and cannot overlap a replacement open.
mod abi;
mod metadata;
mod native;
#[cfg(test)]
mod tests;

use serde::Serialize;
use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::fs::{FileTypeExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};

pub const MAX_REPORT_BYTES: usize = 65_536;
pub const MAX_DEADLINE: Duration = Duration::from_secs(2);
const MAX_DEVICES: usize = 8;
static LEASES: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorKind {
    InvalidRequest,
    AccessDenied,
    Unsupported,
    Timeout,
    Busy,
    Removed,
    Eof,
    Capacity,
    Io,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IoError {
    pub kind: ErrorKind,
    pub code: Option<i32>,
    pub message: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "status", content = "data", rename_all = "kebab-case")]
pub enum Receive {
    Report(Vec<u8>),
    Timeout,
    Removed,
    Error(IoError),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GetKind {
    Input,
    Feature,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetReport {
    pub request_id: String,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub descriptor: Vec<u8>,
    pub bus: u32,
    pub vendor_id: u16,
    pub product_id: u16,
    pub name: Option<String>,
    pub physical: Option<String>,
    pub unique: Option<String>,
}

trait Calls: Send + Sync {
    fn ioctl(&self, fd: i32, request: u32, bytes: &mut [u8]) -> std::io::Result<usize>;
    fn poll(&self, fd: i32, timeout: i32) -> std::io::Result<i16>;
    fn read(&self, fd: i32, bytes: &mut [u8]) -> std::io::Result<usize>;
}
struct Lease {
    file: File,
    key: PathBuf,
}
impl Drop for Lease {
    fn drop(&mut self) {
        LEASES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.key);
    }
}
struct PendingGet {
    result: mpsc::Receiver<Result<Vec<u8>, IoError>>,
}
pub struct Device {
    lease: Arc<Lease>,
    calls: Arc<dyn Calls>,
    pending: Option<PendingGet>,
}

fn error(kind: ErrorKind, message: impl Into<String>) -> IoError {
    IoError {
        kind,
        code: None,
        message: message.into(),
    }
}
fn os(context: &str, failure: std::io::Error) -> IoError {
    let code = failure.raw_os_error();
    let kind = match code {
        Some(libc::EACCES | libc::EPERM) => ErrorKind::AccessDenied,
        Some(libc::ENODEV | libc::ENXIO | libc::EBADF | libc::ENOENT) => ErrorKind::Removed,
        Some(libc::ENOTTY | libc::ENOSYS | libc::EOPNOTSUPP) => ErrorKind::Unsupported,
        _ => ErrorKind::Io,
    };
    IoError {
        kind,
        code,
        message: format!("{context}: {failure}"),
    }
}
fn deadline(timeout: Duration) -> Result<(), IoError> {
    if timeout > MAX_DEADLINE {
        Err(error(
            ErrorKind::InvalidRequest,
            "hidraw deadline exceeds two seconds",
        ))
    } else {
        Ok(())
    }
}
impl Device {
    pub fn open(path: &Path) -> Result<Self, IoError> {
        let canonical = path
            .canonicalize()
            .map_err(|e| os("Resolve selected hidraw path", e))?;
        let hidraw = canonical.parent() == Some(Path::new("/dev"))
            && canonical
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix("hidraw"))
                .is_some_and(|suffix| {
                    !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
                });
        if !hidraw {
            return Err(error(
                ErrorKind::InvalidRequest,
                "Only an explicitly selected /dev/hidrawN interface can be opened",
            ));
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&canonical)
            .map_err(|e| os("Open shared read-only hidraw interface", e))?;
        if !file
            .metadata()
            .map_err(|e| os("Inspect opened hidraw fd", e))?
            .file_type()
            .is_char_device()
        {
            return Err(error(
                ErrorKind::InvalidRequest,
                "Selected hidraw path is not a character device",
            ));
        }
        Self::from_file(file, canonical, Arc::new(native::Native))
    }
    fn from_file(file: File, key: PathBuf, calls: Arc<dyn Calls>) -> Result<Self, IoError> {
        let mut leases = LEASES.lock().unwrap_or_else(|e| e.into_inner());
        if leases.contains(&key) {
            return Err(error(
                ErrorKind::Busy,
                "A hidraw operation still owns this interface generation",
            ));
        }
        if leases.len() >= MAX_DEVICES {
            return Err(error(
                ErrorKind::Capacity,
                "hidraw session/retired-worker limit reached",
            ));
        }
        leases.insert(key.clone());
        drop(leases);
        Ok(Self {
            lease: Arc::new(Lease { file, key }),
            calls,
            pending: None,
        })
    }
    pub fn file(&self) -> &File {
        &self.lease.file
    }
    pub fn metadata(&self) -> Result<Metadata, IoError> {
        metadata::read(self.file().as_raw_fd(), self.calls.as_ref())
    }
    pub fn poll(&mut self, max_bytes: usize, timeout: Duration) -> Receive {
        match self.read(max_bytes, timeout) {
            Ok(Some(bytes)) => Receive::Report(bytes),
            Ok(None) => Receive::Timeout,
            Err(failure) if failure.kind == ErrorKind::Removed => Receive::Removed,
            Err(failure) => Receive::Error(failure),
        }
    }
    fn read(&self, max_bytes: usize, timeout: Duration) -> Result<Option<Vec<u8>>, IoError> {
        deadline(timeout)?;
        if max_bytes == 0 || max_bytes > MAX_REPORT_BYTES {
            return Err(error(ErrorKind::Capacity, "Invalid hidraw report capacity"));
        }
        let end = Instant::now() + timeout;
        loop {
            let remaining = end.saturating_duration_since(Instant::now());
            let milliseconds = remaining
                .as_millis()
                .saturating_add(u128::from(remaining.subsec_nanos() % 1_000_000 != 0))
                .min(2000) as i32;
            let events = match self.calls.poll(self.file().as_raw_fd(), milliseconds) {
                Ok(events) => events,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                    if Instant::now() >= end {
                        return Ok(None);
                    }
                    continue;
                }
                Err(e) => return Err(os("Poll hidraw input", e)),
            };
            if events & libc::POLLIN != 0 {
                let mut bytes = vec![0; max_bytes + 1];
                match self.calls.read(self.file().as_raw_fd(),&mut bytes) {
                    Ok(0)=>return Err(error(ErrorKind::Eof,"hidraw returned EOF")),
                    Ok(count) if count>max_bytes=>return Err(error(ErrorKind::Capacity,"hidraw report exceeded capacity; truncated delivery must be recorded as a gap")),
                    Ok(count)=>{bytes.truncate(count);return Ok(Some(bytes));},
                    Err(e) if matches!(e.kind(),std::io::ErrorKind::WouldBlock|std::io::ErrorKind::Interrupted)=>{
                        if Instant::now()>=end {return Ok(None);} continue;
                    },
                    Err(e)=>return Err(os("Read hidraw input",e)),
                }
            }
            if events & (libc::POLLHUP | libc::POLLNVAL) != 0 {
                return Err(error(
                    ErrorKind::Removed,
                    "hidraw interface hung up or became inaccessible",
                ));
            }
            if events & libc::POLLERR != 0 {
                return Err(error(ErrorKind::Io, "hidraw poll reported an I/O error"));
            }
            return Ok(None);
        }
    }
    pub fn get(
        &mut self,
        request_id: &str,
        kind: GetKind,
        report_id: u8,
        max_bytes: usize,
        timeout: Duration,
    ) -> Result<GetReport, IoError> {
        let started = Instant::now();
        deadline(timeout)?;
        if request_id.is_empty()
            || request_id.len() > 80
            || request_id.chars().any(char::is_control)
        {
            return Err(error(
                ErrorKind::InvalidRequest,
                "Bounded hidraw request ID required",
            ));
        }
        if max_bytes < 2 || max_bytes > MAX_REPORT_BYTES {
            return Err(error(
                ErrorKind::Capacity,
                "Invalid GET report bound including selector",
            ));
        }
        let abi = abi::native()
            .ok_or_else(|| error(ErrorKind::Unsupported, "Linux ioctl ABI is unsupported"))?;
        let request = abi.get(kind, max_bytes).ok_or_else(|| {
            error(
                ErrorKind::Capacity,
                "GET length exceeds this Linux ABI's ioctl size field",
            )
        })?;
        if let Some(previous) = self.pending.as_ref() {
            match previous.result.try_recv() {
                Err(mpsc::TryRecvError::Empty) => {
                    return Err(error(
                        ErrorKind::Busy,
                        "Previous timed-out hidraw GET is still pending",
                    ))
                }
                _ => {
                    self.pending.take();
                }
            }
        }
        let lease = Arc::clone(&self.lease);
        let calls = Arc::clone(&self.calls);
        let (send, result) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("hidraw-read-only-get".into())
            .spawn(move || {
                let mut bytes = vec![0; max_bytes];
                bytes[0] = report_id;
                let result = calls
                    .ioctl(lease.file.as_raw_fd(), request, &mut bytes)
                    .map_err(|e| os("Explicit hidraw GET", e))
                    .and_then(|count| {
                        if count == 0 || count > bytes.len() {
                            return Err(error(ErrorKind::Io, "GET returned invalid byte count"));
                        }
                        bytes.truncate(count);
                        if report_id != 0 && bytes[0] != report_id {
                            return Err(error(ErrorKind::Io, "GET returned a different report ID"));
                        }
                        Ok(bytes)
                    });
                let _ = send.send(result);
                // `lease` holds the fd/path permit even if the caller timed out/dropped.
            })
            .map_err(|e| os("Start bounded hidraw GET worker", e))?;
        self.pending = Some(PendingGet { result });
        match self
            .pending
            .as_ref()
            .unwrap()
            .result
            .recv_timeout(timeout.saturating_sub(started.elapsed()))
        {
            Ok(result) => {
                self.pending.take();
                result.map(|bytes| GetReport {
                    request_id: request_id.into(),
                    bytes,
                })
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Err(error(
                ErrorKind::Timeout,
                "hidraw GET deadline expired; late result fenced",
            )),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.pending.take();
                Err(error(ErrorKind::Io, "hidraw GET worker disconnected"))
            }
        }
    }
}
