//! Every SDK operation runs in a short-lived process. The qualified SDK has a
//! global callback, shared packet buffer and unjoinable threads; process
//! isolation prevents old-slot callbacks from entering a later capture.
mod callback;
mod configured_descriptor;
mod inspection;
mod native;

use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const INSPECT: &str = "--osg-vjoy-ffb-debug-inspect";
const CAPTURE: &str = "--osg-vjoy-ffb-debug-capture";
const MAX_LINE: u64 = 524_288;
type Sink = Arc<dyn Fn(u32, Vec<u8>) + Send + Sync>;

fn validate_slot(slot: u8) -> Result<(), String> {
    if (1..=16).contains(&slot) {
        Ok(())
    } else {
        Err("vJoy slot must be 1–16.".into())
    }
}
fn spawn(argument: &str, slot: u8) -> Result<Child, String> {
    validate_slot(slot)?;
    Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .args([argument, &slot.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .spawn()
        .map_err(|e| format!("Could not start isolated vJoy SDK helper: {e}"))
}
fn line(reader: &mut impl BufRead) -> Result<Option<Value>, String> {
    let mut bytes = Vec::new();
    let read = reader
        .take(MAX_LINE + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(|e| format!("vJoy helper read failed: {e}"))?;
    if read == 0 {
        return Ok(None);
    }
    if bytes.len() as u64 > MAX_LINE || bytes.last() != Some(&b'\n') {
        return Err("vJoy helper exceeded the message bound or disconnected mid-message.".into());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| format!("Invalid vJoy helper response: {e}"))
}
fn response(value: Value) -> Result<Value, String> {
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Err(error.to_owned());
    }
    if value["backend"] != "vjoy-ffb-sdk" || value["ok"] != true {
        return Err("Unexpected vJoy SDK helper acknowledgement.".into());
    }
    Ok(value)
}
fn retire(child: &mut Child) {
    drop(child.stdin.take()); // EOF requests graceful detach and relinquishment.
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
        }
    }
}

pub(super) fn inspect(slot: u8) -> Result<Value, String> {
    inspect_child(spawn(INSPECT, slot)?, slot)
}
fn inspect_child(mut child: Child, slot: u8) -> Result<Value, String> {
    let stdout = child
        .stdout
        .take()
        .ok_or("vJoy inspection stdout is unavailable.")?;
    let (tx, rx) = mpsc::sync_channel(1);
    let reader = thread::spawn(move || {
        let result = line(&mut BufReader::new(stdout))
            .and_then(|v| v.ok_or_else(|| "vJoy inspection helper exited before replying.".into()))
            .and_then(response);
        let _ = tx.send(result);
    });
    let result = rx
        .recv_timeout(Duration::from_secs(12))
        .map_err(|_| "vJoy SDK inspection timed out or disconnected.".to_string())
        .and_then(|v| v)
        .and_then(|v| {
            if v["inspection"].is_object() && v["inspection"]["slotId"] == slot {
                Ok(v["inspection"].clone())
            } else {
                Err("vJoy inspection returned another slot or no capabilities.".into())
            }
        });
    retire(&mut child);
    let _ = reader.join();
    result
}

pub(super) struct Capture {
    pub(super) capabilities: Value,
    child: Arc<Mutex<Child>>,
    reader: Option<JoinHandle<()>>,
    stopping: Arc<AtomicBool>,
    failure: Arc<Mutex<Option<String>>>,
}
impl Capture {
    pub(super) fn failure(&self) -> Option<String> {
        self.failure
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        retire(&mut self.child.lock().unwrap_or_else(|e| e.into_inner()));
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

pub(super) fn start(slot: u8, sink: Sink) -> Result<Capture, String> {
    validate_slot(slot)?;
    if slot == 16 {
        return Err("vJoy FFB receive uses four-bit device IDs and supports slots 1–15.".into());
    }
    start_child(spawn(CAPTURE, slot)?, slot, sink)
}
fn start_child(mut child: Child, slot: u8, sink: Sink) -> Result<Capture, String> {
    let stdout = child
        .stdout
        .take()
        .ok_or("vJoy capture stdout is unavailable.")?;
    let stopping = Arc::new(AtomicBool::new(false));
    let failure = Arc::new(Mutex::new(None));
    let child = Arc::new(Mutex::new(child));
    let failed_child = child.clone();
    let (ready, acknowledgement) = mpsc::sync_channel(1);
    let stop = stopping.clone();
    let fault = failure.clone();
    let reader = thread::spawn(move || {
        let mut stdout = BufReader::new(stdout);
        let handshake = line(&mut stdout)
            .and_then(|v| v.ok_or_else(|| "vJoy capture helper exited before acquisition.".into()))
            .and_then(response)
            .and_then(|v| {
                if v["slotId"] == slot
                    && v["acquired"] == true
                    && v["capabilities"].is_object()
                    && v["capabilities"]["slotId"] == slot
                {
                    Ok(v["capabilities"].clone())
                } else {
                    Err(
                        "vJoy capture helper acknowledged a different slot or did not acquire it."
                            .into(),
                    )
                }
            });
        let accepted = handshake.is_ok();
        let _ = ready.send(handshake);
        if !accepted {
            return;
        }
        let result = read_packets(&mut stdout, &stop, &sink, slot);
        if !stop.load(Ordering::Acquire) {
            *fault.lock().unwrap_or_else(|e| e.into_inner()) = Some(
                result
                    .err()
                    .unwrap_or_else(|| "vJoy SDK helper disconnected.".into()),
            );
            // A dead parent reader must also release the SDK owner, even if
            // no more effects arrive to reveal the broken stdout pipe.
            retire(&mut failed_child.lock().unwrap_or_else(|e| e.into_inner()));
        }
    });
    let mut capture = Capture {
        capabilities: Value::Null,
        child,
        reader: Some(reader),
        stopping,
        failure,
    };
    let capabilities = acknowledgement
        .recv_timeout(Duration::from_secs(12))
        .map_err(|_| "vJoy SDK acquisition timed out or disconnected.".to_string())??;
    // A dropped Capture closes the helper on every failure path above.
    if let Some(error) = capture.failure() {
        return Err(error);
    }
    capture.capabilities = capabilities;
    Ok(capture)
}

fn read_packets(
    reader: &mut impl BufRead,
    stop: &AtomicBool,
    sink: &Sink,
    slot: u8,
) -> Result<(), String> {
    while let Some(value) = line(reader)? {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        if let Some(error) = value["error"].as_str() {
            return Err(error.to_owned());
        }
        let command = value["command"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or("Invalid vJoy callback command.")?;
        let payload = value["payload"]
            .as_array()
            .filter(|p| !p.is_empty() && p.len() <= 248)
            .ok_or("Invalid vJoy callback payload.")?;
        let payload: Result<Vec<_>, _> = payload
            .iter()
            .map(|v| {
                v.as_u64()
                    .and_then(|n| u8::try_from(n).ok())
                    .ok_or("Invalid vJoy payload byte.")
            })
            .collect();
        let bytes = payload?;
        if bytes[0] >> 4 != slot {
            return Err("vJoy helper returned another slot's FFB packet.".into());
        }
        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink(command, bytes)));
        if outcome.is_err() {
            return Err("vJoy debug event sink panicked.".into());
        }
    }
    Ok(())
}

fn emit(value: Value) -> Result<(), String> {
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    serde_json::to_writer(&mut stdout, &value).map_err(|e| e.to_string())?;
    stdout
        .write_all(b"\n")
        .and_then(|_| stdout.flush())
        .map_err(|e| e.to_string())
}

fn capture_helper(api: &native::Api, slot: u8) -> Result<(), String> {
    capture_with(api, slot, emit, |sender| {
        thread::spawn(move || {
            let mut byte = [0];
            let _ = std::io::stdin().read(&mut byte);
            let _ = sender.send(callback::Message::Stop);
        });
    })
}

fn capture_with(
    api: &native::Api,
    slot: u8,
    mut emit: impl FnMut(Value) -> Result<(), String>,
    after_ready: impl FnOnce(mpsc::SyncSender<callback::Message>),
) -> Result<(), String> {
    let inspected = inspection::inspect(api, slot)?;
    if inspected["captureAvailable"] != true {
        return Err(format!(
            "vJoy slot {slot} must be FREE and force-feedback capable; current status: {}.",
            inspected["status"]
        ));
    }
    let (sender, receiver) = mpsc::sync_channel(256);
    callback::install(slot, sender.clone())?;
    unsafe {
        (api.register)(Some(callback::receive), std::ptr::null_mut());
    }
    // Recheck immediately before acquiring. AcquireVJD's native open resolves
    // a competing feeder race; this process has never owned any slot.
    if unsafe { (api.status)(u32::from(slot)) } != 1
        || unsafe { (api.acquire)(u32::from(slot)) } != 1
    {
        callback::detach();
        return Err("vJoy acquisition failed; another feeder may own the slot.".into());
    }
    struct Owned<'a>(&'a native::Api, u8);
    impl Drop for Owned<'_> {
        fn drop(&mut self) {
            callback::detach();
            unsafe {
                (self.0.release)(u32::from(self.1));
            }
        }
    }
    let _owned = Owned(api, slot);
    if unsafe { (api.status)(u32::from(slot)) } != 0 {
        return Err("vJoy SDK did not retain ownership after acquisition.".into());
    }
    emit(
        json!({"ok":true,"backend":"vjoy-ffb-sdk","slotId":slot,"acquired":true,"capabilities":inspected}),
    )?;
    after_ready(sender);
    loop {
        let message = match receiver.recv_timeout(Duration::from_millis(500)) {
            Ok(message) => message,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if unsafe { (api.status)(u32::from(slot)) } != 0 {
                    return Err(
                        "vJoy SDK capture lost slot ownership or the driver was disconnected."
                            .into(),
                    );
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("vJoy callback channel disconnected.".into())
            }
        };
        if callback::OVERFLOW.load(Ordering::Acquire) {
            return Err("vJoy FFB callback queue overflowed; capture lost packets.".into());
        }
        match message {
            callback::Message::Packet(command, payload) => {
                emit(json!({"command":command,"payload":payload}))?
            }
            callback::Message::Error(error) => return Err(error),
            callback::Message::Stop => return Ok(()),
        }
    }
}

/// Must be called before initializing the desktop/browser-dev host. A matched
/// helper always exits here, so SDK threads cannot survive into host startup.
pub(super) fn run_helper() -> bool {
    let arguments: Vec<_> = std::env::args().collect();
    let Some(_) = arguments
        .get(1)
        .filter(|v| matches!(v.as_str(), INSPECT | CAPTURE))
    else {
        return false;
    };
    let result = helper_result(&arguments, native::Api::open, emit);
    if let Err(error) = &result {
        let _ = emit(json!({"error":error}));
    }
    std::process::exit(if result.is_ok() { 0 } else { 1 });
}

fn helper_result(
    arguments: &[String],
    open: impl FnOnce() -> Result<native::Api, String>,
    mut emit: impl FnMut(Value) -> Result<(), String>,
) -> Result<(), String> {
    if arguments.len() != 3 {
        return Err("Invalid vJoy SDK helper arguments.".into());
    }
    let slot = arguments[2]
        .parse::<u8>()
        .map_err(|_| "Invalid vJoy SDK helper slot.")?;
    validate_slot(slot)?;
    let api = open()?;
    if arguments[1] == INSPECT {
        emit(
            json!({"ok":true,"backend":"vjoy-ffb-sdk","inspection":inspection::inspect(&api, slot)?}),
        )
    } else {
        capture_helper(&api, slot)
    }
}

#[cfg(test)]
mod tests;
