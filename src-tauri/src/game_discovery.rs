//! Installation discovery and launching are kept separate from peripheral sessions.
//! Steam app IDs come from the publishers' store pages; no commands or launch
//! arguments from saved profiles are executed.
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub(crate) trait Platform: Send + Sync {
    fn integration_setup_supported(&self) -> bool {
        false
    }
    fn catalogue(&self, manual: &BTreeMap<String, String>) -> Vec<Value>;
    fn validate(&self, game_id: &str, executable: &str) -> Result<(), String>;
    /// Only create the native process. Unified launch admission must not hold
    /// its short ownership guard while waiting for a process to appear.
    fn spawn_process(&self, _game: &Value) -> Result<(), String> {
        Err("This platform has no admitted process-only game launcher.".into())
    }
    fn launch(&self, game: &Value) -> Result<ProcessIdentity, String>;
    fn find(&self, game: &Value) -> Result<Option<ProcessIdentity>, String>;
    /// Driving telemetry requires its producer, independently of the game menu.
    fn integration_process(&self, game: &Value) -> Result<Option<ProcessIdentity>, String> {
        self.find(game)
    }
    fn alive(&self, identity: &ProcessIdentity) -> Result<bool, String>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProcessIdentity {
    pub pid: u32,
    pub path: String,
    /// Windows process creation FILETIME, distinct from a reusable PID.
    pub epoch: u64,
}
impl ProcessIdentity {
    pub(crate) fn value(&self) -> Value {
        json!({"pid":self.pid,"path":self.path,"epoch":self.epoch})
    }
    pub(crate) fn parse(value: &Value) -> Result<Self, String> {
        let pid = value["pid"]
            .as_u64()
            .and_then(|p| u32::try_from(p).ok())
            .filter(|p| *p != 0)
            .ok_or("Saved game process ID is invalid.")?;
        let path = value["path"]
            .as_str()
            .filter(|p| !p.is_empty() && p.len() <= 32768 && !p.contains('\0'))
            .ok_or("Saved game process path is invalid.")?;
        let epoch = value["epoch"]
            .as_u64()
            .filter(|e| *e != 0)
            .ok_or("Saved game process creation time is invalid.")?;
        Ok(Self {
            pid,
            path: path.into(),
            epoch,
        })
    }
}

#[derive(Default)]
pub(crate) struct NativePlatform;

pub(crate) fn exclusive_xplane(expected: &ProcessIdentity) -> Result<(), String> {
    let actual = native::find(&["X-Plane.exe".into()])?;
    if actual.as_ref() != Some(expected) {
        return Err("The running X-Plane process does not match the selected installation/session. Stop and activate its game profile again.".into());
    }
    Ok(())
}

struct SupportedGame {
    id: &'static str,
    name: &'static str,
    provider: &'static str,
    steam_id: u32,
    executables: &'static [&'static str],
}
const GAMES: &[SupportedGame] = &[
    SupportedGame {
        id: "dcs",
        name: "DCS World",
        provider: "dcs-bios",
        steam_id: 223750,
        executables: &["bin/DCS.exe", "bin-mt/DCS.exe"],
    },
    SupportedGame {
        id: "assetto-corsa-rally",
        name: "Assetto Corsa Rally",
        provider: "assetto-corsa-rally",
        steam_id: 3917090,
        // The Unreal runtime is preferred over a root bootstrap executable.
        executables: &["acr/Binaries/Win64/acr.exe", "acr.exe"],
    },
    SupportedGame {
        id: "x-plane-11",
        name: "X-Plane 11",
        provider: "x-plane",
        steam_id: 269950,
        executables: &["X-Plane.exe"],
    },
    SupportedGame {
        id: "x-plane-12",
        name: "X-Plane 12",
        provider: "x-plane",
        steam_id: 2014780,
        executables: &["X-Plane.exe"],
    },
    SupportedGame {
        id: "assetto-corsa",
        name: "Assetto Corsa",
        provider: "assetto-corsa",
        steam_id: 244210,
        // The launcher does not publish driving telemetry; watch the simulator.
        executables: &["acs.exe", "acs_x86.exe"],
    },
    SupportedGame {
        id: "assetto-corsa-competizione",
        name: "Assetto Corsa Competizione",
        provider: "assetto-corsa-competizione",
        steam_id: 805550,
        executables: &["AC2/Binaries/Win64/AC2-Win64-Shipping.exe", "AC2.exe"],
    },
    SupportedGame {
        id: "assetto-corsa-evo",
        name: "Assetto Corsa EVO",
        provider: "assetto-corsa-evo",
        steam_id: 3058630,
        executables: &["AssettoCorsaEVO.exe"],
    },
];
fn supported(id: &str) -> Result<&'static SupportedGame, String> {
    GAMES
        .iter()
        .find(|g| g.id == id)
        .ok_or_else(|| "Choose a supported game.".into())
}

pub(crate) fn executable_picker(game_id: &str) -> Result<(String, &'static str), String> {
    let game = supported(game_id)?;
    let executable = game.executables[0].rsplit('/').next().unwrap();
    Ok((
        format!("Choose {} executable ({executable})", game.name),
        executable,
    ))
}

#[derive(Default)]
struct Roots {
    steam: Vec<PathBuf>,
    dcs: Vec<PathBuf>,
    launch_supported: bool,
}
fn add_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.contains(&path) {
        paths.push(path);
    }
}
fn path_string(path: &Path) -> Option<String> {
    path.to_str().map(str::to_owned)
}
fn normalized_path(path: &str) -> String {
    path.replace('/', "\\")
        .trim_start_matches("\\\\?\\")
        .to_lowercase()
}
fn paths_match(left: &str, right: &str) -> bool {
    normalized_path(left) == normalized_path(right)
}
fn validated_executable(game: &SupportedGame, path: &Path) -> Result<PathBuf, String> {
    let expected = game.executables[0].rsplit('/').next().unwrap();
    if !path.is_absolute()
        || !path
            .file_name()
            .and_then(|p| p.to_str())
            .is_some_and(|p| game.executables.iter().any(|exe| p.eq_ignore_ascii_case(exe.rsplit('/').next().unwrap())))
        || !path.is_file()
    {
        return Err(format!(
            "Select an existing {expected} executable for {}.",
            game.name
        ));
    }
    let path = fs::canonicalize(path).map_err(|e| format!("Cannot access game executable: {e}"))?;
    if game.provider == "x-plane"
        && crate::xplane_installation::version(&path)?
            != game
                .id
                .trim_start_matches("x-plane-")
                .parse::<u64>()
                .unwrap()
    {
        return Err(format!(
            "The selected executable does not belong to {}.",
            game.name
        ));
    }
    if path_string(&path).is_none() {
        return Err("Game executable path is not valid text.".into());
    }
    Ok(path)
}
fn installation(
    game: &SupportedGame,
    selection: &Path,
) -> Option<(PathBuf, Vec<PathBuf>, PathBuf)> {
    if selection.is_file() {
        let executable = validated_executable(game, selection).ok()?;
        let mut root = executable.parent()?.to_path_buf();
        if game.id == "dcs"
            && matches!(
                root.file_name()?.to_str()?.to_ascii_lowercase().as_str(),
                "bin" | "bin-mt"
            )
        {
            root = root.parent()?.to_path_buf();
        } else if game.id != "dcs" && root.file_name()?.to_str()?.eq_ignore_ascii_case("Win64") {
            root = root.parent()?.parent()?.parent()?.to_path_buf();
        }
        let mut paths = vec![executable.clone()];
        for relative in game.executables {
            if let Ok(path) = validated_executable(game, &root.join(relative)) {
                add_path(&mut paths, path);
            }
        }
        return Some((executable, paths, root));
    }
    if !selection.is_absolute() {
        return None;
    }
    let root = fs::canonicalize(selection).ok()?;
    let paths: Vec<_> = game
        .executables
        .iter()
        .filter_map(|p| validated_executable(game, &root.join(p)).ok())
        .collect();
    Some((paths.first()?.clone(), paths, root))
}

// A small bounded KeyValues reader accepts current and legacy Steam manifests.
// It preserves Windows backslashes and Unicode, and rejects incomplete objects.
#[derive(Debug, PartialEq)]
enum Vdf {
    Text(String),
    Object(Vec<(String, Vdf)>),
}
impl Vdf {
    fn get(&self, name: &str) -> Option<&Vdf> {
        match self {
            Self::Object(entries) => entries
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v),
            _ => None,
        }
    }
    fn text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            _ => None,
        }
    }
}
fn parse_vdf(input: &str) -> Result<Vdf, String> {
    if input.len() > 1_048_576 {
        return Err("Steam manifest is too large.".into());
    }
    let mut chars = input.chars().peekable();
    let mut tokens = Vec::new();
    while let Some(c) = chars.next() {
        if c.is_whitespace() || c == '\u{feff}' {
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            for c in chars.by_ref() {
                if c == '\n' {
                    break;
                }
            }
            continue;
        }
        if c == '{' || c == '}' {
            tokens.push(c.to_string());
            continue;
        }
        let mut word = String::new();
        if c == '"' {
            let mut closed = false;
            while let Some(c) = chars.next() {
                if c == '"' {
                    closed = true;
                    break;
                }
                if c == '\\' && matches!(chars.peek(), Some('\\' | '"')) {
                    word.push(chars.next().unwrap());
                } else {
                    word.push(c);
                }
            }
            if !closed {
                return Err("Steam manifest has an incomplete string.".into());
            }
        } else {
            word.push(c);
            while chars
                .peek()
                .is_some_and(|c| !c.is_whitespace() && *c != '{' && *c != '}')
            {
                word.push(chars.next().unwrap());
            }
        }
        // Prefixing scalar tokens means quoted literal braces stay scalars.
        tokens.push(format!("s{word}"));
    }
    fn object(tokens: &[String], cursor: &mut usize, depth: usize) -> Result<Vdf, String> {
        if depth > 16 {
            return Err("Steam manifest nesting is too deep.".into());
        }
        let mut entries = Vec::new();
        while *cursor < tokens.len() {
            if tokens[*cursor] == "}" {
                if depth == 0 {
                    return Err("Steam manifest has an unexpected brace.".into());
                }
                *cursor += 1;
                return Ok(Vdf::Object(entries));
            }
            let key = tokens[*cursor]
                .strip_prefix('s')
                .ok_or("Steam manifest has an invalid key.")?
                .to_owned();
            *cursor += 1;
            let value = tokens
                .get(*cursor)
                .ok_or("Steam manifest has a missing value.")?;
            *cursor += 1;
            let value = if value == "{" {
                object(tokens, cursor, depth + 1)?
            } else {
                Vdf::Text(
                    value
                        .strip_prefix('s')
                        .ok_or("Steam manifest has an invalid value.")?
                        .into(),
                )
            };
            entries.push((key, value));
        }
        if depth != 0 {
            return Err("Steam manifest has an incomplete object.".into());
        }
        Ok(Vdf::Object(entries))
    }
    object(&tokens, &mut 0, 0)
}
fn read_vdf(path: &Path) -> Option<Vdf> {
    let mut input = String::new();
    fs::File::open(path)
        .ok()?
        .take(1_048_577)
        .read_to_string(&mut input)
        .ok()?;
    parse_vdf(&input).ok()
}
fn library_paths(root: &Path) -> Vec<PathBuf> {
    let mut paths = vec![root.to_path_buf()];
    let manifest = read_vdf(&root.join("steamapps/libraryfolders.vdf"))
        .or_else(|| read_vdf(&root.join("config/libraryfolders.vdf")));
    if let Some(Vdf::Object(entries)) = manifest.as_ref().and_then(|m| m.get("libraryfolders")) {
        for (key, value) in entries {
            if key.parse::<u32>().is_err() {
                continue;
            }
            if let Some(path) = value
                .text()
                .or_else(|| value.get("path").and_then(Vdf::text))
            {
                let path = PathBuf::from(path);
                if path.is_absolute() {
                    add_path(&mut paths, path);
                }
            }
        }
    }
    paths
}
fn steam_installation(game: &SupportedGame, library: &Path) -> Option<PathBuf> {
    let manifest = read_vdf(&library.join(format!("steamapps/appmanifest_{}.acf", game.steam_id)))?;
    let state = manifest.get("AppState")?;
    if state.get("appid")?.text()?.parse::<u32>().ok()? != game.steam_id {
        return None;
    }
    let name = state.get("installdir")?.text()?;
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', ':', '\0']) {
        return None;
    }
    Some(library.join("steamapps/common").join(name))
}
fn discover(roots: &Roots, manual: &BTreeMap<String, String>) -> Vec<Value> {
    let mut steam_installs = Vec::new();
    let steam_executable = roots
        .steam
        .iter()
        .map(|p| p.join("steam.exe"))
        .find(|p| p.is_file());
    for steam in &roots.steam {
        for library in library_paths(steam) {
            for game in GAMES {
                if let Some(root) = steam_installation(game, &library) {
                    if let Some(install) = installation(game, &root) {
                        steam_installs.push((game.id, install));
                    }
                }
            }
        }
    }
    GAMES.iter().map(|game| {
        let selected = manual.get(game.id).filter(|s| !s.trim().is_empty());
        let mut source = "not-found";
        let found = if let Some(selected) = selected {
            source = "manual";
            installation(game, Path::new(selected))
        } else if let Some((_, install)) = steam_installs.iter().find(|(id,_)| *id == game.id) {
            source = "steam";
            Some(install.clone())
        } else if game.id == "dcs" {
            let install = roots.dcs.iter().find_map(|p|installation(game,p));
            if install.is_some() { source = "standalone"; }
            install
        } else if game.provider == "x-plane" {
            let install=crate::xplane_installation::roots().iter().find_map(|p|installation(game,p));
            if install.is_some() {source="standalone";}
            install
        } else { None };
        let matches_steam = found.as_ref().is_some_and(|(_, _, root)| {
            steam_installs.iter().any(|(id, (_, _, steam_root))| {
                *id == game.id && paths_match(&root.to_string_lossy(), &steam_root.to_string_lossy())
            })
        });
        let steam = game.provider != "x-plane" && (source == "steam" || matches_steam || game.id.starts_with("assetto-corsa") && found.is_some());
        // X-Plane supports direct launch, including a selected secondary copy.
        // Steam -applaunch may launch another installation, so monitor/launch the exact executable.
        let mut installations: Vec<Value> = steam_installs.iter().filter(|(id,_)|*id==game.id).map(|(_, (exe,_,root))|json!({"executable":exe,"installPath":root,"source":"steam"})).collect();
        let mut installation_paths=BTreeSet::new();
        installations.retain(|i|installation_paths.insert(normalized_path(i["executable"].as_str().unwrap())));
        if game.provider == "x-plane" {
            for root in crate::xplane_installation::roots() {
                if let Some((exe,_,root))=installation(game,&root) { if !installations.iter().any(|i|i["executable"]==json!(exe)) { installations.push(json!({"executable":exe,"installPath":root,"source":"standalone"})); } }
            }
            if let Some((exe,_,root))=&found { if !installations.iter().any(|i|i["executable"]==json!(exe)) { installations.push(json!({"executable":exe,"installPath":root,"source":source})); } }
        }
        let executable = found.as_ref().and_then(|(exe,_,_)|path_string(exe));
        let install_path = found.as_ref().and_then(|(_,_,root)|path_string(root));
        let process_executables: Vec<_> = found.as_ref().map(|(_,paths,_)|paths.iter().filter_map(|p|path_string(p)).collect()).unwrap_or_default();
        let launch_supported = roots.launch_supported && found.is_some() && (!steam || steam_executable.is_some());
        let reason = if !roots.launch_supported { Some("Game launch and process monitoring require Windows.") }
            else if steam && steam_executable.is_none() { Some("Install Steam to launch this game.") }
            else { None };
        json!({"id":game.id,"name":game.name,"provider":game.provider,
            "installed":found.is_some(),"executable":executable,"installPath":install_path,
            "source":source,"manualPath":selected,"installations":installations,"launchSupported":launch_supported,"processMonitoringSupported":roots.launch_supported,
            "launchUnavailableReason":reason,"steamAppId":if steam {Some(game.steam_id)} else {None},
            "steamExecutable":if steam {steam_executable.as_ref().and_then(|p|path_string(p))} else {None},
            "processExecutables":process_executables})
    }).collect()
}

#[derive(Debug, PartialEq)]
struct LaunchRequest {
    executable: PathBuf,
    directory: PathBuf,
    arguments: Vec<String>,
}
fn launch_request(game: &Value) -> Result<LaunchRequest, String> {
    let spec = supported(game["id"].as_str().ok_or("Choose a supported game.")?)?;
    let path = game["executable"]
        .as_str()
        .ok_or("Game installation was not found.")?;
    let executable = validated_executable(spec, Path::new(path))?;
    if !game["installed"].as_bool().unwrap_or(false) {
        return Err("Game installation was not found.".into());
    }
    if let Some(app_id) = game["steamAppId"].as_u64() {
        if app_id != spec.steam_id as u64 {
            return Err("Steam launch ID does not match the supported game.".into());
        }
        let steam = PathBuf::from(
            game["steamExecutable"]
                .as_str()
                .ok_or("Steam executable was not found.")?,
        );
        if !steam.is_absolute()
            || !steam.is_file()
            || !steam
                .file_name()
                .and_then(|p| p.to_str())
                .is_some_and(|p| p.eq_ignore_ascii_case("steam.exe"))
        {
            return Err("Steam executable was not found.".into());
        }
        let directory = steam
            .parent()
            .ok_or("Steam directory was not found.")?
            .into();
        return Ok(LaunchRequest {
            executable: steam,
            directory,
            arguments: vec!["-applaunch".into(), spec.steam_id.to_string()],
        });
    }
    if spec.id.starts_with("assetto-corsa") {
        return Err(format!("{} must be launched through Steam.", spec.name));
    }
    let (_, _, directory) =
        installation(spec, &executable).ok_or("Game installation directory was not found.")?;
    Ok(LaunchRequest {
        executable,
        directory,
        arguments: vec![],
    })
}
fn process_paths(game: &Value) -> Result<Vec<String>, String> {
    let spec = supported(game["id"].as_str().ok_or("Choose a supported game.")?)?;
    let selected = game["executable"]
        .as_str()
        .ok_or("Game installation was not found.")?;
    let (_, paths, _) =
        installation(spec, Path::new(selected)).ok_or("Game executable is no longer available.")?;
    // Unreal's root bootstrap can exit after spawning the real runtime. Watch
    // the runtime when present so a successful handoff does not restore hardware.
    if matches!(spec.id, "assetto-corsa-rally" | "assetto-corsa-competizione") {
        if let Some(path) = paths.iter().filter_map(|p| path_string(p))
            .find(|p| normalized_path(p).contains("\\binaries\\win64\\")) {
            return Ok(vec![path]);
        }
    }
    Ok(paths.into_iter().filter_map(|p| path_string(&p)).collect())
}
fn find_game_process(
    game: &Value,
    integration: bool,
    mut inspect: impl FnMut(&[String]) -> Result<Option<ProcessIdentity>, String>,
) -> Result<Option<ProcessIdentity>, String> {
    let runtime = inspect(&process_paths(game)?)?;
    if runtime.is_some() || integration || game["id"] != "assetto-corsa" {
        return Ok(runtime);
    }
    // Steam starts AC's persistent menu before a user chooses a car and track.
    // It counts as a launched game, but never as the shared-memory producer.
    let spec = supported("assetto-corsa")?;
    let selected = game["executable"].as_str().ok_or("Choose an Assetto Corsa installation.")?;
    let (_, _, root) = installation(spec, Path::new(selected)).ok_or("The Assetto Corsa installation changed.")?;
    let launcher = root.join("AssettoCorsa.exe");
    if !launcher.is_file() { return Ok(None); }
    let launcher = fs::canonicalize(launcher).map_err(|error|format!("Cannot inspect the Assetto Corsa launcher: {error}"))?;
    inspect(&[path_string(&launcher).ok_or("The Assetto Corsa launcher path is not valid text.")?])
}
fn wait_for_process(
    timeout: Duration,
    mut find: impl FnMut() -> Result<Option<ProcessIdentity>, String>,
    mut pause: impl FnMut(Duration),
) -> Result<ProcessIdentity, String> {
    let start = Instant::now();
    loop {
        if let Some(process) = find()? {
            return Ok(process);
        }
        if start.elapsed() >= timeout {
            return Err("Game did not start before the startup timeout. The previous hardware setup will be restored.".into());
        }
        pause(Duration::from_millis(250).min(timeout.saturating_sub(start.elapsed())));
    }
}
impl Platform for NativePlatform {
    fn integration_setup_supported(&self) -> bool {
        cfg!(windows)
    }
    fn catalogue(&self, manual: &BTreeMap<String, String>) -> Vec<Value> {
        discover(&native::roots(), manual)
    }
    fn validate(&self, game_id: &str, executable: &str) -> Result<(), String> {
        validated_executable(supported(game_id)?, Path::new(executable)).map(|_| ())
    }
    fn spawn_process(&self, game: &Value) -> Result<(), String> {
        if !cfg!(windows) {
            return Err("Game launch and process monitoring require Windows.".into());
        }
        let request = launch_request(game)?;
        if self.find(game)?.is_some() {
            return Err(
                "This game is already running. Close it before launching a new profile.".into(),
            );
        }
        native::spawn(&request)
    }
    fn launch(&self, game: &Value) -> Result<ProcessIdentity, String> {
        self.spawn_process(game)?;
        wait_for_process(
            Duration::from_secs(45),
            || self.find(game),
            std::thread::sleep,
        )
    }
    fn find(&self, game: &Value) -> Result<Option<ProcessIdentity>, String> {
        find_game_process(game, false, native::find)
    }
    fn integration_process(&self, game: &Value) -> Result<Option<ProcessIdentity>, String> {
        find_game_process(game, true, native::find)
    }
    fn alive(&self, identity: &ProcessIdentity) -> Result<bool, String> {
        native::alive(identity)
    }
}

#[cfg(not(windows))]
mod native {
    use super::*;
    pub(super) fn roots() -> Roots {
        let mut roots = Roots::default();
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            for relative in [
                "Library/Application Support/Steam",
                ".steam/steam",
                ".local/share/Steam",
                ".var/app/com.valvesoftware.Steam/.local/share/Steam",
            ] {
                add_path(&mut roots.steam, home.join(relative));
            }
        }
        roots
    }
    pub(super) fn spawn(_: &LaunchRequest) -> Result<(), String> {
        Err("Game launch requires Windows.".into())
    }
    pub(super) fn find(_: &[String]) -> Result<Option<ProcessIdentity>, String> {
        Err("Game process monitoring requires Windows.".into())
    }
    pub(super) fn alive(_: &ProcessIdentity) -> Result<bool, String> {
        Err("Game process monitoring requires Windows.".into())
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::{ffi::c_void, mem, ptr};
    type RawHandle = *mut c_void;
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[repr(C)]
    struct ProcessEntry {
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
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> RawHandle;
        fn Process32FirstW(snapshot: RawHandle, entry: *mut ProcessEntry) -> i32;
        fn Process32NextW(snapshot: RawHandle, entry: *mut ProcessEntry) -> i32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> RawHandle;
        fn GetProcessTimes(
            handle: RawHandle,
            created: *mut FileTime,
            exited: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        fn QueryFullProcessImageNameW(
            handle: RawHandle,
            flags: u32,
            buffer: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn WaitForSingleObject(handle: RawHandle, milliseconds: u32) -> u32;
        fn CloseHandle(handle: RawHandle) -> i32;
    }
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegGetValueW(
            key: RawHandle,
            subkey: *const u16,
            name: *const u16,
            flags: u32,
            kind: *mut u32,
            data: *mut c_void,
            size: *mut u32,
        ) -> i32;
    }
    struct Handle(RawHandle);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    fn registry(key: i32, subkey: &str, name: &str) -> Option<PathBuf> {
        let mut value = vec![0u16; 32768];
        let mut length = (value.len() * 2) as u32;
        let result = unsafe {
            RegGetValueW(
                key as isize as RawHandle,
                wide(subkey).as_ptr(),
                wide(name).as_ptr(),
                6,
                ptr::null_mut(),
                value.as_mut_ptr().cast(),
                &mut length,
            )
        };
        if result != 0 || length < 2 || length as usize > value.len() * 2 {
            return None;
        }
        value.truncate(length as usize / 2);
        let length = value.iter().position(|c| *c == 0).unwrap_or(value.len());
        let value = String::from_utf16(&value[..length]).ok()?;
        if value.is_empty() {
            None
        } else {
            Some(PathBuf::from(value))
        }
    }
    pub(super) fn roots() -> Roots {
        let mut roots = Roots {
            launch_supported: true,
            ..Roots::default()
        };
        for (key, subkey, name) in [
            (0x80000001u32 as i32, "Software\\Valve\\Steam", "SteamPath"),
            (
                0x80000002u32 as i32,
                "SOFTWARE\\WOW6432Node\\Valve\\Steam",
                "InstallPath",
            ),
            (
                0x80000002u32 as i32,
                "SOFTWARE\\Valve\\Steam",
                "InstallPath",
            ),
        ] {
            if let Some(path) = registry(key, subkey, name) {
                add_path(&mut roots.steam, path);
            }
        }
        for name in ["DCS World", "DCS World OpenBeta"] {
            if let Some(path) = registry(
                0x80000001u32 as i32,
                &format!("Software\\Eagle Dynamics\\{name}"),
                "Path",
            ) {
                add_path(&mut roots.dcs, path);
            }
        }
        for env in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Some(root) = std::env::var_os(env) {
                let root = PathBuf::from(root);
                add_path(&mut roots.steam, root.join("Steam"));
                for name in ["DCS World", "DCS World OpenBeta"] {
                    add_path(&mut roots.dcs, root.join("Eagle Dynamics").join(name));
                }
            }
        }
        roots
    }
    fn open(pid: u32) -> Result<Option<Handle>, String> {
        let handle = unsafe { OpenProcess(0x1000 | 0x00100000, 0, pid) };
        if handle.is_null() {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(87) {
                return Ok(None);
            }
            return Err(format!("Cannot inspect game process {pid}: {error}"));
        }
        Ok(Some(Handle(handle)))
    }
    fn identity(handle: &Handle, pid: u32) -> Result<Option<ProcessIdentity>, String> {
        let state = unsafe { WaitForSingleObject(handle.0, 0) };
        if state == 0 {
            return Ok(None);
        }
        if state != 258 {
            return Err("Cannot determine whether the game process is running.".into());
        }
        let (mut created, mut exited, mut kernel, mut user) = (
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
        );
        if unsafe { GetProcessTimes(handle.0, &mut created, &mut exited, &mut kernel, &mut user) }
            == 0
        {
            return Err(format!(
                "Cannot read game process creation time: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut image = vec![0u16; 32768];
        let mut length = image.len() as u32;
        if unsafe { QueryFullProcessImageNameW(handle.0, 0, image.as_mut_ptr(), &mut length) } == 0
        {
            return Err(format!(
                "Cannot read game process executable: {}",
                std::io::Error::last_os_error()
            ));
        }
        let path = String::from_utf16(&image[..length as usize])
            .map_err(|_| "Game process path is not valid text.")?;
        let epoch = ((created.high as u64) << 32) | created.low as u64;
        if epoch == 0 {
            return Err("Game process creation time is unavailable.".into());
        }
        if unsafe { WaitForSingleObject(handle.0, 0) } != 258 {
            return Ok(None);
        }
        Ok(Some(ProcessIdentity { pid, path, epoch }))
    }
    pub(super) fn spawn(request: &LaunchRequest) -> Result<(), String> {
        std::process::Command::new(&request.executable)
            .args(&request.arguments)
            .current_dir(&request.directory)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Could not launch the game: {e}"))
    }
    pub(super) fn find(paths: &[String]) -> Result<Option<ProcessIdentity>, String> {
        let exclusive = paths == ["X-Plane.exe"];
        let mut selected = None;
        let raw = unsafe { CreateToolhelp32Snapshot(2, 0) };
        if raw as isize == -1 {
            return Err(format!(
                "Cannot list game processes: {}",
                std::io::Error::last_os_error()
            ));
        }
        let snapshot = Handle(raw);
        // PROCESSENTRY32W is plain integers plus an inline UTF-16 buffer.
        let mut entry: ProcessEntry = unsafe { mem::zeroed() };
        entry.size = mem::size_of::<ProcessEntry>() as u32;
        let mut available = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
        if !available && std::io::Error::last_os_error().raw_os_error() != Some(18) {
            return Err(format!(
                "Cannot inspect game process list: {}",
                std::io::Error::last_os_error()
            ));
        }
        while available {
            let length = entry.executable.iter().position(|c| *c == 0).unwrap_or(260);
            let basename = String::from_utf16_lossy(&entry.executable[..length]);
            if paths.iter().any(|p| {
                p.rsplit(['\\', '/'])
                    .next()
                    .is_some_and(|s| s.eq_ignore_ascii_case(&basename))
            }) {
                if let Some(handle) = open(entry.pid)? {
                    if let Some(identity) = identity(&handle, entry.pid)? {
                        if exclusive || paths.iter().any(|p| paths_match(p, &identity.path)) {
                            if selected.is_some() {
                                return Err(if exclusive {
                                    "Multiple X-Plane processes are running. Close the other installations before using telemetry."
                                } else {
                                    "Multiple game processes match the selected installation. Close the other instances before activating its profile."
                                }.into());
                            }
                            selected = Some(identity);
                        }
                    }
                }
            }
            available = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
            if !available && std::io::Error::last_os_error().raw_os_error() != Some(18) {
                return Err(format!(
                    "Cannot continue inspecting game processes: {}",
                    std::io::Error::last_os_error()
                ));
            }
        }
        Ok(selected)
    }
    pub(super) fn alive(expected: &ProcessIdentity) -> Result<bool, String> {
        let Some(handle) = open(expected.pid)? else {
            return Ok(false);
        };
        Ok(identity(&handle, expected.pid)?.is_some_and(|actual| {
            actual.epoch == expected.epoch && paths_match(&actual.path, &expected.path)
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn executable_picker_uses_the_selected_game_and_rejects_unknown_games() {
        for (game, name, exe) in [
            ("dcs", "DCS World", "DCS.exe"),
            ("assetto-corsa-rally", "Assetto Corsa Rally", "acr.exe"),
            ("assetto-corsa", "Assetto Corsa", "acs.exe"),
            ("assetto-corsa-competizione", "Assetto Corsa Competizione", "AC2-Win64-Shipping.exe"),
            ("assetto-corsa-evo", "Assetto Corsa EVO", "AssettoCorsaEVO.exe"),
            ("x-plane-11", "X-Plane 11", "X-Plane.exe"),
            ("x-plane-12", "X-Plane 12", "X-Plane.exe"),
        ] {
            let (title, filename) = executable_picker(game).unwrap();
            assert_eq!(filename, exe);
            assert!(title.contains(name));
        }
        assert!(executable_picker("unknown").is_err());
    }
    #[test]
    fn xplane_steam_versions_manual_copies_and_launch_use_the_selected_installation() {
        let scratch = Scratch::new();
        let steam = scratch
            .write("Steam/steam.exe", "")
            .parent()
            .unwrap()
            .to_path_buf();
        for (v, appid) in [(11, 269950), (12, 2014780)] {
            scratch.write(
                &format!("Steam/steamapps/appmanifest_{appid}.acf"),
                &format!("AppState {{ appid {appid} installdir \"X-Plane {v}\" }}"),
            );
            scratch.write(
                &format!("Steam/steamapps/common/X-Plane {v}/X-Plane.exe"),
                "",
            );
            scratch.write(
                &format!("Steam/steamapps/common/X-Plane {v}/Resources/plugins/DataRefs.txt"),
                &format!("2 {v}10 Wed Jun 24 15:46:43 2026\n\n"),
            );
        }
        let roots = Roots {
            steam: vec![steam.clone(), steam],
            launch_supported: true,
            ..Roots::default()
        };
        let games = discover(&roots, &BTreeMap::new());
        for (game, v) in games[2..4].iter().zip([11, 12]) {
            assert_eq!(game["id"], format!("x-plane-{v}"));
            assert_eq!(game["installed"], true);
            assert_eq!(game["source"], "steam");
            assert_eq!(game["installations"].as_array().unwrap().len(), 1);
            let request = launch_request(game).unwrap();
            assert_eq!(request.executable.to_str(), game["executable"].as_str());
            assert!(request.arguments.is_empty());
            assert_eq!(
                process_paths(game).unwrap(),
                vec![game["executable"].as_str().unwrap()]
            );
        }
        let copy = scratch.write("Standalone copy/X-Plane.exe", "");
        scratch.write(
            "Standalone copy/Resources/plugins/DataRefs.txt",
            "2 1210 Wed Jun 24 15:46:43 2026\n\n",
        );
        assert!(NativePlatform
            .validate("x-plane-11", copy.to_str().unwrap())
            .is_err());
        assert!(NativePlatform
            .validate("x-plane-12", copy.to_str().unwrap())
            .is_ok());
        let manual = BTreeMap::from([("x-plane-12".into(), copy.to_str().unwrap().into())]);
        let game = discover(&roots, &manual).remove(3);
        assert_eq!(game["source"], "manual");
        assert_eq!(game["executable"], copy.to_str().unwrap());
        assert_eq!(game["installations"].as_array().unwrap().len(), 2);
        assert_eq!(launch_request(&game).unwrap().executable, copy);
    }
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../.research/game-discovery-tests")
                .join(uuid::Uuid::new_v4().to_string());
            fs::create_dir_all(&root).unwrap();
            Self(fs::canonicalize(root).unwrap())
        }
        fn write(&self, path: &str, text: &str) -> PathBuf {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, text).unwrap();
            path
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn process() -> ProcessIdentity {
        ProcessIdentity {
            pid: 10,
            path: "C:\\Games\\DCS\\bin\\DCS.exe".into(),
            epoch: 50,
        }
    }
    #[test]
    fn process_identity_requires_nonzero_pid_epoch_and_text_path() {
        let identity = process();
        assert_eq!(ProcessIdentity::parse(&identity.value()).unwrap(), identity);
        for value in [
            json!({}),
            json!({"pid":0,"path":"a","epoch":1}),
            json!({"pid":u64::MAX,"path":"a","epoch":1}),
            json!({"pid":1,"path":"","epoch":1}),
            json!({"pid":1,"path":"a\u{0000}","epoch":1}),
            json!({"pid":1,"path":"a","epoch":0}),
        ] {
            assert!(ProcessIdentity::parse(&value).is_err());
        }
        assert!(paths_match("\\\\?\\C:\\Games\\DCS.exe", "c:/games/dcs.exe"));
        assert!(!paths_match("C:\\Games\\DCS.exe", "D:\\Games\\DCS.exe"));
    }
    #[test]
    fn vdf_preserves_quoted_values_escapes_unicode_comments_and_legacy_fields() {
        let parsed=parse_vdf("\u{feff}// heading\n LibraryFolders { 0 \"C:\\\\Steam\" \"1\" { path \"D:\\\\Spiele\\\\Rally ü\" label \"\\\"{\\\"\" } } ").unwrap();
        let folders = parsed.get("libraryfolders").unwrap();
        assert_eq!(folders.get("0").unwrap().text(), Some("C:\\Steam"));
        assert_eq!(
            folders.get("1").unwrap().get("path").unwrap().text(),
            Some("D:\\Spiele\\Rally ü")
        );
        assert_eq!(
            folders.get("1").unwrap().get("label").unwrap().text(),
            Some("\"{\"")
        );
        assert_eq!(
            parse_vdf("key \"C:\\Games\\DCS\"")
                .unwrap()
                .get("key")
                .unwrap()
                .text(),
            Some("C:\\Games\\DCS")
        );
        assert!(parsed.text().is_none());
        assert!(folders.get("none").is_none());
        assert!(Vdf::Text("scalar".into()).get("key").is_none());
    }
    #[test]
    fn incomplete_excessive_and_malformed_manifests_are_rejected() {
        for input in [
            "{",
            "}",
            "key",
            "key }",
            "key { value x",
            "key \"unterminated",
        ] {
            assert!(parse_vdf(input).is_err(), "{input}");
        }
        assert!(parse_vdf(&"x".repeat(1_048_577)).is_err());
        assert!(parse_vdf(&format!("{}{}", "key { ".repeat(18), "}".repeat(18))).is_err());
        let scratch = Scratch::new();
        assert!(read_vdf(&scratch.0.join("missing")).is_none());
        assert!(read_vdf(&scratch.write("large.vdf", &"x".repeat(1_048_577))).is_none());
    }
    fn vdf_path(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "\\\\")
    }
    #[test]
    fn discovers_steam_from_root_and_secondary_library_manifests() {
        let scratch = Scratch::new();
        let steam = scratch.0.join("Steam");
        let secondary = scratch.0.join("Rally library");
        scratch.write("Steam/steam.exe", "");
        scratch.write("Steam/steamapps/libraryfolders.vdf",&format!("\"libraryfolders\" {{ \"0\" {{ \"path\" \"{}\" }} \"1\" {{ \"path\" \"{}\" }} \"metadata\" \"ignore\" \"2\" {{\"path\" \"relative\"}} }}",vdf_path(&steam),vdf_path(&secondary)));
        scratch.write(
            "Steam/steamapps/appmanifest_223750.acf",
            "\"AppState\" {\"appid\" \"223750\" \"installdir\" \"DCSWorld\"}",
        );
        scratch.write("Steam/steamapps/common/DCSWorld/bin/DCS.exe", "");
        scratch.write("Steam/steamapps/common/DCSWorld/bin-mt/DCS.exe", "");
        scratch.write(
            "Rally library/steamapps/appmanifest_3917090.acf",
            "AppState { appid 3917090 installdir \"Assetto Corsa Rally\" }",
        );
        scratch.write(
            "Rally library/steamapps/common/Assetto Corsa Rally/acr/Binaries/Win64/acr.exe",
            "",
        );
        scratch.write(
            "Rally library/steamapps/common/Assetto Corsa Rally/acr.exe",
            "",
        );
        let roots = Roots {
            steam: vec![steam.clone()],
            launch_supported: true,
            ..Roots::default()
        };
        let games = discover(&roots, &BTreeMap::new());
        assert_eq!(games.len(), 7);
        assert_eq!(games[0]["source"], "steam");
        assert_eq!(games[1]["source"], "steam");
        assert_eq!(games[1]["provider"], "assetto-corsa-rally");
        assert_eq!(games[0]["processExecutables"].as_array().unwrap().len(), 2);
        for game in &games[..2] {
            assert_eq!(game["installed"], true);
            assert_eq!(game["launchSupported"], true);
        }
        let request = launch_request(&games[1]).unwrap();
        assert_eq!(request.executable, steam.join("steam.exe"));
        assert_eq!(request.arguments, ["-applaunch", "3917090"]);
        assert_eq!(process_paths(&games[0]).unwrap().len(), 2);
        let rally_paths = process_paths(&games[1]).unwrap();
        assert_eq!(rally_paths.len(), 1);
        assert!(Path::new(&rally_paths[0]).ends_with("acr/Binaries/Win64/acr.exe"));
        let manual = BTreeMap::from([(
            "dcs".into(),
            games[0]["executable"].as_str().unwrap().into(),
        )]);
        let manual_game = discover(&roots, &manual)[0].clone();
        assert_eq!(manual_game["source"], "manual");
        assert_eq!(
            launch_request(&manual_game).unwrap().arguments,
            ["-applaunch", "223750"]
        );
    }
    #[test]
    fn legacy_library_file_and_manifest_validation_prevent_false_discovery() {
        let scratch = Scratch::new();
        let steam = scratch.0.join("Steam");
        let library = scratch.0.join("Library");
        scratch.write(
            "Steam/config/libraryfolders.vdf",
            &format!(
                "LibraryFolders {{ 0 \"{}\" 1 \"{}\" 3 relative }}",
                vdf_path(&library),
                vdf_path(&library)
            ),
        );
        assert_eq!(library_paths(&steam), vec![steam.clone(), library.clone()]);
        for manifest in [
            "AppState { appid 223750 installdir wrong }",
            "AppState { appid nope installdir Rally }",
            "AppState { appid 3917090 installdir \"../Rally\" }",
            "AppState { appid 3917090 installdir .. }",
            "AppState { appid 3917090 installdir \"\" }",
            "AppState { appid 3917090 installdir \"C:\\Rally\" }",
        ] {
            scratch.write("Library/steamapps/appmanifest_3917090.acf", manifest);
            assert!(steam_installation(&GAMES[1], &library).is_none());
        }
        scratch.write(
            "Library/steamapps/appmanifest_3917090.acf",
            "AppState { appid 3917090 installdir Rally }",
        );
        let games = discover(
            &Roots {
                steam: vec![steam],
                ..Roots::default()
            },
            &BTreeMap::new(),
        );
        assert_eq!(games[1]["installed"], false);
    }
    #[test]
    fn standalone_manual_roots_executables_and_missing_paths() {
        let scratch = Scratch::new();
        let executable = scratch.write("DCS/bin-mt/DCS.exe", "");
        let root = scratch.0.join("DCS");
        let roots = Roots {
            dcs: vec![root.clone()],
            launch_supported: true,
            ..Roots::default()
        };
        let standalone = discover(&roots, &BTreeMap::new());
        assert_eq!(standalone[0]["source"], "standalone");
        assert_eq!(standalone[0]["launchSupported"], true);
        let request = launch_request(&standalone[0]).unwrap();
        assert_eq!(request.executable, executable);
        assert_eq!(request.directory, root);
        assert!(request.arguments.is_empty());
        for selected in [&root, &executable] {
            let manual = BTreeMap::from([("dcs".into(), selected.display().to_string())]);
            let games = discover(&roots, &manual);
            assert_eq!(games[0]["source"], "manual");
            assert_eq!(games[0]["installPath"], root.display().to_string());
        }
        let manual = BTreeMap::from([(
            "dcs".into(),
            scratch.0.join("missing").display().to_string(),
        )]);
        assert_eq!(discover(&roots, &manual)[0]["installed"], false);
        let manual = BTreeMap::from([("dcs".into(), " ".into())]);
        assert_eq!(discover(&roots, &manual)[0]["source"], "standalone");
        assert!(installation(&GAMES[0], Path::new("relative")).is_none());
        assert!(validated_executable(&GAMES[0], &scratch.write("wrong.exe", "")).is_err());
        let custom = scratch.write("custom/DCS.exe", "");
        assert_eq!(
            installation(&GAMES[0], &custom).unwrap().2,
            scratch.0.join("custom")
        );
    }
    #[test]
    fn rally_manual_detection_requires_steam_and_launch_data_is_revalidated() {
        let scratch = Scratch::new();
        let executable = scratch.write("Rally/acr/Binaries/Win64/acr.exe", "");
        let manual = BTreeMap::from([(
            "assetto-corsa-rally".into(),
            executable.display().to_string(),
        )]);
        let roots = Roots {
            launch_supported: true,
            ..Roots::default()
        };
        let mut game = discover(&roots, &manual)[1].clone();
        assert_eq!(game["installed"], true);
        assert_eq!(game["launchSupported"], false);
        assert_eq!(
            game["installPath"],
            scratch.0.join("Rally").display().to_string()
        );
        assert!(launch_request(&game).is_err());
        game["steamExecutable"] = json!(scratch.write("Steam/steam.exe", "").display().to_string());
        assert!(launch_request(&game).is_ok());
        game["steamAppId"] = json!(223750);
        assert!(launch_request(&game).is_err());
        game["steamAppId"] = Value::Null;
        assert!(launch_request(&game).is_err());
        game["steamAppId"] = json!(3917090);
        game["steamExecutable"] = json!(scratch.write("Steam/wrong.exe", "").display().to_string());
        assert!(launch_request(&game).is_err());
        game["installed"] = json!(false);
        assert!(launch_request(&game).is_err());
        assert!(launch_request(&json!({"id":"unknown"})).is_err());
        assert!(launch_request(&json!({"id":"dcs"})).is_err());
        assert!(process_paths(&json!({"id":"dcs"})).is_err());
        assert!(process_paths(&json!({"id":"dcs","executable":"missing"})).is_err());
        let root_executable = scratch.write("RallyRoot/acr.exe", "");
        assert_eq!(
            installation(&GAMES[1], &root_executable).unwrap().2,
            scratch.0.join("RallyRoot")
        );
        assert_eq!(
            process_paths(&json!({"id":"assetto-corsa-rally","executable":root_executable}))
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn startup_wait_handles_delayed_handoff_timeout_and_inspection_error() {
        let mut attempts = 0;
        let found = wait_for_process(
            Duration::from_secs(1),
            || {
                attempts += 1;
                Ok(if attempts == 3 { Some(process()) } else { None })
            },
            |_| {},
        )
        .unwrap();
        assert_eq!(found, process());
        assert_eq!(attempts, 3);
        assert!(wait_for_process(Duration::ZERO, || Ok(None), |_| {})
            .unwrap_err()
            .contains("timeout"));
        assert_eq!(
            wait_for_process(Duration::ZERO, || Err("inspection failed".into()), |_| {})
                .unwrap_err(),
            "inspection failed"
        );
    }
    #[test]
    #[cfg(not(windows))]
    fn non_windows_catalogue_supports_manual_detection_but_refuses_live_launch() {
        let platform = NativePlatform;
        let scratch = Scratch::new();
        let executable = scratch.write("DCS/bin/DCS.exe", "");
        platform
            .validate("dcs", executable.to_str().unwrap())
            .unwrap();
        assert!(platform
            .validate("unknown", executable.to_str().unwrap())
            .is_err());
        let manual = BTreeMap::from([("dcs".into(), executable.display().to_string())]);
        let catalogue = platform.catalogue(&manual);
        assert_eq!(catalogue[0]["installed"], true);
        assert_eq!(catalogue[0]["launchSupported"], false);
        assert!(platform
            .launch(&catalogue[0])
            .unwrap_err()
            .contains("Windows"));
        assert!(platform
            .find(&catalogue[0])
            .unwrap_err()
            .contains("Windows"));
        assert!(platform.alive(&process()).unwrap_err().contains("Windows"));
        assert!(native::spawn(&launch_request(&catalogue[0]).unwrap()).is_err());
    }
    #[test]
    fn assetto_titles_discover_launch_and_monitor_their_own_native_runtime() {
        let scratch = Scratch::new();
        let steam = scratch.write("Steam/steam.exe", "").parent().unwrap().to_path_buf();
        let titles = [
            ("assetto-corsa", 244210, "assettocorsa", "acs.exe"),
            ("assetto-corsa-competizione", 805550, "Assetto Corsa Competizione", "AC2/Binaries/Win64/AC2-Win64-Shipping.exe"),
            ("assetto-corsa-evo", 3058630, "Assetto Corsa EVO", "AssettoCorsaEVO.exe"),
        ];
        for (_, app, folder, runtime) in titles {
            scratch.write(&format!("Steam/steamapps/appmanifest_{app}.acf"),
                &format!("AppState {{ appid {app} installdir \"{folder}\" }}"));
            scratch.write(&format!("Steam/steamapps/common/{folder}/{runtime}"), "");
        }
        // ACC's bootstrap can remain alongside the long-lived simulator process.
        scratch.write("Steam/steamapps/common/Assetto Corsa Competizione/AC2.exe", "");
        scratch.write("Steam/steamapps/common/assettocorsa/AssettoCorsa.exe", "");
        let roots = Roots { steam: vec![steam.clone()], launch_supported: true, ..Roots::default() };
        let games = discover(&roots, &BTreeMap::new());
        for (id, app, folder, runtime) in titles {
            let game = games.iter().find(|game| game["id"] == id).unwrap();
            assert_eq!(game["provider"], id);
            assert_eq!(game["installed"], true);
            let launch = launch_request(game).unwrap();
            assert_eq!(launch.executable, steam.join("steam.exe"));
            assert_eq!(launch.arguments, vec!["-applaunch".to_owned(), app.to_string()]);
            let process = process_paths(game).unwrap();
            assert_eq!(process.len(), 1);
            assert!(Path::new(&process[0]).ends_with(runtime));
            let manual_path = scratch.0.join(format!("Steam/steamapps/common/{folder}/{runtime}"));
            let manual = discover(&roots, &BTreeMap::from([(id.into(), manual_path.display().to_string())]));
            let selected = manual.iter().find(|game|game["id"] == id).unwrap();
            assert_eq!(selected["source"], "manual");
            assert_eq!(selected["installPath"], steam.join("steamapps/common").join(folder).display().to_string());
            assert!(NativePlatform.validate(id, manual_path.to_str().unwrap()).is_ok());
            for (other, _, _, _) in titles { if other != id { assert!(NativePlatform.validate(other, manual_path.to_str().unwrap()).is_err()); } }
        }
    }

    #[test]
    fn assetto_corsa_menu_completes_launch_without_becoming_a_telemetry_producer() {
        let scratch = Scratch::new();
        let runtime = scratch.write("AC/acs.exe", "");
        let launcher = scratch.write("AC/AssettoCorsa.exe", "");
        let game = json!({"id":"assetto-corsa","executable":runtime});
        let menu = ProcessIdentity {pid:42, epoch:1, path:launcher.display().to_string()};
        let mut scans = 0;
        let found = find_game_process(&game, false, |paths| { scans += 1; Ok((paths == [launcher.display().to_string()]).then(||menu.clone())) }).unwrap();
        assert_eq!(found, Some(menu));
        assert_eq!(scans, 2);
        let mut scans = 0;
        assert!(find_game_process(&game, true, |_| { scans += 1; Ok(None) }).unwrap().is_none());
        assert_eq!(scans, 1);
        let driving = ProcessIdentity {pid:43, epoch:2, path:runtime.display().to_string()};
        let mut scans = 0;
        assert_eq!(find_game_process(&game, false, |_| { scans += 1; Ok(Some(driving.clone())) }).unwrap(), Some(driving));
        assert_eq!(scans, 1);
    }

}
