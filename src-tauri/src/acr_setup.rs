//! Local Assetto setup evidence. Shared memory is native; no game/plugin files change.
//! Only this module's Windows acquisition boundary can produce native verification.
use crate::{
    game_discovery::{Platform, ProcessIdentity},
    game_telemetry::{self, Game, ChannelStatus, RawSample, Session, Snapshot, Source, SourceIdentity},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const MAX_EXECUTABLE: u64 = 512 * 1024 * 1024;
const PROBE_MS: u64 = 2500;
const FRESHNESS_MS: u64 = 1500;
pub(crate) const LAYOUT_REVISION: &str = "acr-prefix-3";
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
            | "ac2-win64-shipping.exe"
            | "assettocorsaevo.exe"
    )
}
fn shares_mapping(game: Game, image: &str) -> bool {
    is_competing_image(image)
        && (game == Game::Evo) == basename(image).eq_ignore_ascii_case("AssettoCorsaEVO.exe")
}
/// Stable durable evidence for a selected executable, never a persisted process PID.
pub(crate) fn fingerprint(executable: &Path) -> Result<Value, String> {
    fingerprint_for_game(executable, Game::Rally)
}
fn executable_path(executable: &Path, game: Game) -> Result<PathBuf, String> {
    let names: &[&str] = match game {
        Game::Rally => &["acr.exe"],
        Game::Corsa => &["acs.exe", "acs_x86.exe"],
        Game::Competizione => &["AC2.exe", "AC2-Win64-Shipping.exe"],
        Game::Evo => &["AssettoCorsaEVO.exe"],
    };
    if !executable.is_absolute()
        || !executable
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| names.iter().any(|name| s.eq_ignore_ascii_case(name)))
    {
        return Err(format!("Select an existing absolute {} executable for {} integration.", names[0], game.id()));
    }
    let runtime = executable
        .parent()
        .and_then(|root| match game {
            Game::Rally => Some(root.join("acr/Binaries/Win64/acr.exe")),
            Game::Competizione => Some(root.join("AC2/Binaries/Win64/AC2-Win64-Shipping.exe")),
            _ => None,
        })
        .filter(|runtime| runtime.is_file())
        .unwrap_or_else(|| executable.to_path_buf());
    let path = fs::canonicalize(&runtime)
        .map_err(|e| format!("Cannot access the selected Assetto executable: {e}"))?;
    Ok(path)
}
fn executable_metadata(path: &Path) -> Result<fs::Metadata, String> {
    let metadata = fs::metadata(path).map_err(|error|error.to_string())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_EXECUTABLE {
        return Err("The selected Assetto executable is empty or exceeds the 512 MiB verification limit.".into());
    }
    Ok(metadata)
}
#[derive(Clone, PartialEq, Eq)]
struct ExecutableStamp {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
}
impl ExecutableStamp {
    fn read(executable: &Path, game: Game) -> Result<Self, String> {
        let path = executable_path(executable, game)?;
        let metadata = executable_metadata(&path)?;
        Ok(Self { path, bytes:metadata.len(), modified:metadata.modified().map_err(|error|format!("Cannot inspect game executable modification time: {error}"))?, created:metadata.created().ok() })
    }
}
/// Passive status may reuse a content hash only while the current executable's
/// identity and metadata match. One retained entry per dialect bounds this to
/// four records. Explicit setup/check and new profile preparation bypass reuse.
#[derive(Default)]
pub(crate) struct FingerprintCache {
    entries: BTreeMap<&'static str, (ExecutableStamp, Value)>,
}
impl FingerprintCache {
    pub(crate) fn inspect(&mut self, executable: &Path, game: Game, force_hash: bool) -> Result<Value, String> {
        self.inspect_with(executable, game, force_hash, fingerprint_for_game)
    }
    fn inspect_with(&mut self, executable: &Path, game: Game, force_hash: bool, mut hash: impl FnMut(&Path, Game) -> Result<Value, String>) -> Result<Value, String> {
        let stamp = match ExecutableStamp::read(executable, game) {
            Ok(stamp) => stamp,
            Err(error) => { self.entries.remove(game.id()); return Err(error); }
        };
        if !force_hash {
            if let Some((known, value)) = self.entries.get(game.id()) {
                if *known == stamp { return Ok(value.clone()); }
            }
        }
        self.entries.remove(game.id());
        let value = hash(&stamp.path, game)?;
        if ExecutableStamp::read(executable, game)? != stamp {
            return Err("The Assetto executable changed during inspection. Recheck its updated build.".into());
        }
        self.entries.insert(game.id(), (stamp, value.clone()));
        Ok(value)
    }
}
pub(crate) fn fingerprint_for_game(executable: &Path, game: Game) -> Result<Value, String> {
    let path = executable_path(executable, game)?;
    let before = executable_metadata(&path)?;
    let mut file = fs::File::open(&path)
        .map_err(|e| format!("Cannot read the selected Assetto executable: {e}"))?;
    let mut digest = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| format!("Cannot verify the Assetto executable: {e}"))?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        if bytes > MAX_EXECUTABLE {
            return Err("The Assetto executable changed or exceeds the verification limit.".into());
        }
        digest.update(&buffer[..n]);
    }
    let after = fs::metadata(&path).map_err(|e| e.to_string())?;
    if bytes != before.len()
        || after.len() != before.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(
            "The Assetto executable changed during verification. Recheck after its update completes."
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
        return Err("The Assetto executable has oversized version metadata.".into());
    }
    // Aligned storage for the Windows version resource and its fixed information.
    let mut data = vec![0u32; (size as usize + 3) / 4];
    if unsafe { GetFileVersionInfoW(wide.as_ptr(), 0, size, data.as_mut_ptr().cast()) } == 0 {
        return Err(format!(
            "Cannot read the Assetto executable version: {}",
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
        return Err("The Assetto executable version metadata is malformed.".into());
    }
    let fixed = unsafe { &*value.cast::<Fixed>() };
    if fixed.signature != 0xfeef04bd {
        return Err("The Assetto executable version signature is invalid.".into());
    }
    Ok(Some(format!(
        "{}.{}.{}.{}",
        fixed.file_ms >> 16,
        fixed.file_ms & 65535,
        fixed.file_ls >> 16,
        fixed.file_ls & 65535
    )))
}
pub(crate) fn competing_processes(selected_pid: u32) -> Result<Vec<Value>, String> {
    competing_processes_for_game(selected_pid, Game::Rally)
}
#[cfg(not(windows))]
pub(crate) fn competing_processes_for_game(_selected_pid: u32, _game: Game) -> Result<Vec<Value>, String> {
    Err("Checking native Assetto mapping producers requires Windows.".into())
}
#[cfg(windows)]
pub(crate) fn competing_processes_for_game(selected_pid: u32, game: Game) -> Result<Vec<Value>, String> {
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
        if entry.pid != selected_pid && shares_mapping(game, &image) {
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
    format!("Another Assetto process is running ({}). Close competing Assetto games before verifying Assetto; shared mapping names do not identify their producer.",competitors.iter().map(|p|format!("{} · PID {}",p["image"].as_str().unwrap_or("unknown"),p["pid"])).collect::<Vec<_>>().join(", "))
}
pub(crate) fn ensure_single_producer(pid: u32) -> Result<(), String> {
    ensure_single_producer_for_game(pid, Game::Rally)
}
pub(crate) fn ensure_single_producer_for_game(pid: u32, game: Game) -> Result<(), String> {
    let competitors = competing_processes_for_game(pid, game)?;
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
fn check_context(sample: &RawSample, game: Game) -> Result<i32, String> {
    let reading = game_telemetry::parse_for_game(sample, game)?;
    if reading.status == 2 {
        if reading.context.shared_memory_version.is_empty()
            || reading.context.game_version.is_empty()
        {
            return Err("Assetto telemetry does not identify its shared-memory/game version. This layout remains unverified.".into());
        }
        if reading.context.car_model.is_empty() || reading.context.track.is_empty() {
            return Err(
                "Assetto has no car/track context yet. Enter a live driving session and recheck."
                    .into(),
            );
        }
    }
    Ok(if reading.telemetry_active() {
        2
    } else {
        reading.status
    })
}
/// Pure assessment used with native acquisition; it has no nativeVerified success flag.
struct Verifier {
    session: Session,
    game: Game,
    expected: SourceIdentity,
    samples: usize,
    last_status: Option<i32>,
    sparse_status: bool,
}
impl Verifier {
    fn new(identity: &ProcessIdentity) -> Result<Self, String> {
        Self::new_for_game(identity, Game::Rally)
    }
    fn new_for_game(identity: &ProcessIdentity, game: Game) -> Result<Self, String> {
        Ok(Self {
            session: Session::new_for_game(FRESHNESS_MS, game)?,
            game,
            expected: SourceIdentity {
                id: game.source_id(identity.pid),
                epoch: identity.epoch,
            },
            samples: 0,
            last_status: None,
            sparse_status: false,
        })
    }
    fn observe(&mut self, sample: &RawSample, now: u64) -> Result<Snapshot, String> {
        if sample.source != self.expected {
            return Err("The selected Assetto process identity changed. Reattach to the new process and establish a fresh baseline.".into());
        }
        self.sparse_status = game_telemetry::parse_for_game(sample, self.game)?.sparse_status();
        self.last_status = Some(check_context(sample, self.game)?);
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
                "Assetto reports invalid RPM, gear or speed values. This layout remains unverified.",
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
        if self.sparse_status {
            diagnostics.push("This Assetto build omits session status and version strings. Telemetry freshness uses advancing physics packets; pause/menu/replay detection is unavailable unless the game reports a status or stops packets.".into());
        }
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
    let Some(process) = platform.integration_process(game)? else {
        return Ok(result(fingerprint,"waiting",vec!["Assetto is configured for native shared memory. Start the game, enter a live driving session and recheck; no game plugin or file changes are needed.".into()]));
    };
    if !paths_match(
        &process.path,
        fingerprint["executable"].as_str().unwrap_or(""),
    ) {
        return Ok(result(fingerprint,"process-exited",vec!["The selected Assetto process does not match this installation. Recheck discovery after its launch handoff.".into()]));
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
                vec!["The Assetto process identity changed before telemetry attachment.".into()],
            ));
        }
        let telemetry_game = Game::from_id(game["id"].as_str().unwrap_or("assetto-corsa-rally"))?;
        let mut verifier = Verifier::new_for_game(&process, telemetry_game)?;
        let mut last_error = None;
        loop {
            if check() {
                return Err("Assetto integration verification was cancelled.".into());
            }
            if !platform.alive(&process)? {
                return Ok(result(fingerprint,"process-exited",vec!["The selected Assetto process exited. Recheck after relaunch to attach its new identity.".into()]));
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
                "Checking native Assetto telemetry",
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
                                    vec!["The Assetto process exited before verification completed."
                                        .into()],
                                ));
                            }
                            let executable = Path::new(
                                game["executable"]
                                    .as_str()
                                    .ok_or("Select the Assetto installation first.")?,
                            );
                            let after = fingerprint_for_game(executable, telemetry_game)?;
                            if after["fingerprint"] != fingerprint["fingerprint"] {
                                return Ok(result(&after,"executable-changed",vec!["Assetto was updated during verification. Recheck its new version.".into()]));
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
            .ok_or("Select the Assetto installation first.")?,
    );
    let telemetry_game = Game::from_id(game["id"].as_str().unwrap_or("assetto-corsa-rally"))?;
    let fingerprint = fingerprint_for_game(executable, telemetry_game)?;
    if check() {
        return Err("Assetto integration verification was cancelled.".into());
    }
    if !cfg!(windows) {
        return Ok(result(&fingerprint,"unsupported-platform",vec!["Native Assetto shared-memory verification requires Windows. Synthetic sources retain their separate capability.".into()]));
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
            let source = crate::acr_windows::WindowsSource::new_for_game(process.pid, basename(&process.path), telemetry_game)?;
            let image = source.process_image().map(str::to_owned);
            Ok(AttachedSource {
                source: Box::new(source),
                image,
            })
        },
        |pid| competing_processes_for_game(pid, telemetry_game),
        || start.get_or_insert_with(Instant::now).elapsed().as_millis() as u64,
        || std::thread::sleep(Duration::from_millis(100)),
    )?;
    if proof["compatible"] == true {
        proof["nativeVerified"] = json!(true);
        progress("Native Assetto telemetry verified", 100);
    }
    Ok(proof)
}

#[cfg(test)]
#[path = "acr_setup_tests.rs"]
mod tests;
