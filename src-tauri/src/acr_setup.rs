//! Local ACR setup evidence. Shared memory is native; no game/plugin files change.
//! Only this module's Windows acquisition boundary can produce native verification.
use crate::{
    game_discovery::{Platform, ProcessIdentity},
    game_telemetry::{self, ChannelStatus, RawSample, Session, Snapshot, Source, SourceIdentity},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::Path,
    time::{Duration, Instant, UNIX_EPOCH},
};
const MAX_EXECUTABLE: u64 = 512 * 1024 * 1024;
const PROBE_MS: u64 = 2500;
const FRESHNESS_MS: u64 = 1500;
pub(crate) const LAYOUT_REVISION: &str = "acr-prefix-1";
fn paths_match(left: &str, right: &str) -> bool {
    left.replace('/', "\\")
        .trim_start_matches("\\\\?\\")
        .to_lowercase()
        == right
            .replace('/', "\\")
            .trim_start_matches("\\\\?\\")
            .to_lowercase()
}
fn basename(image: &str) -> &str {
    image.rsplit(['\\', '/']).next().unwrap_or(image)
}
fn is_competing_image(image: &str) -> bool {
    matches!(
        basename(image).to_ascii_lowercase().as_str(),
        "acr.exe"
            | "acs.exe"
            | "acs_x86.exe"
            | "ac2.exe"
            | "ac2-win64-shipping.exe"
            | "acc.exe"
            | "assettocorsa.exe"
    )
}
/// Stable durable evidence for a selected executable, never a persisted process PID.
pub(crate) fn fingerprint(executable: &Path) -> Result<Value, String> {
    if !executable.is_absolute()
        || !executable
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("acr.exe"))
    {
        return Err("Select an existing absolute acr.exe executable for ACR integration.".into());
    }
    let runtime = executable
        .parent()
        .map(|root| root.join("acr/Binaries/Win64/acr.exe"))
        .filter(|runtime| runtime.is_file())
        .unwrap_or_else(|| executable.to_path_buf());
    let path = fs::canonicalize(&runtime)
        .map_err(|e| format!("Cannot access the selected ACR executable: {e}"))?;
    let before = fs::metadata(&path).map_err(|e| e.to_string())?;
    if !before.is_file() || before.len() == 0 || before.len() > MAX_EXECUTABLE {
        return Err(
            "The selected ACR executable is empty or exceeds the 512 MiB verification limit."
                .into(),
        );
    }
    let mut file = fs::File::open(&path)
        .map_err(|e| format!("Cannot read the selected ACR executable: {e}"))?;
    let mut digest = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| format!("Cannot verify the ACR executable: {e}"))?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        if bytes > MAX_EXECUTABLE {
            return Err("The ACR executable changed or exceeds the verification limit.".into());
        }
        digest.update(&buffer[..n]);
    }
    let after = fs::metadata(&path).map_err(|e| e.to_string())?;
    if bytes != before.len()
        || after.len() != before.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(
            "The ACR executable changed during verification. Recheck after its update completes."
                .into(),
        );
    }
    let sha256 = format!("{:x}", digest.finalize());
    let modified = after
        .modified()
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().to_string());
    Ok(
        json!({"fingerprint":format!("sha256:{sha256}"),"sha256":sha256,"fileBytes":bytes,"fileVersion":file_version(&path)?,"executable":path,"modifiedUnixNanos":modified}),
    )
}
#[cfg(not(windows))]
fn file_version(_path: &Path) -> Result<Option<String>, String> {
    Ok(None)
}
#[cfg(windows)]
fn file_version(path: &Path) -> Result<Option<String>, String> {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt};
    #[repr(C)]
    struct Fixed {
        signature: u32,
        structure: u32,
        file_ms: u32,
        file_ls: u32,
        product_ms: u32,
        product_ls: u32,
        flags_mask: u32,
        flags: u32,
        os: u32,
        kind: u32,
        subtype: u32,
        date_ms: u32,
        date_ls: u32,
    }
    #[link(name = "version")]
    unsafe extern "system" {
        fn GetFileVersionInfoSizeW(file: *const u16, handle: *mut u32) -> u32;
        fn GetFileVersionInfoW(file: *const u16, handle: u32, size: u32, data: *mut c_void) -> i32;
        fn VerQueryValueW(
            data: *const c_void,
            block: *const u16,
            value: *mut *mut c_void,
            length: *mut u32,
        ) -> i32;
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut handle = 0;
    let size = unsafe { GetFileVersionInfoSizeW(wide.as_ptr(), &mut handle) };
    if size == 0 {
        return Ok(None);
    }
    if size > 65536 {
        return Err("The ACR executable has oversized version metadata.".into());
    }
    // Aligned storage for the Windows version resource and its fixed information.
    let mut data = vec![0u32; (size as usize + 3) / 4];
    if unsafe { GetFileVersionInfoW(wide.as_ptr(), 0, size, data.as_mut_ptr().cast()) } == 0 {
        return Err(format!(
            "Cannot read the ACR executable version: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut value = std::ptr::null_mut();
    let mut length = 0;
    let root = [b'\\' as u16, 0];
    if unsafe { VerQueryValueW(data.as_ptr().cast(), root.as_ptr(), &mut value, &mut length) } == 0
        || value.is_null()
        || length < std::mem::size_of::<Fixed>() as u32
    {
        return Err("The ACR executable version metadata is malformed.".into());
    }
    let fixed = unsafe { &*value.cast::<Fixed>() };
    if fixed.signature != 0xfeef04bd {
        return Err("The ACR executable version signature is invalid.".into());
    }
    Ok(Some(format!(
        "{}.{}.{}.{}",
        fixed.file_ms >> 16,
        fixed.file_ms & 65535,
        fixed.file_ls >> 16,
        fixed.file_ls & 65535
    )))
}
#[cfg(not(windows))]
pub(crate) fn competing_processes(_selected_pid: u32) -> Result<Vec<Value>, String> {
    Err("Checking native ACR mapping producers requires Windows.".into())
}
#[cfg(windows)]
pub(crate) fn competing_processes(selected_pid: u32) -> Result<Vec<Value>, String> {
    use std::{ffi::c_void, mem};
    type Handle = *mut c_void;
    #[repr(C)]
    struct Entry {
        size: u32,
        usage: u32,
        pid: u32,
        heap: usize,
        module: u32,
        threads: u32,
        parent: u32,
        priority: i32,
        flags: u32,
        executable: [u16; 260],
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> Handle;
        fn Process32FirstW(snapshot: Handle, entry: *mut Entry) -> i32;
        fn Process32NextW(snapshot: Handle, entry: *mut Entry) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }
    struct Owned(Handle);
    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }
    let raw = unsafe { CreateToolhelp32Snapshot(2, 0) };
    if raw.is_null() || raw as isize == -1 {
        return Err(format!(
            "Cannot check competing Assetto processes: {}",
            std::io::Error::last_os_error()
        ));
    }
    let snapshot = Owned(raw);
    let mut entry: Entry = unsafe { mem::zeroed() };
    entry.size = mem::size_of::<Entry>() as u32;
    let mut results = Vec::new();
    if unsafe { Process32FirstW(snapshot.0, &mut entry) } == 0 {
        return Err(format!(
            "Cannot inspect Assetto process identities: {}",
            std::io::Error::last_os_error()
        ));
    }
    for _ in 0..16384 {
        let length = entry
            .executable
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(entry.executable.len());
        let image = String::from_utf16_lossy(&entry.executable[..length]);
        if entry.pid != selected_pid && is_competing_image(&image) {
            results.push(json!({"pid":entry.pid,"image":image}));
        }
        if unsafe { Process32NextW(snapshot.0, &mut entry) } == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(18) {
                return Err(format!(
                    "Cannot complete Assetto process inspection: {error}"
                ));
            }
            return Ok(results);
        }
    }
    Err("Assetto process enumeration exceeded its bounded limit.".into())
}
fn conflict_message(competitors: &[Value]) -> String {
    format!("Another Assetto process is running ({}). Close competing Assetto games before verifying ACR; shared mapping names do not identify their producer.",competitors.iter().map(|p|format!("{} · PID {}",p["image"].as_str().unwrap_or("unknown"),p["pid"])).collect::<Vec<_>>().join(", "))
}
pub(crate) fn ensure_single_producer(pid: u32) -> Result<(), String> {
    let competitors = competing_processes(pid)?;
    if competitors.is_empty() {
        Ok(())
    } else {
        Err(conflict_message(&competitors))
    }
}
fn required_channels_valid(snapshot: &Snapshot) -> bool {
    snapshot.fresh
        && ["rpm", "gear", "speed_kmh"].iter().all(|id| {
            snapshot
                .channels
                .get(*id)
                .is_some_and(|c| c.status == ChannelStatus::Ready && c.value.is_some())
        })
}
fn check_context(sample: &RawSample) -> Result<i32, String> {
    let reading = game_telemetry::parse(sample)?;
    if reading.status == 2 {
        if reading.context.shared_memory_version.is_empty()
            || reading.context.game_version.is_empty()
        {
            return Err("ACR telemetry does not identify its shared-memory/game version. This layout remains unverified.".into());
        }
        if reading.context.car_model.is_empty() || reading.context.track.is_empty() {
            return Err(
                "ACR has no car/track context yet. Enter a live driving session and recheck."
                    .into(),
            );
        }
    }
    Ok(reading.status)
}
/// Pure assessment used with native acquisition; it has no nativeVerified success flag.
struct Verifier {
    session: Session,
    expected: SourceIdentity,
    samples: usize,
    last_status: Option<i32>,
}
impl Verifier {
    fn new(identity: &ProcessIdentity) -> Result<Self, String> {
        Ok(Self {
            session: Session::new(FRESHNESS_MS)?,
            expected: SourceIdentity {
                id: format!("acr-pid-{}", identity.pid),
                epoch: identity.epoch,
            },
            samples: 0,
            last_status: None,
        })
    }
    fn observe(&mut self, sample: &RawSample, now: u64) -> Result<Snapshot, String> {
        if sample.source != self.expected {
            return Err("The selected ACR process identity changed. Reattach to the new process and establish a fresh baseline.".into());
        }
        self.last_status = Some(check_context(sample)?);
        self.samples += 1;
        Ok(self.session.feed(sample, now))
    }
    fn assessment(&mut self, now: u64) -> Value {
        let snapshot = self.session.tick(now);
        let compatible = required_channels_valid(&snapshot);
        let (status, message) = if compatible {
            (
                "receiving",
                "Receiving native-compatible telemetry; RPM, gear and speed are available.",
            )
        } else if snapshot.fresh {
            (
                "unsupported-layout",
                "ACR reports invalid RPM, gear or speed values. This layout remains unverified.",
            )
        } else if self.last_status == Some(0) {
            (
                "waiting",
                "The game is in a menu or has no active driving session.",
            )
        } else if self.last_status == Some(1) {
            (
                "replay",
                "Replay telemetry cannot verify a live driving session.",
            )
        } else if self.last_status == Some(3) {
            ("paused", "The game is paused. Resume driving and recheck.")
        } else if snapshot.status == "stale" || self.samples > 1 {
            ("stale","The original physics packet IDs have not advanced. Enter a live session and recheck.")
        } else {
            (
                "waiting",
                "Waiting for a live session and advancing original physics packets.",
            )
        };
        let max_rpm = snapshot
            .channels
            .get("max_rpm")
            .is_some_and(|c| c.status == ChannelStatus::Ready && c.value.is_some());
        let mut diagnostics = vec![message.to_string()];
        if compatible && !max_rpm {
            diagnostics.push("Maximum RPM is unavailable. RPM, gear and speed still work; rev-ratio bindings need a valid maximum RPM. No redline is inferred from observed RPM.".into());
        }
        json!({"compatible":compatible,"status":status,"diagnostics":diagnostics,"channels":snapshot.value()["channels"],"context":snapshot.value()["context"],"packetId":snapshot.packet_id,"maxRpmAvailable":max_rpm,"revRatioAvailable":compatible&&max_rpm,"layoutRevision":LAYOUT_REVISION})
    }
}
fn failure_status(error: &str) -> &'static str {
    let text = error.to_ascii_lowercase();
    if text.contains("another assetto process") {
        "conflicting-producer"
    } else if text.contains("os error 5") || text.contains("access is denied") {
        "access-denied"
    } else if text.contains("exited")
        || text.contains("no longer running")
        || text.contains("identity changed")
    {
        "process-exited"
    } else if text.contains("not available") && text.contains("os error 2") {
        "waiting"
    } else if text.contains("car/track context") {
        "waiting"
    } else if text.contains("changed during all bounded") {
        "waiting"
    } else {
        "unsupported-layout"
    }
}
fn result(fingerprint: &Value, status: &str, diagnostics: Vec<String>) -> Value {
    json!({"nativeVerified":false,"status":status,"diagnostics":diagnostics,"channels":{},"fingerprint":fingerprint,"layoutRevision":LAYOUT_REVISION,"gameFilesChanged":false})
}
/// Acquisition and timing seams exercise the actual process/packet orchestration.
/// This helper never asserts native verification, even with compatible authored frames.
struct AttachedSource {
    source: Box<dyn Source>,
    image: Option<String>,
}
fn probe_acquired(
    game: &Value,
    platform: &dyn Platform,
    fingerprint: &Value,
    check: impl Fn() -> bool,
    progress: impl Fn(&str, u8),
    acquire: impl FnOnce(&ProcessIdentity) -> Result<AttachedSource, String>,
    competitors: impl Fn(u32) -> Result<Vec<Value>, String>,
    mut clock: impl FnMut() -> u64,
    wait: impl Fn(),
) -> Result<Value, String> {
    let Some(process) = platform.find(game)? else {
        return Ok(result(fingerprint,"waiting",vec!["ACR is configured for native shared memory. Start the game, enter a live driving session and recheck; no game plugin or file changes are needed.".into()]));
    };
    if !paths_match(
        &process.path,
        fingerprint["executable"].as_str().unwrap_or(""),
    ) {
        return Ok(result(fingerprint,"process-exited",vec!["The selected ACR process does not match this installation. Recheck discovery after its launch handoff.".into()]));
    }
    let competing = competitors(process.pid)?;
    if !competing.is_empty() {
        return Ok(result(
            fingerprint,
            "conflicting-producer",
            vec![conflict_message(&competing)],
        ));
    }
    let mut attached = match acquire(&process) {
        Ok(source) => source,
        Err(error) => return Ok(result(fingerprint, failure_status(&error), vec![error])),
    };
    // Close the same acquisition on every success/error/cancel path.
    let outcome = (|| {
        if !attached
            .image
            .as_deref()
            .is_some_and(|image| paths_match(image, &process.path))
        {
            return Ok(result(
                fingerprint,
                "process-exited",
                vec!["The ACR process identity changed before telemetry attachment.".into()],
            ));
        }
        let mut verifier = Verifier::new(&process)?;
        let mut last_error = None;
        loop {
            if check() {
                return Err("ACR integration verification was cancelled.".into());
            }
            if !platform.alive(&process)? {
                return Ok(result(fingerprint,"process-exited",vec!["The selected ACR process exited. Recheck after relaunch to attach its new identity.".into()]));
            }
            let competing = competitors(process.pid)?;
            if !competing.is_empty() {
                return Ok(result(
                    fingerprint,
                    "conflicting-producer",
                    vec![conflict_message(&competing)],
                ));
            }
            let now = clock();
            progress(
                "Checking native ACR telemetry",
                (15 + now.min(PROBE_MS) * 75 / PROBE_MS) as u8,
            );
            match attached.source.poll() {
                Ok(Some(sample)) => match verifier.observe(&sample, now) {
                    Ok(snapshot) => {
                        last_error = None;
                        if required_channels_valid(&snapshot) {
                            let competing = competitors(process.pid)?;
                            if !competing.is_empty() {
                                return Ok(result(
                                    fingerprint,
                                    "conflicting-producer",
                                    vec![conflict_message(&competing)],
                                ));
                            }
                            if !platform.alive(&process)? {
                                return Ok(result(
                                    fingerprint,
                                    "process-exited",
                                    vec!["The ACR process exited before verification completed."
                                        .into()],
                                ));
                            }
                            let executable = Path::new(
                                game["executable"]
                                    .as_str()
                                    .ok_or("Select the ACR installation first.")?,
                            );
                            let after = crate::acr_setup::fingerprint(executable)?;
                            if after["fingerprint"] != fingerprint["fingerprint"] {
                                return Ok(result(&after,"executable-changed",vec!["ACR was updated during verification. Recheck its new version.".into()]));
                            }
                            let mut assessed = verifier.assessment(now);
                            assessed["nativeVerified"] = json!(false);
                            assessed["fingerprint"] = fingerprint.clone();
                            assessed["process"] = process.value();
                            assessed["gameFilesChanged"] = json!(false);
                            return Ok(assessed);
                        }
                    }
                    Err(error) => {
                        return Ok(result(fingerprint, failure_status(&error), vec![error]))
                    }
                },
                Ok(None) => {}
                Err(error) => {
                    if failure_status(&error) != "waiting" {
                        return Ok(result(fingerprint, failure_status(&error), vec![error]));
                    }
                    last_error = Some(error);
                }
            }
            if now >= PROBE_MS {
                if let Some(error) = last_error {
                    return Ok(result(
                        fingerprint,
                        "waiting",
                        vec![error, "Enter a live driving session and recheck.".into()],
                    ));
                }
                let mut assessed = verifier.assessment(now);
                assessed["nativeVerified"] = json!(false);
                assessed["fingerprint"] = fingerprint.clone();
                assessed["process"] = process.value();
                assessed["gameFilesChanged"] = json!(false);
                return Ok(assessed);
            }
            wait();
        }
    })();
    attached.source.close();
    outcome
}
/// Read-only bounded native check. Closed games never verify themselves or persist a PID.
pub(crate) fn probe(
    game: &Value,
    platform: &dyn Platform,
    check: impl Fn() -> bool,
    progress: impl Fn(&str, u8),
) -> Result<Value, String> {
    let executable = Path::new(
        game["executable"]
            .as_str()
            .ok_or("Select the ACR installation first.")?,
    );
    let fingerprint = fingerprint(executable)?;
    if check() {
        return Err("ACR integration verification was cancelled.".into());
    }
    if !cfg!(windows) {
        return Ok(result(&fingerprint,"unsupported-platform",vec!["Native ACR shared-memory verification requires Windows. Synthetic sources retain their separate capability.".into()]));
    }
    let mut start = None;
    let mut proof = probe_acquired(
        game,
        platform,
        &fingerprint,
        &check,
        &progress,
        |process| {
            // This is the sole acquisition boundary allowed to establish native evidence.
            let source = crate::acr_windows::WindowsSource::new(process.pid)?;
            let image = source.process_image().map(str::to_owned);
            Ok(AttachedSource {
                source: Box::new(source),
                image,
            })
        },
        competing_processes,
        || start.get_or_insert_with(Instant::now).elapsed().as_millis() as u64,
        || std::thread::sleep(Duration::from_millis(100)),
    )?;
    if proof["compatible"] == true {
        proof["nativeVerified"] = json!(true);
        progress("Native ACR telemetry verified", 100);
    }
    Ok(proof)
}

#[cfg(test)]
#[path = "acr_setup_tests.rs"]
mod tests;
