//! MANUAL WINDOWS QUALIFICATION SOURCE. Not referenced by the ordinary crate.
//!
//! The preparer copies this module into a separately stamped qualification tree.
//! Its main has the existing feeder/inspection helper dispatch and NO desktop or
//! GatewayManager fallback. Nothing is activated at startup. Preflight is read
//! only; Apply, Stop, Recover and browser setup are separate explicit requests.
//!
//! Config is machine-local, never a portable profile: schemaVersion:1,
//! reviewedCommit:<full SHA>, privateRoot:<existing ordinary directory inside
//! the qualification copy>, vkb:[{controllerId,interface}], pedals:[...],
//! requiredBindings:[{inventoryResourceId,observedPortId,contractHash,label}].
//! The reviewed current DCS bindings are RIGHT X/Y, LEFT Y and BOTH POV1 hats.
//! This is not a claim that every physical VKB control was combined. Pedals are
//! hidden-only and are never silently added to the virtual controller.
//!
//! NDJSON commands: preflight, apply {document,assignments,gameId:null|"dcs"},
//! stop, pause {paused}, recover, inspect {request}, topology {request},
//! visibility_preferences {enabled:true}, companion {request}, game_setup {request}, shutdown.
//! Browser pair offers are written only to native protected private storage;
//! stdout contains the protected file path, never its credential payload.
//! The actual graph/pins are reviewed AFTER real preflight discovery. No guessed
//! IDs, fabricated source frames, game process, DCS catalogue, or browser sink.
//! A hardware-only staging graph may precede external DCS launch. The full
//! MH-6J graph then uses normal stage_with_handoff/Apply. Existing handoff
//! RESTORES before applying incoming hiding; continuous hiding is not claimed.

use crate::profile_session::{
    ApplyPhase, Candidate, Health, Host, OwnedRun, Prepared, Receipt, RunOwner,
};
use crate::profile_session_host::NativeProfileHost;
use osg_controller_input::visibility::{journal, policy, runtime::Adapter};
use osg_profiles::store::LocalAssignment;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};

const MODE: &str = "--native-profile-qualification";
const MAX_LINE: usize = osg_profiles::MAX_PROFILE_BYTES + 65_536;
const MAX_GUARD: usize = 262_144;
const STAMP: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/qualification-copy.json"
));

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PhysicalPin {
    controller_id: String,
    interface: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BindingPin {
    inventory_resource_id: String,
    observed_port_id: String,
    contract_hash: String,
    label: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Config {
    schema_version: u32,
    reviewed_commit: String,
    private_root: PathBuf,
    #[serde(default)]
    vkb: Vec<PhysicalPin>,
    #[serde(default)]
    pedals: Vec<PhysicalPin>,
    #[serde(default)]
    required_bindings: Vec<BindingPin>,
}
fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}
fn hash(value: &impl Serialize) -> Result<String, String> {
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(err)?)
    ))
}
fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.to_string() == value)
}
fn regular(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    for parent in path.parent().into_iter().flat_map(Path::ancestors) {
        let metadata = std::fs::symlink_metadata(parent).map_err(err)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("Reparse-point private input directory is forbidden".into());
            }
        }
        if metadata.file_type().is_symlink() {
            return Err("Indirect private input directory is forbidden".into());
        }
    }
    let metadata = std::fs::symlink_metadata(path).map_err(err)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Reparse-point input is forbidden".into());
        }
    }
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > limit {
        return Err("Expected a bounded regular private file".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(err)?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(err)?;
    if bytes.len() as u64 > limit {
        return Err("Private file exceeded its bound".into());
    }
    Ok(bytes)
}
fn config(path: &Path) -> Result<Config, String> {
    let value: Config = serde_json::from_slice(&regular(path, 1_048_576)?).map_err(err)?;
    let stamp: Value = serde_json::from_str(STAMP).map_err(err)?;
    if value.schema_version != 1 || stamp["reviewedCommit"] != value.reviewed_commit {
        return Err(
            "Configuration does not match this explicitly reviewed qualification copy".into(),
        );
    }
    if !value.private_root.is_absolute() {
        return Err("Private state requires an absolute path".into());
    }
    // Check the original chain before canonicalization can hide a junction.
    for parent in value.private_root.ancestors() {
        let metadata = std::fs::symlink_metadata(parent).map_err(err)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("Reparse-point private state is forbidden".into());
            }
        }
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("Private state is not an ordinary directory".into());
        }
    }
    let root = value.private_root.canonicalize().map_err(err)?;
    let copy = Path::new(
        stamp["privateCopyRoot"]
            .as_str()
            .ok_or("Missing copy root stamp")?,
    )
    .canonicalize()
    .map_err(err)?;
    if root == copy || !root.starts_with(&copy) {
        return Err("Private state must be a child of the stamped copy".into());
    }
    for parent in root.ancestors().take_while(|p| p.starts_with(&copy)) {
        let metadata = std::fs::symlink_metadata(parent).map_err(err)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("Reparse-point private state is forbidden".into());
            }
        }
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("Private state is not an ordinary directory".into());
        }
    }
    let ids = value
        .vkb
        .iter()
        .chain(&value.pedals)
        .map(|pin| &pin.controller_id)
        .collect::<BTreeSet<_>>();
    if ids.len() != value.vkb.len() + value.pedals.len()
        || ids.iter().any(|id| !canonical_uuid(id))
        || value
            .vkb
            .iter()
            .chain(&value.pedals)
            .any(|pin| pin.interface.is_empty() || pin.interface.len() > 2048)
    {
        return Err("Distinct exact physical identities/interfaces are required".into());
    }
    Ok(Config {
        private_root: root,
        ..value
    })
}
fn line(input: &mut impl BufRead, limit: usize) -> Result<Option<Value>, String> {
    let mut bytes = Vec::new();
    loop {
        let buffer = input.fill_buf().map_err(err)?;
        if buffer.is_empty() {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                Err("Truncated protocol frame".into())
            };
        }
        let end = buffer.iter().position(|b| *b == b'\n');
        let count = end.map_or(buffer.len(), |n| n + 1);
        if bytes.len() + count > limit {
            return Err("Protocol frame exceeded its bound".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
        input.consume(count);
        if end.is_some() {
            return serde_json::from_slice(&bytes).map(Some).map_err(err);
        }
    }
}
fn emit(output: &Mutex<io::Stdout>, value: &Value) -> Result<(), String> {
    let mut output = output.lock().map_err(err)?;
    writeln!(output, "{value}").map_err(err)?;
    output.flush().map_err(err)
}

struct Components {
    controllers: Arc<osg_controller_input::ControllerService>,
    resources: Arc<crate::profile_resources::Runtime>,
    games: Arc<crate::games::GamesRuntime>,
    companions: Arc<crate::companion_host::Runtime>,
    panels: Arc<crate::gateways::ProfilePanelIo>,
    epoch: Arc<AtomicU64>,
}
impl Components {
    fn new(config: &Config) -> Self {
        let root = &config.private_root;
        let controllers = Arc::new(osg_controller_input::ControllerService::new(
            Some(root.join("controllers.json")),
            true,
        ));
        let clock = controllers.clone();
        let provider = Arc::new(osg_mapping_host::companions::CompanionProvider::new(
            Arc::new(move || clock.stream_page("", 0).captured_us),
        ));
        let companions = Arc::new(crate::companion_host::Runtime::new(
            Some(root.join("companions")),
            provider.clone(),
        ));
        // Actual installation/process/provider observation; never a fixture Platform.
        let games = Arc::new(crate::games::GamesRuntime::new(
            Arc::new(|_| panic!("OSG/retired game I/O is excluded from this qualification")),
            crate::games::Paths::in_directory(root),
            Arc::new(crate::game_discovery::NativePlatform),
        ));
        let usb = Arc::new(crate::module_usb_fleet::Fleet::new(
            Arc::new(|| panic!("OSG serial enumeration is excluded")),
            false,
        ));
        let collect = crate::profile_resource_sources::collector(
            controllers.clone(),
            provider,
            companions.clone(),
            games.clone(),
            Arc::new(Vec::new),
            Arc::new(|_| panic!("OSG transport is excluded")),
            usb,
            false,
        );
        let resources = Arc::new(crate::profile_resources::Runtime::new(
            Some(root.join("profile-resources-v1")),
            collect,
            true,
        ));
        Self {
            controllers,
            resources,
            games,
            companions,
            panels: Arc::new(crate::gateways::ProfilePanelIo::empty()),
            epoch: Arc::new(AtomicU64::new(1)),
        }
    }
    fn validate_physical_pins(&self, config: &Config) -> Result<(), String> {
        let inventory = self.controllers.resource_inventory();
        for pin in config.vkb.iter().chain(&config.pedals) {
            let source = inventory
                .sources
                .iter()
                .find(|s| s.saved.id == pin.controller_id)
                .ok_or(
                    "A selected physical controller is absent from the actual native registry",
                )?;
            if !source.online
                || !source.baseline_ready
                || source.saved.identity.backend != "windows-raw-input"
                || source.saved.identity.path != pin.interface
            {
                return Err("A selected physical controller lacks its exact current native interface/baseline".into());
            }
        }
        Ok(())
    }
    fn preflight(&self) -> Result<Value, String> {
        self.resources.refresh_native()?;
        let mut controller_pages = Vec::new();
        let mut offset = 0;
        loop {
            let page = self
                .controllers
                .request(&json!({"command":"controller_list","offset":offset}))?;
            let count = page["sources"]
                .as_array()
                .ok_or("Missing native controller inventory")?
                .len();
            let total = page["total"].as_u64().ok_or("Missing controller total")?;
            controller_pages.push(page);
            offset += count as u64;
            if offset >= total {
                break;
            }
            if count == 0 || offset > 256 {
                return Err("Native controller pagination did not advance".into());
            }
        }
        let mut details = Vec::new();
        // Native source/control IDs and complete paged descriptors, never names as authority.
        for source in controller_pages
            .iter()
            .flat_map(|p| p["sources"].as_array().into_iter().flatten())
        {
            let id = source["id"].as_str().ok_or("Missing controller identity")?;
            let mut offset = 0;
            loop {
                let page = self.controllers.request(
                    &json!({"command":"controller_inspect","sourceId":id,"offset":offset}),
                )?;
                let count = page["controls"]
                    .as_array()
                    .ok_or("Missing native controls")?
                    .len();
                let total = page["total"]
                    .as_u64()
                    .ok_or("Missing native control total")?;
                details.push(page);
                offset += count as u64;
                if offset >= total {
                    break;
                }
                if count == 0 || offset > 256 {
                    return Err("Native control pagination did not advance".into());
                }
            }
        }
        let mut resource_pages = Vec::new();
        let mut offset = 0;
        loop {
            let page = self.resources.request(
                &json!({"command":"profile_resources_list","offset":offset,"limit":128}),
            )?;
            let count = page["resources"]
                .as_array()
                .ok_or("Missing native resource page")?
                .len();
            let total = page["total"]
                .as_u64()
                .ok_or("Missing native resource total")?;
            resource_pages.push(page);
            offset += count as u64;
            if offset >= total {
                break;
            }
            if count == 0 || offset > 40_000 {
                return Err("Native resource pagination did not advance".into());
            }
        }
        let mut resource_details = Vec::new();
        for resource in resource_pages
            .iter()
            .flat_map(|p| p["resources"].as_array().into_iter().flatten())
        {
            let id = resource["id"]
                .as_str()
                .ok_or("Missing native resource ID")?;
            let mut offset = 0;
            loop {
                let page=self.resources.request(&json!({"command":"profile_resources_inspect","resourceId":id,"offset":offset,"limit":128}))?;
                let count = page["ports"]
                    .as_array()
                    .ok_or("Missing native resource ports")?
                    .len();
                let total = page["total"].as_u64().ok_or("Missing native port total")?;
                resource_details.push(page);
                offset += count as u64;
                if offset >= total {
                    break;
                }
                if count == 0 || offset > 40_000 {
                    return Err("Native resource port pagination did not advance".into());
                }
            }
        }
        let slots = osg_virtual_controller::inspection::current();
        let visibility = self
            .controllers
            .request(&json!({"command":"controller_visibility_status"}))?;
        let hidhide = read_native_hidhide();
        let companion = self
            .companions
            .profile_metadata()
            .and_then(|hub| hub.command(&json!({"command":"companion_status"})));
        Ok(
            json!({"mode":"read-only-preflight","qualified":false,"osgHardware":"explicitly deferred",
            "controllerPages":controller_pages,"controllerDetails":details,"resourcePages":resource_pages,"resourceDetails":resource_details,
            "games":self.games.status(),"companions":companion,"slots":slots,"visibility":visibility,"nativeHidHide":hidhide,
            "scope":"Review actual current DCS-bound VKB flight/view ports, both POV hats, and hidden-only T-Rudder; no claim that all physical controls are mapped",
            "visibilityInterpretation":"Existing hidden/allowed entries are foreign baseline, not evidence that this profile caused hiding. A causal hiding test may need separately reviewed temporary baseline setup; no global clearing is performed.",
            "notPerformed":["claim","hide","assign","launch","profile Apply","OSG discovery"]}),
        )
    }
    fn shutdown(&self) {
        self.panels.close();
        self.resources.shutdown();
        self.games.shutdown();
        self.companions.shutdown();
        self.controllers.shutdown();
    }
}

fn read_native_hidhide() -> Result<Value, String> {
    #[cfg(windows)]
    {
        use journal::Backend;
        let mut driver = osg_controller_input::visibility::windows::installed()?;
        return serde_json::to_value(driver.read()?).map_err(err);
    }
    #[cfg(not(windows))]
    {
        Err("This manual qualification is Windows-only".into())
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProcessIdentity {
    pid: u32,
    creation: u64,
    executable: String,
}

#[cfg(windows)]
mod process_proof {
    use super::*;
    use std::ffi::c_void;
    type Handle = *mut c_void;
    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
        fn GetProcessTimes(
            handle: Handle,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        fn QueryFullProcessImageNameW(
            handle: Handle,
            flags: u32,
            buffer: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    }
    pub struct Owned(Handle);
    unsafe impl Send for Owned {}
    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    impl Owned {
        pub fn open(pid: u32) -> Result<Self, String> {
            let handle = unsafe { OpenProcess(0x0010_0000 | 0x1000, 0, pid) };
            if handle.is_null() {
                return Err(err(io::Error::last_os_error()));
            }
            Ok(Self(handle))
        }
        pub fn identity(&self, pid: u32) -> Result<ProcessIdentity, String> {
            let mut times = [
                FileTime { low: 0, high: 0 },
                FileTime { low: 0, high: 0 },
                FileTime { low: 0, high: 0 },
                FileTime { low: 0, high: 0 },
            ];
            let pointer = times.as_mut_ptr();
            if unsafe {
                GetProcessTimes(
                    self.0,
                    pointer,
                    pointer.add(1),
                    pointer.add(2),
                    pointer.add(3),
                )
            } == 0
            {
                return Err(err(io::Error::last_os_error()));
            }
            let mut text = vec![0u16; 32768];
            let mut length = text.len() as u32;
            if unsafe { QueryFullProcessImageNameW(self.0, 0, text.as_mut_ptr(), &mut length) } == 0
            {
                return Err(err(io::Error::last_os_error()));
            }
            let executable = String::from_utf16(&text[..length as usize]).map_err(err)?;
            let creation = u64::from(times[0].low) | (u64::from(times[0].high) << 32);
            if creation == 0 {
                return Err("Missing native process creation identity".into());
            }
            Ok(ProcessIdentity {
                pid,
                creation,
                executable,
            })
        }
        pub fn wait_for_death(&self) -> Result<(), String> {
            loop {
                match unsafe { WaitForSingleObject(self.0, 50) } {
                    0 => return Ok(()),
                    258 => {}
                    _ => return Err(err(io::Error::last_os_error())),
                }
            }
        }
    }
    pub fn current() -> Result<ProcessIdentity, String> {
        let pid = std::process::id();
        Owned::open(pid)?.identity(pid)
    }
}

struct GuardianIo {
    child: Child,
    input: ChildStdin,
    replies: mpsc::Receiver<Value>,
    armed: BTreeMap<String, Value>,
    plans: BTreeMap<String, Value>,
}
struct Guardian {
    io: Mutex<GuardianIo>,
    healthy: AtomicBool,
}
impl Guardian {
    #[cfg(windows)]
    fn start(config_path: &Path, config: &Config) -> Result<Arc<Self>, String> {
        use std::os::windows::process::CommandExt;
        let parent = process_proof::current()?;
        let nonce = uuid::Uuid::new_v4().to_string();
        let mut child = Command::new(std::env::current_exe().map_err(err)?)
            .args([MODE, "guardian"])
            .arg(config_path)
            .arg(serde_json::to_string(&parent).map_err(err)?)
            .arg(&nonce)
            .creation_flags(0x0800_0000 | 0x0000_0200 | 0x0100_0000) // hidden, new process group, independent of an enclosing job
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(err)?;
        let child_proof = process_proof::Owned::open(child.id())?.identity(child.id())?;
        let input = child.stdin.take().ok_or("Missing guardian pipe")?;
        let output = child.stdout.take().ok_or("Missing guardian reply pipe")?;
        let (send, replies) = mpsc::sync_channel(2);
        thread::spawn(move || {
            let mut input = BufReader::new(output);
            while let Ok(Some(value)) = line(&mut input, MAX_GUARD) {
                if send.send(value).is_err() {
                    break;
                }
            }
        });
        let ready = replies.recv_timeout(Duration::from_secs(3)).map_err(err)?;
        if ready["ready"] != true
            || ready["nonce"] != nonce
            || ready["parent"] != json!(parent)
            || ready["guardian"] != json!(child_proof)
            || ready["configHash"] != hash(config)?
        {
            let _ = child.kill();
            return Err("Guardian native identity/arm handshake did not match".into());
        }
        Ok(Arc::new(Self {
            io: Mutex::new(GuardianIo {
                child,
                input,
                replies,
                armed: BTreeMap::new(),
                plans: BTreeMap::new(),
            }),
            healthy: AtomicBool::new(true),
        }))
    }
    #[cfg(not(windows))]
    fn start(_: &Path, _: &Config) -> Result<Arc<Self>, String> {
        Err("This qualification is Windows-only".into())
    }
    fn live(&self) -> Result<(), String> {
        if !self.healthy.load(Ordering::Acquire)
            || self
                .io
                .lock()
                .map_err(err)?
                .child
                .try_wait()
                .map_err(err)?
                .is_some()
        {
            return Err(
                "The independent guardian is unavailable; productive qualification must stop"
                    .into(),
            );
        }
        Ok(())
    }
    fn approved_plans(&self) -> Result<Value, String> {
        Ok(json!(self.io.lock().map_err(err)?.plans))
    }
    fn arm(
        &self,
        owner: &RunOwner,
        receipts: &[Receipt],
        controllers: &osg_controller_input::ControllerService,
        previous: Option<&OwnedRun>,
    ) -> Result<(), String> {
        self.live()?;
        let selected = receipts
            .iter()
            .filter(|receipt| receipt.kind == "controller-visibility")
            .collect::<Vec<_>>();
        if selected.is_empty() {
            return Ok(());
        }
        if selected.len() != 1 {
            return Err("Expected exactly one native visibility receipt".into());
        }
        let key = selected[0].state["visibilityOwner"]
            .as_str()
            .ok_or("Missing exact visibility owner")?
            .to_owned();
        if let Some(acknowledged) = self.io.lock().map_err(err)?.armed.get(&key) {
            return if acknowledged == &json!(selected[0]) {
                Ok(())
            } else {
                Err("A guardian owner was rebound to different receipts".into())
            };
        }
        // Re-read the actual native preparation and bind it to the receipt the
        // real Host sealed. The guardian must authorize the full baseline before
        // the coordinator can persist or enter the first mutating phase.
        let intent = controllers.game_visibility(
            "capture",
            &json!({"mode":"hidhide-session","sourceIds":selected[0].state["sourceIds"]}),
        )?;
        let old = previous.and_then(|run| {
            run.receipts
                .iter()
                .find(|r| r.kind == "controller-visibility")
        });
        let preparation = match old {
            Some(receipt) => controllers.game_visibility(
                "handoff_preflight",
                &json!({"owner":receipt.state["visibilityOwner"],"intent":intent}),
            )?,
            None => controllers.game_visibility("preflight", &intent)?,
        };
        if json!(hash(&preparation)?) != selected[0].state["preparationHash"] {
            return Err(
                "Native preparation changed after Host capture; no hiding authorized".into(),
            );
        }
        let request =
            json!({"command":"arm","owner":owner,"receipts":selected,"preparation":preparation});
        let bytes = serde_json::to_vec(&request).map_err(err)?;
        if bytes.len() + 1 > MAX_GUARD {
            return Err("Guardian authority exceeded its bound".into());
        }
        let mut io = self.io.lock().map_err(err)?;
        if io
            .input
            .write_all(&bytes)
            .and_then(|_| io.input.write_all(b"\n"))
            .and_then(|_| io.input.flush())
            .is_err()
        {
            self.healthy.store(false, Ordering::Release);
            return Err("Guardian authority pipe failed".into());
        }
        let reply = match io.replies.recv_timeout(Duration::from_secs(3)) {
            Ok(reply) => reply,
            Err(error) => {
                self.healthy.store(false, Ordering::Release);
                return Err(err(error));
            }
        };
        if reply["armed"] != true || reply["owner"] != json!(owner) {
            self.healthy.store(false, Ordering::Release);
            return Err(format!("Guardian refused exact ownership: {reply}"));
        }
        io.armed.insert(key.clone(), json!(selected[0]));
        io.plans
            .insert(key, request["preparation"]["policy"].clone());
        Ok(())
    }
}

// All execution/cleanup remains the actual NativeProfileHost. This wrapper adds
// only the independent guardian acknowledgement before returning captured
// receipts (and thus before the coordinator can journal/enter Visibility).
struct GuardedHost {
    inner: NativeProfileHost,
    guardian: Arc<Guardian>,
    controllers: Arc<osg_controller_input::ControllerService>,
}
impl Host for GuardedHost {
    fn prepare(&mut self, c: &Candidate, o: &RunOwner) -> Result<Prepared, String> {
        self.inner.prepare(c, o)
    }
    fn validate_prepared(&mut self, p: &Prepared, o: &RunOwner) -> Result<(), String> {
        self.inner.validate_prepared(p, o)
    }
    fn capture(
        &mut self,
        p: &Prepared,
        o: &RunOwner,
        old: Option<&OwnedRun>,
    ) -> Result<Vec<Receipt>, String> {
        let receipts = self.inner.capture(p, o, old)?;
        self.guardian.arm(o, &receipts, &self.controllers, old)?;
        Ok(receipts)
    }
    fn set_authority(&mut self, o: Option<&RunOwner>) -> Result<(), String> {
        self.inner.set_authority(o)
    }
    fn neutralize(&mut self, r: &OwnedRun) -> Result<(), String> {
        self.inner.neutralize(r)
    }
    fn prepare_recovery(&mut self, r: &OwnedRun) -> Result<(), String> {
        self.inner.prepare_recovery(r)
    }
    fn recapture_phase(
        &mut self,
        p: ApplyPhase,
        plan: &mut Prepared,
        r: &OwnedRun,
        old: Option<&OwnedRun>,
    ) -> Result<Option<Vec<Receipt>>, String> {
        let replacement = self.inner.recapture_phase(p, plan, r, old)?;
        if let Some(ref receipts) = replacement {
            self.guardian
                .arm(&r.owner, receipts, &self.controllers, old)?;
        }
        Ok(replacement)
    }
    fn enter(&mut self, p: ApplyPhase, plan: &Prepared, r: &OwnedRun) -> Result<(), String> {
        self.guardian.live()?;
        self.inner.enter(p, plan, r)
    }
    fn restore(&mut self, r: &OwnedRun, recovery: bool) -> Result<(), String> {
        self.inner.restore(r, recovery)
    }
    fn health(&mut self, r: &OwnedRun) -> Result<Health, String> {
        self.guardian.live()?;
        self.inner.health(r)
    }
    fn supports_launch_hold(&self) -> bool {
        self.inner.supports_launch_hold()
    }
    fn hold_health(&mut self, r: &OwnedRun) -> Result<Health, String> {
        self.guardian.live()?;
        self.inner.hold_health(r)
    }
    fn bind_launch_epoch(&mut self, r: &OwnedRun, e: u64) -> Result<(), String> {
        self.inner.bind_launch_epoch(r, e)
    }
    fn install_launch_context(&mut self, r: &OwnedRun, c: &Candidate) -> Result<Prepared, String> {
        self.inner.install_launch_context(r, c)
    }
    fn inspect(
        &self,
        r: &OwnedRun,
        q: &crate::profile_session::inspection::Request,
    ) -> Result<crate::profile_session::inspection::Page, String> {
        self.inner.inspect(r, q)
    }
    fn inspect_topology(
        &self,
        r: &OwnedRun,
        q: &crate::profile_session::topology::Request,
    ) -> Result<crate::profile_session::topology::Page, String> {
        self.inner.inspect_topology(r, q)
    }
    fn discard_preparation(&mut self, o: &RunOwner) {
        self.inner.discard_preparation(o)
    }
}

#[derive(Clone)]
struct Armed {
    owner: Value,
    visibility_owner: String,
    preparation_hash: String,
    source_ids: Vec<String>,
}
#[cfg(windows)]
fn guardian(config: &Config, parent: ProcessIdentity, nonce: &str) -> Result<(), String> {
    use journal::Backend;
    let process = process_proof::Owned::open(parent.pid)?;
    if process.identity(parent.pid)? != parent {
        return Err("Parent PID was reused or executable/creation changed".into());
    }
    let own = process_proof::current()?;
    if !own.executable.eq_ignore_ascii_case(&parent.executable) {
        return Err("Guardian executable differs from its exact parent".into());
    }
    let mut driver = osg_controller_input::visibility::windows::installed()?;
    let before = driver.read()?;
    let capture = driver.executable(&parent.executable)?;
    let instances = config
        .pedals
        .iter()
        .map(|pin| driver.instance(&pin.interface))
        .collect::<Result<BTreeSet<_>, _>>()?;
    drop(driver); // Do not hold the HidHide control endpoint during the live host.
    let expected = config
        .pedals
        .iter()
        .map(|pin| pin.controller_id.clone())
        .collect::<BTreeSet<_>>();
    let output = Mutex::new(io::stdout());
    emit(
        &output,
        &json!({"ready":true,"nonce":nonce,"parent":parent,"guardian":own,"configHash":hash(config)?,"privateRoot":config.private_root,"beforeHash":hash(&before)?}),
    )?;
    let mut armed: Vec<Armed> = vec![];
    let mut input = io::stdin().lock();
    let protocol = (|| -> Result<(), String> {
        while let Some(request) = line(&mut input, MAX_GUARD)? {
            if request["command"] != "arm" {
                return Err("Unknown guardian command".into());
            }
            let owner = request["owner"].clone();
            let typed: RunOwner = serde_json::from_value(owner.clone()).map_err(err)?;
            if !canonical_uuid(&typed.session)
                || !canonical_uuid(&typed.profile_id)
                || typed.generation == 0
                || typed.profile_revision == 0
                || typed.profile_revision > osg_profiles::MAX_REVISION
            {
                return Err("Malformed exact profile owner".into());
            }
            let receipts = request["receipts"]
                .as_array()
                .ok_or("Missing exact visibility receipts")?;
            if receipts.len() != 1 {
                return Err("Guardian requires one exact native visibility receipt".into());
            }
            let receipt = &receipts[0];
            let state = &receipt["state"];
            let visibility_owner = state["visibilityOwner"]
                .as_str()
                .filter(|id| canonical_uuid(id))
                .ok_or("Missing native visibility owner")?;
            let source_ids: Vec<String> =
                serde_json::from_value(state["sourceIds"].clone()).map_err(err)?;
            let preparation_hash = state["preparationHash"]
                .as_str()
                .filter(|h| h.starts_with("sha256:") && h.len() == 71)
                .ok_or("Missing native preparation hash")?;
            if receipt["kind"] != "controller-visibility"
                || state["owner"] != owner
                || source_ids.iter().cloned().collect::<BTreeSet<_>>() != expected
                || source_ids.len() != expected.len()
                || expected.is_empty()
            {
                return Err("Guardian receipt differs from the selected hidden-only pedals".into());
            }
            let preparation = &request["preparation"];
            if hash(preparation)? != preparation_hash {
                return Err("Guardian preparation does not match the exact sealed receipt".into());
            }
            let plan: policy::Plan =
                serde_json::from_value(preparation["policy"].clone()).map_err(err)?;
            plan.validate()?;
            if plan.before != before
                || plan.capture != capture
                || plan
                    .selections
                    .iter()
                    .map(|s| s.instance.clone())
                    .collect::<BTreeSet<_>>()
                    != instances
                || plan.selections.len() != instances.len()
                || plan
                    .selections
                    .iter()
                    .any(|s| s.policy != policy::Policy::HideAll)
            {
                return Err("Native preflight differs from the independent original baseline or exact selected pedals; hiding refused before mutation".into());
            }
            let proof = Armed {
                owner: owner.clone(),
                visibility_owner: visibility_owner.into(),
                preparation_hash: preparation_hash.into(),
                source_ids,
            };
            if let Some(previous) = armed
                .iter()
                .find(|p| p.visibility_owner == visibility_owner)
            {
                if previous.owner != proof.owner
                    || previous.preparation_hash != proof.preparation_hash
                    || previous.source_ids != proof.source_ids
                {
                    return Err("Guardian owner was rebound to another receipt".into());
                }
            } else {
                if armed.len() >= 32 {
                    return Err(
                        "Guardian ownership history is full; stop and review before more runs"
                            .into(),
                    );
                }
                armed.push(proof);
            }
            emit(&output, &json!({"armed":true,"owner":owner}))?;
        }
        Ok(())
    })();
    // Even a later malformed arm/closed pipe cannot discard a previous guard.
    // The live host notices the bounded failed acknowledgement and stops, while
    // this independent process retains all prior cleanup pins until real death.
    // EOF alone is not death evidence. Hold the native process handle through
    // actual termination; no PID-only liveness or reusable-PID assumption.
    process.wait_for_death()?;
    let result = (|| -> Result<Value, String> {
        let mut driver = osg_controller_input::visibility::windows::installed()?;
        let mut restored = vec![];
        for proof in armed.iter().rev() {
            let path = config
                .private_root
                .join(format!("game-visibility-{}.json", proof.visibility_owner));
            let Some(record) = journal::inspect(&path)? else {
                continue;
            };
            record.validate()?;
            if record.plan.before != before
                || record.plan.capture != capture
                || record
                    .plan
                    .selections
                    .iter()
                    .map(|s| s.instance.clone())
                    .collect::<BTreeSet<_>>()
                    != instances
                || record.plan.selections.len() != instances.len()
                || record
                    .plan
                    .selections
                    .iter()
                    .any(|s| s.policy != policy::Policy::HideAll)
            {
                return Err("Journal policy differs from the independently captured native baseline/selected pedals; preserved".into());
            }
            if record.clean {
                continue;
            }
            let id = record.id.clone();
            let mut store = journal::Store::open(path)?;
            let clean = journal::restore(&mut driver, &mut store, &id)?;
            if !clean.clean || driver.read()? != before {
                return Err(
                    "Native compareRestore did not verify the complete original state".into(),
                );
            }
            restored.push(json!({"visibilityOwner":proof.visibility_owner,"recordId":id,"profileOwner":proof.owner}));
        }
        if !armed.is_empty() && driver.read()? != before {
            return Err("The final global policy differs from the original native baseline; preserved for explicit inspection".into());
        }
        Ok(
            json!({"state":"restored","parent":parent,"restored":restored,"beforeHash":hash(&before)?,"protocolError":protocol.err()}),
        )
    })();
    // Private report only. No deletion, blanket clearing, or forced activation.
    std::fs::write(
        config.private_root.join("guardian-result.json"),
        serde_json::to_vec_pretty(&json!({"result":result})).map_err(err)?,
    )
    .map_err(err)?;
    result.map(|_| ())
}

struct Application {
    config: Config,
    components: Arc<Components>,
    guardian: Arc<Guardian>,
    coordinator: Mutex<crate::profile_session::Coordinator<GuardedHost>>,
    closing: AtomicBool,
}
impl Application {
    fn independent<T>(
        &self,
        operation: Option<&str>,
        dispatch: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        // Preserve native session admission without constructing GatewayManager.
        // Guard and actual independent mutation share the same lock scope.
        let session = operation
            .map(|_| self.coordinator.lock().map_err(err))
            .transpose()?;
        if let (Some(session), Some(operation)) = (&session, operation) {
            session.guard_request(operation)?;
        }
        dispatch()
    }
    fn apply(&self, request: &Value) -> Result<Value, String> {
        if self.config.vkb.len() < 2
            || self.config.pedals.is_empty()
            || self.config.required_bindings.len() < 5
        {
            return Err("Review actual VKB/pedal IDs and all current DCS-bound flight/view ports before Apply".into());
        }
        self.components.validate_physical_pins(&self.config)?;
        let document = osg_profiles::ProfileDocument::parse_draft(
            &serde_json::to_vec(&request["document"]).map_err(err)?,
        )?;
        let assignments: Vec<LocalAssignment> =
            serde_json::from_value(request["assignments"].clone()).map_err(err)?;
        osg_profiles::store::validate_assignments(&document, &assignments)?;
        if !document.overrides.is_empty() {
            return Err(
                "Actual OSG publication overrides are excluded from this qualification".into(),
            );
        }
        for pin in &self.config.required_bindings {
            if !assignments.iter().any(|a| {
                a.source_id == pin.inventory_resource_id
                    && a.ports.iter().any(|p| {
                        p.observed_port_id == pin.observed_port_id
                            && p.expected_contract_hash == pin.contract_hash
                    })
            }) {
                return Err(format!(
                    "Reviewed current DCS binding is missing: {}",
                    pin.label
                ));
            }
        }
        let snapshot = self
            .components
            .resources
            .snapshot_for_compile(assignments.clone())?;
        let mut selected_physical = BTreeMap::new();
        for assignment in &assignments {
            let resource = snapshot
                .resources
                .iter()
                .find(|r| r.summary.id == assignment.source_id)
                .ok_or("Assigned native resource disappeared")?;
            if resource.summary.resource_type == osg_profiles::resources::ResourceType::OsgModule
                || resource.summary.connections.iter().any(|c| {
                    matches!(
                        c.route.transport,
                        osg_profiles::resources::Transport::Can
                            | osg_profiles::resources::Transport::Usb
                    )
                })
            {
                return Err("OSG resources/transports are explicitly deferred".into());
            }
            if resource.summary.resource_type
                == osg_profiles::resources::ResourceType::HostController
            {
                let physical = resource
                    .summary
                    .connections
                    .iter()
                    .filter_map(|c| c.route.id.strip_prefix("hid:"))
                    .collect::<Vec<_>>();
                if physical.len() != 1 {
                    return Err("Expected one exact native physical HID route".into());
                }
                if self
                    .config
                    .pedals
                    .iter()
                    .any(|p| p.controller_id == physical[0])
                    && !assignment.ports.is_empty()
                {
                    return Err("Pedals are hidden-only; their physical X route must not be silently added to VKB A+B".into());
                }
                if !self
                    .config
                    .vkb
                    .iter()
                    .chain(&self.config.pedals)
                    .any(|p| p.controller_id == physical[0])
                {
                    return Err("An unreviewed physical controller was assigned".into());
                }
                if selected_physical
                    .insert(physical[0].to_owned(), assignment.resource_id.clone())
                    .is_some()
                {
                    return Err("A selected physical controller was assigned more than once".into());
                }
            }
        }
        if self
            .config
            .vkb
            .iter()
            .chain(&self.config.pedals)
            .any(|pin| !selected_physical.contains_key(&pin.controller_id))
        {
            return Err(
                "The complete reviewed VKB plus hidden-only pedals selection must be present"
                    .into(),
            );
        }
        let expected_hidden = self
            .config
            .pedals
            .iter()
            .map(|pin| selected_physical[&pin.controller_id].clone())
            .collect::<BTreeSet<_>>();
        let actual_hidden = document
            .visibility
            .iter()
            .filter(|v| v.mode == osg_profiles::VisibilityMode::HiddenWhileActive)
            .map(|v| v.resource_id.clone())
            .collect::<BTreeSet<_>>();
        if actual_hidden != expected_hidden {
            return Err("Visibility must select exactly the reviewed hidden-only pedals; no implicit or broader hiding".into());
        }
        if document.virtual_outputs.is_empty() || document.companion_assignments.is_empty() {
            return Err("The selected combined virtual controller and actual browser companion must both remain in the composed graph".into());
        }
        let game_id = match request.get("gameId") {
            None | Some(Value::Null) => None,
            Some(Value::String(id)) if id == "dcs" => {
                if !document.resources.iter().any(|resource| {
                    resource.kind == osg_profiles::ResourceKind::Game && !resource.ports.is_empty()
                }) {
                    return Err(
                        "The final actual DCS run must contain its reviewed live CommonData branch"
                            .into(),
                    );
                }
                let actual = self
                    .components
                    .games
                    .prepare_profile_game_context("dcs")?
                    .ok_or("Real DCS process/setup context is not yet available")?;
                // This checks configured catalogue selection only. Actual ownship,
                // process and export freshness remain the real GameSources proof.
                if actual.aircraft_module.as_deref() != Some("MH-6J") {
                    return Err("The native provider setup has not selected the MH-6J catalogue; no AH-6 command alias is accepted".into());
                }
                Some(id.clone())
            }
            _ => return Err("Only the selected actual DCS context is in scope".into()),
        };
        // No epoch advance for preflight/Apply: a failed incoming preparation
        // must preserve the old run. Explicit Stop/Pause cancel at the input boundary.
        let mut candidate = Candidate {
            document,
            assignments,
            read_only: false,
            game_id,
            admission_epoch: Some(self.components.epoch.load(Ordering::Acquire)),
            stage: None,
        };
        let outgoing = self
            .coordinator
            .lock()
            .map_err(err)?
            .visibility_handoff_source()?;
        candidate.stage = Some(NativeProfileHost::stage_with_handoff(
            &self.components.resources,
            &self.components.games,
            &self.components.companions,
            self.components.controllers.clone(),
            true,
            &candidate,
            outgoing,
        )?);
        let mut coordinator = self.coordinator.lock().map_err(err)?;
        coordinator.apply(candidate)?;
        Ok(
            json!({"profileSession":coordinator.status(),"nativeVisibilityPlans":self.guardian.approved_plans()?,"qualified":false,"scope":"current DCS-bound VKB flight/view + hidden-only pedals + actual browser + actual MH-6J CommonData when selected","handoff":"restore-before-apply; verify current DCS pedal access after takeover"}),
        )
    }
    fn request(&self, request: &Value) -> Result<Value, String> {
        match request["command"]
            .as_str()
            .ok_or("Missing explicit command")?
        {
            "preflight" => self.components.preflight(),
            "apply" => self.apply(request),
            "stop" => {
                let mut c = self.coordinator.lock().map_err(err)?;
                c.stop()?;
                Ok(json!(c.status()))
            }
            "pause" => {
                let mut c = self.coordinator.lock().map_err(err)?;
                c.pause(
                    request["paused"]
                        .as_bool()
                        .ok_or("Explicit paused boolean required")?,
                )?;
                Ok(json!(c.status()))
            }
            "recover" => {
                let mut c = self.coordinator.lock().map_err(err)?;
                c.recover()?;
                Ok(json!(c.status()))
            }
            "status" => Ok(json!(self.coordinator.lock().map_err(err)?.status())),
            "inspect" => {
                let q = crate::profile_session::inspection::Request::parse(&request["request"])?;
                serde_json::to_value(self.coordinator.lock().map_err(err)?.inspect(&q)?)
                    .map_err(err)
            }
            "topology" => {
                let q = crate::profile_session::topology::Request::parse(&request["request"])?;
                serde_json::to_value(self.coordinator.lock().map_err(err)?.inspect_topology(&q)?)
                    .map_err(err)
            }
            "visibility_preferences" => {
                if request["enabled"] != true {
                    return Err(
                        "Explicit hiding preference opt-in required; this command does not Apply"
                            .into(),
                    );
                }
                self.independent(Some("claim"),||{self.components
                    .controllers
                    .request(&json!({"command":"controller_visibility_enable","enabled":true}))?;
                for pin in &self.config.pedals {
                    self.components.controllers.request(&json!({"command":"controller_visibility_save","sourceId":pin.controller_id,"intent":{"mode":"hide-all"}}))?;
                }
                self.components
                    .controllers
                    .request(&json!({"command":"controller_visibility_status"}))
                })
            }
            "companion" => {
                let value = &request["request"];
                let command = value["command"]
                    .as_str()
                    .ok_or("Missing explicit browser setup command")?;
                if !matches!(
                    command,
                    "companion_enable"
                        | "companion_disable"
                        | "companion_status"
                        | "companion_pair_offer"
                        | "companion_pair_cancel"
                        | "companion_approve"
                        | "companion_update"
                        | "companion_browser_certificate"
                        | "companion_publish_begin"
                        | "companion_publish_chunk"
                        | "companion_publish_commit"
                        | "companion_publish_abort"
                        | "companion_assign"
                        | "companion_unassign"
                ) {
                    return Err("Only explicit ordinary browser WSS setup is permitted; no USB detection/transport operation".into());
                }
                if command == "companion_pair_offer" && value["companion"]["kind"] != "browser" {
                    return Err(
                        "Only actual browser pairing is selected for this qualification".into(),
                    );
                }
                let operation = (!matches!(
                    command,
                    "companion_status" | "companion_browser_certificate"
                ))
                .then_some(command);
                let mut reply =
                    self.independent(operation, || self.components.companions.request(value))?;
                if command == "companion_pair_offer" {
                    use osg_companion_service::persistence::StateStorage;
                    let path = self.config.private_root.join("browser-pairing.dpapi");
                    osg_companion_service::persistence::SealedFileStore::new(&path)
                        .write(&serde_json::to_vec(&reply).map_err(err)?)?;
                    reply["pairing"] = json!({"protectedPrivateFile":path,"protection":"native current-user protected store","credentialPayloadPrinted":false});
                }
                Ok(reply)
            }
            "game_setup" => {
                let value = &request["request"];
                let command = value["command"]
                    .as_str()
                    .ok_or("Missing explicit game metadata/setup operation")?;
                if !matches!(
                    command,
                    "game_get_state"
                        | "game_scan"
                        | "game_installation"
                        | "game_integration_check"
                        | "game_integration_settings_save"
                ) || value.get("gameId").is_some_and(|id| id != "dcs")
                {
                    return Err("Only explicit DCS installation/check/provider settings are allowed; no installation, launch or activation side effect".into());
                }
                let operation =
                    (!matches!(command, "game_get_state" | "game_integration_settings_save"))
                        .then_some(command);
                self.independent(operation, || {
                    self.components.games.request(value)?;
                    Ok(self.components.games.status())
                })
            }
            "shutdown" => {
                self.coordinator.lock().map_err(err)?.stop()?;
                self.closing.store(true, Ordering::Release);
                Ok(json!({"stopped":true}))
            }
            _ => Err(
                "Unknown qualification command; no generic gateway/service forwarding exists"
                    .into(),
            ),
        }
    }
}

fn serve(configuration: Config, config_path: &Path) -> Result<(), String> {
    let components = Arc::new(Components::new(&configuration));
    let guardian = Guardian::start(config_path, &configuration)?;
    let inner = NativeProfileHost::new(
        components.resources.clone(),
        components.controllers.clone(),
        components.games.clone(),
        components.companions.clone(),
        components.panels.clone(),
        true,
        components.epoch.clone(),
    );
    let application = Arc::new(Application {
        config: configuration.clone(),
        components: components.clone(),
        guardian: guardian.clone(),
        coordinator: Mutex::new(crate::profile_session::Coordinator::new(
            GuardedHost {
                inner,
                guardian,
                controllers: components.controllers.clone(),
            },
            Some(configuration.private_root.join("profile-session-v1.json")),
        )),
        closing: AtomicBool::new(false),
    });
    let clock = application.clone();
    let worker = thread::spawn(move || {
        while !clock.closing.load(Ordering::Acquire) {
            let _ = clock.coordinator.lock().map(|mut c| c.poll());
            thread::sleep(Duration::from_millis(5));
        }
    });
    let output = Arc::new(Mutex::new(io::stdout()));
    let (send, input) = mpsc::sync_channel(8);
    // A blocking console read must not prevent an explicit Shutdown from
    // finishing cleanup. This reader holds no authority and dies with the process.
    thread::spawn(move || {
        let mut input = io::stdin().lock();
        loop {
            let next = line(&mut input, MAX_LINE);
            let terminal = !matches!(&next, Ok(Some(_)));
            if send.send(next).is_err() || terminal {
                break;
            }
        }
    });
    let mut jobs: Vec<thread::JoinHandle<()>> = Vec::new();
    let serving = (|| -> Result<(), String> {
        emit(
            &output,
            &json!({"ready":true,"mode":"explicit-private-qualification","activated":false,"qualified":false,"reviewedCommit":configuration.reviewed_commit}),
        )?;
        while !application.closing.load(Ordering::Acquire) {
            let request = match input.recv_timeout(Duration::from_millis(20)) {
                Ok(Ok(Some(request))) => request,
                Ok(Ok(None)) => break,
                Ok(Err(error)) => return Err(error),
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            // Responsive cancellation never waits behind a pending Apply's mutex.
            if matches!(
                request["command"].as_str(),
                Some("stop" | "recover" | "shutdown")
            ) || request["command"] == "pause" && request["paused"] == true
            {
                components.epoch.fetch_add(1, Ordering::AcqRel);
            }
            let mut pending = Vec::new();
            for job in jobs.drain(..) {
                if job.is_finished() {
                    job.join().map_err(|_| "Qualification request panicked")?;
                } else {
                    pending.push(job);
                }
            }
            jobs = pending;
            if jobs.len() >= 8 {
                emit(
                    &output,
                    &json!({"ok":false,"error":"Qualification request budget exceeded"}),
                )?;
                continue;
            }
            let app = application.clone();
            let output = output.clone();
            jobs.push(thread::spawn(move || {
                let result = app.request(&request);
                let reply = match result {
                    Ok(value) => json!({"requestId":request["requestId"],"ok":true,"state":value}),
                    Err(error) => {
                        json!({"requestId":request["requestId"],"ok":false,"error":error})
                    }
                };
                let _ = emit(&output, &reply);
            }));
        }
        Ok(())
    })();
    components.epoch.fetch_add(1, Ordering::AcqRel);
    let mut requests = Ok(());
    for job in jobs {
        if job.join().is_err() {
            requests = Err("Qualification request panicked".into());
        }
    }
    let stop = application
        .coordinator
        .lock()
        .map_err(err)
        .and_then(|mut c| c.stop());
    application.closing.store(true, Ordering::Release);
    let _ = worker.join();
    components.shutdown();
    serving.and(requests).and(stop)
}

pub fn main() -> i32 {
    let result = (|| -> Result<(), String> {
        if !cfg!(windows) {
            return Err("The selected manual qualification is Windows-only".into());
        }
        let args = std::env::args().skip(1).collect::<Vec<_>>();
        if args.len() < 3 || args[0] != MODE {
            return Err("Explicit --native-profile-qualification preflight|serve <private-config.json> required; no fallback exists".into());
        }
        let path = PathBuf::from(&args[2]);
        if !path.is_absolute() {
            return Err("Use an absolute private configuration path".into());
        }
        let configuration = config(&path)?;
        // These path anchors are set before constructing any native components.
        std::env::set_var(
            "OSG_DCS_BIOS_CONFIG",
            configuration.private_root.join("dcs-anchor.json"),
        );
        std::env::set_var(
            "OSG_GAMES_CONFIG",
            configuration.private_root.join("games-anchor.json"),
        );
        match args[1].as_str() {
            "preflight" if args.len() == 3 => {
                let components = Components::new(&configuration);
                let result = components.preflight();
                components.shutdown();
                emit(
                    &Mutex::new(io::stdout()),
                    &json!({"ok":result.is_ok(),"result":result}),
                )
            }
            "serve" if args.len() == 3 => serve(configuration, &path),
            #[cfg(windows)]
            "guardian" if args.len() == 5 => guardian(
                &configuration,
                serde_json::from_str(&args[3]).map_err(err)?,
                &args[4],
            ),
            _ => Err(
                "Unknown qualification mode or extra arguments; no Manager/service fallback".into(),
            ),
        }
    })();
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("Manual qualification refused: {error}");
            2
        }
    }
}
