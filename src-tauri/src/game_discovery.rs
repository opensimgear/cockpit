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
    fn process_monitoring_supported(&self) -> bool {
        false
    }
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
    fn find_many(&self, games: &[Value]) -> Vec<Result<Option<ProcessIdentity>, String>> {
        games.iter().map(|game| self.find(game)).collect()
    }
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
struct ProcessInventory {
    processes: Vec<(ProcessIdentity, u32)>,
    // When a protected matching process cannot expose its full path, only
    // games watching that basename lose proof; unrelated games stay usable.
    errors: BTreeMap<String, String>,
}
impl ProcessInventory {
    fn inspect(
        &self,
        paths: &[String],
        exclusive: bool,
    ) -> Result<Option<ProcessIdentity>, String> {
        for path in paths {
            let basename = path.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase();
            if let Some(error) = self.errors.get(&basename) {
                return Err(error.clone());
            }
        }
        let candidates = self
            .processes
            .iter()
            .filter(|(process, _)| {
                exclusive || paths.iter().any(|path| paths_match(path, &process.path))
            })
            .cloned()
            .collect::<Vec<_>>();
        selected_process(&candidates, exclusive)
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

struct IntegrationDefinition {
    id: &'static str,
    name: &'static str,
    provider: &'static str,
    steam_id: u32,
    executables: &'static [&'static str],
}
// Adapter recognition metadata, independent of the open-ended saved game registry.
const INTEGRATIONS: &[IntegrationDefinition] = &[
    IntegrationDefinition {
        id: "dcs",
        name: "DCS World",
        provider: "dcs-bios",
        steam_id: 223750,
        executables: &["bin/DCS.exe", "bin-mt/DCS.exe"],
    },
    IntegrationDefinition {
        id: "assetto-corsa-rally",
        name: "Assetto Corsa Rally",
        provider: "assetto-corsa-rally",
        steam_id: 3917090,
        // The Unreal runtime is preferred over a root bootstrap executable.
        executables: &["acr/Binaries/Win64/acr.exe", "acr.exe"],
    },
    IntegrationDefinition {
        id: "x-plane-11",
        name: "X-Plane 11",
        provider: "x-plane",
        steam_id: 269950,
        executables: &["X-Plane.exe"],
    },
    IntegrationDefinition {
        id: "x-plane-12",
        name: "X-Plane 12",
        provider: "x-plane",
        steam_id: 2014780,
        executables: &["X-Plane.exe"],
    },
    IntegrationDefinition {
        id: "assetto-corsa",
        name: "Assetto Corsa",
        provider: "assetto-corsa",
        steam_id: 244210,
        // The launcher does not publish driving telemetry; watch the simulator.
        executables: &["acs.exe", "acs_x86.exe"],
    },
    IntegrationDefinition {
        id: "assetto-corsa-competizione",
        name: "Assetto Corsa Competizione",
        provider: "assetto-corsa-competizione",
        steam_id: 805550,
        executables: &["AC2/Binaries/Win64/AC2-Win64-Shipping.exe", "AC2.exe"],
    },
    IntegrationDefinition {
        id: "assetto-corsa-evo",
        name: "Assetto Corsa EVO",
        provider: "assetto-corsa-evo",
        steam_id: 3058630,
        executables: &["AssettoCorsaEVO.exe"],
    },
];
fn integration_definition(id: &str) -> Result<&'static IntegrationDefinition, String> {
    INTEGRATIONS
        .iter()
        .find(|g| g.id == id)
        .ok_or_else(|| "Choose an available telemetry integration.".into())
}

pub(crate) fn executable_picker(game_id: &str) -> Result<(String, &'static str), String> {
    if !valid_id(game_id) {
        return Err("Choose a valid game identity.".into());
    }
    Ok(("Choose the game's runtime executable".into(), "Game.exe"))
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
pub(crate) fn normalized_path(path: &str) -> String {
    path.replace('/', "\\")
        .trim_start_matches("\\\\?\\")
        .to_lowercase()
}
pub(crate) fn validate_executable(path: &str) -> Result<PathBuf, String> {
    let path = Path::new(path);
    if !crate::game_installations::executable(&path.to_string_lossy())
        || !path.is_file()
        || !path
            .extension()
            .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("exe"))
    {
        return Err("Choose an existing absolute game executable (.exe).".into());
    }
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("Cannot access game executable: {error}"))?;
    if canonical.to_str().is_none() {
        return Err("Game executable path is not valid text.".into());
    }
    Ok(canonical)
}
pub(crate) fn library_steam_roots() -> Vec<PathBuf> {
    native::roots().steam
}
pub(crate) fn recognized_for_steam(app_id: u32) -> Option<&'static str> {
    INTEGRATIONS
        .iter()
        .find(|game| game.steam_id == app_id)
        .map(|game| game.id)
}

include!("games/discovery/catalogue.rs");
include!("games/discovery/manifests.rs");
include!("games/discovery/launch.rs");
include!("games/discovery/unsupported.rs");
include!("games/discovery/windows.rs");
include!("games/discovery/tests.rs");
