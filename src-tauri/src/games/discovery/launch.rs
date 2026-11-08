#[derive(Debug, PartialEq)]
struct LaunchRequest {
    executable: PathBuf,
    directory: PathBuf,
    arguments: Vec<String>,
}
fn launch_request(game: &Value) -> Result<LaunchRequest, String> {
    if !valid_id(game["id"].as_str().unwrap_or("")) || game["installed"] != true {
        return Err("Game installation was not found.".into());
    }
    let executable = validate_executable(
        game["executable"]
            .as_str()
            .ok_or("Choose a game executable before launching.")?,
    )?;
    let key = integration_key(game);
    if let Some(spec) = key.and_then(|key| integration_definition(key).ok()) {
        validated_executable(spec, &executable)?;
    }
    if let Some(app_id) = game["steamAppId"]
        .as_u64()
        .filter(|id| *id > 0 && *id <= u32::MAX as u64)
    {
        if let Some(spec) = key.and_then(|key| integration_definition(key).ok()) {
            if app_id != spec.steam_id as u64 || spec.provider == "x-plane" {
                return Err(
                    "Steam launch route does not match the selected integration installation."
                        .into(),
                );
            }
        }
        let steam = PathBuf::from(
            game["steamExecutable"]
                .as_str()
                .ok_or("Steam is unavailable; rescan its installed library.")?,
        );
        if !steam.is_absolute()
            || !steam.is_file()
            || !steam
                .file_name()
                .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("steam.exe"))
        {
            return Err("The Steam client executable is unavailable.".into());
        }
        return Ok(LaunchRequest {
            directory: steam
                .parent()
                .ok_or("Steam directory is unavailable.")?
                .to_owned(),
            executable: steam,
            arguments: vec!["-applaunch".into(), app_id.to_string()],
        });
    }
    if key.is_some_and(|key| key.starts_with("assetto-corsa")) {
        return Err("This Assetto installation must be launched through Steam.".into());
    }
    let directory = game["installPath"]
        .as_str()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_dir())
        .or_else(|| executable.parent().map(Path::to_path_buf))
        .ok_or("Game directory is unavailable.")?;
    Ok(LaunchRequest {
        executable,
        directory,
        arguments: vec![],
    })
}
fn process_paths(game: &Value) -> Result<Vec<String>, String> {
    if !valid_id(game["id"].as_str().unwrap_or("")) {
        return Err("Choose a valid game identity.".into());
    }
    let selected = game["executable"]
        .as_str()
        .ok_or("Choose the game's runtime executable.")?;
    let executable = validate_executable(selected)?;
    let paths = game["processExecutables"]
        .as_array()
        .map(|paths| {
            paths
                .iter()
                .map(|path| {
                    path.as_str()
                        .ok_or_else(|| "Invalid runtime executable.".to_owned())
                        .and_then(validate_executable)
                        .and_then(|path| {
                            path_string(&path).ok_or_else(|| "Invalid runtime path.".into())
                        })
                })
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(preferred_process_paths(
        integration_key(game),
        paths,
        path_string(&executable).as_deref(),
    ))
}
fn find_game_process(
    game: &Value,
    integration: bool,
    mut inspect: impl FnMut(&[String]) -> Result<Option<ProcessIdentity>, String>,
) -> Result<Option<ProcessIdentity>, String> {
    let runtime = inspect(&process_paths(game)?)?;
    if runtime.is_some() || integration || integration_key(game) != Some("assetto-corsa") {
        return Ok(runtime);
    }
    // Steam starts AC's persistent menu before a user chooses a car and track.
    // It counts as a launched game, but never as the shared-memory producer.
    let spec = integration_definition("assetto-corsa")?;
    let selected = game["executable"]
        .as_str()
        .ok_or("Choose an Assetto Corsa installation.")?;
    let (_, _, root) =
        installation(spec, Path::new(selected)).ok_or("The Assetto Corsa installation changed.")?;
    let launcher = root.join("AssettoCorsa.exe");
    if !launcher.is_file() {
        return Ok(None);
    }
    let launcher = fs::canonicalize(launcher)
        .map_err(|error| format!("Cannot inspect the Assetto Corsa launcher: {error}"))?;
    inspect(&[path_string(&launcher).ok_or("The Assetto Corsa launcher path is not valid text.")?])
}
/// Process inventory and event subscriptions share the same registered targets.
/// Include the persistent Corsa launcher without treating it as a telemetry source.
pub(crate) fn activation_process_paths(games: &[Value]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut paths = Vec::new();
    for game in games {
        let _ = find_game_process(game, false, |items| {
            for item in items {
                if seen.insert(normalized_path(item)) {
                    paths.push(item.clone());
                }
            }
            Ok(None)
        });
    }
    paths
}

fn selected_process(
    candidates: &[(ProcessIdentity, u32)],
    exclusive_xplane: bool,
) -> ProcessLookup {
    if candidates.len() <= 1 {
        return Ok(candidates.first().map(|(identity, _)| identity.clone()));
    }
    // The original AC menu can retain its bootstrap after starting a child of
    // the same executable. Only a single verified parent chain is a handoff;
    // independently launched copies, branches, and other runtimes stay ambiguous.
    let launcher_chain = !exclusive_xplane
        && candidates.iter().all(|(identity, _)| {
            identity
                .path
                .rsplit(['/', '\\'])
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case("AssettoCorsa.exe"))
                && paths_match(&identity.path, &candidates[0].0.path)
        });
    if launcher_chain {
        let leaves = candidates
            .iter()
            .filter(|(identity, _)| !candidates.iter().any(|(_, parent)| *parent == identity.pid))
            .collect::<Vec<_>>();
        if leaves.len() == 1 {
            let leaf = leaves[0];
            let mut current = leaf;
            let mut visited = BTreeSet::new();
            loop {
                if !visited.insert(current.0.pid) {
                    break;
                }
                match candidates
                    .iter()
                    .find(|(identity, _)| identity.pid == current.1)
                {
                    Some(parent) if parent.0.epoch <= current.0.epoch => current = parent,
                    Some(_) => break,
                    None if visited.len() == candidates.len() => return Ok(Some(leaf.0.clone())),
                    None => break,
                }
            }
        }
    }
    Err(ProcessLookupError::Ambiguous(if exclusive_xplane {
        "Multiple X-Plane processes are running. Close the other installations before using telemetry."
    } else {
        "Multiple game processes match the selected installation. Close the other instances before activating its profile."
    }.into()))
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
    fn process_monitoring_supported(&self) -> bool {
        cfg!(windows)
    }
    #[cfg(windows)]
    fn watch_activation_processes(
        &self,
        games: &[Value],
        wakes: [std::sync::Arc<osg_streams::NativeWake>; 2],
    ) -> Option<crate::xplane::process_events::InventoryEvents> {
        if games.len() > osg_profiles::activation::MAX_GAMES {
            return None;
        }
        let monitored: Vec<_> = games.iter()
            .filter(|game| crate::game_activation::process_candidate(game))
            .cloned().collect();
        if monitored.iter().any(|game| process_paths(game).is_err()) { return None; }
        crate::xplane::process_events::InventoryEvents::start(&activation_process_paths(&monitored), wakes)
            .ok().flatten()
    }
    fn integration_setup_supported(&self) -> bool {
        cfg!(windows)
    }
    fn background_processes(&self, games: &[Value]) -> Option<BackgroundProcesses> {
        cfg!(windows).then(|| capture_background_processes(games, native::inventory))
    }
    fn shares_background_processes(&self) -> bool {
        cfg!(windows)
    }
    fn catalogue(&self, manual: &BTreeMap<String, String>) -> Vec<Value> {
        let mut roots=native::roots();
        if !crate::game_library::installed_clients().iter().any(|(id,_)|*id=="steam") {roots.steam.clear();}
        discover(&roots, manual)
    }
    fn validate(&self, game_id: &str, executable: &str) -> Result<(), String> {
        if !valid_id(game_id) {
            return Err("Choose a valid native game identity.".into());
        }
        validate_executable(executable).map(|_| ())
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
    fn find_many(&self, games: &[Value]) -> Vec<Result<Option<ProcessIdentity>, String>> {
        let paths = activation_process_paths(games);
        let inventory = native::inventory(&paths);
        games
            .iter()
            .map(|game| match &inventory {
                Err(error) => Err(error.clone()),
                Ok(inventory) => {
                    find_game_process(game, false, |wanted| inventory.inspect(wanted, false).map_err(|error| error.to_string()))
                }
            })
            .collect()
    }
    fn integration_process(&self, game: &Value) -> Result<Option<ProcessIdentity>, String> {
        find_game_process(game, true, native::find)
    }
    fn integration_processes(&self, games: &[&Value]) -> Vec<ProcessLookup> {
        let paths: Vec<_> = games.iter().map(|game| process_paths(game)).collect();
        let mut seen = BTreeSet::new();
        let wanted = paths.iter().filter_map(|paths| paths.as_ref().ok()).flatten()
            .filter(|path| seen.insert(normalized_path(path))).cloned().collect::<Vec<_>>();
        let inventory = native::inventory(&wanted);
        paths.into_iter().map(|paths| {
            let paths = paths.map_err(ProcessLookupError::Unavailable)?;
            inventory.as_ref().map_err(|error| ProcessLookupError::Unavailable(error.clone()))?.inspect(&paths, false)
        }).collect()
    }
    fn alive(&self, identity: &ProcessIdentity) -> Result<bool, String> {
        native::alive(identity)
    }
}

fn capture_background_processes(
    games: &[Value],
    scan: impl FnOnce(&[String]) -> Result<ProcessInventory, String>,
) -> BackgroundProcesses {
    let mut rows = BackgroundProcesses { activation: BTreeMap::new(), integration: BTreeMap::new() };
    let activation_supported = games.len() <= osg_profiles::activation::MAX_GAMES;
    let monitored = games.iter().filter(|game| crate::game_activation::process_candidate(game)
        && (activation_supported || integration_key(game).is_some()))
        .cloned().collect::<Vec<_>>();
    let paths = activation_process_paths(&monitored);
    let inventory = if paths.is_empty() { Ok(ProcessInventory::default()) } else { scan(&paths) };
    let inspect = |paths: &[String]| {
        inventory.as_ref().map_err(|error| ProcessLookupError::Unavailable(error.clone()))?
            .inspect(paths, false)
    };
    for game in &monitored {
        let Some(id) = game["id"].as_str() else { continue; };
        if activation_supported {
            let activation = find_game_process(game, false, |paths| inspect(paths).map_err(|error| error.to_string()));
            rows.activation.insert(id.to_owned(), activation);
        }
        let integration = process_paths(game).map_err(ProcessLookupError::Unavailable)
            .and_then(|paths| inspect(&paths));
        rows.integration.insert(id.to_owned(), (process_selection(game), integration));
    }
    rows
}
