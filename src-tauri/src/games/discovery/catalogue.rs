pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 96
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
pub(crate) fn integration_provider(key: &str) -> Option<&'static str> {
    INTEGRATIONS
        .iter()
        .find(|game| game.id == key)
        .map(|game| game.provider)
}
pub(crate) fn integration_binding(game: &Value) -> Option<&str> {
    let key = game["integrationKey"].as_str()?;
    integration_provider(key)
        .filter(|provider| game["provider"].as_str() == Some(*provider))
        .map(|_| key)
}
/// Runtime authority additionally requires the selected executable to match.
pub(crate) fn integration_key(game: &Value) -> Option<&str> {
    (game["integrationCompatible"] == true)
        .then(|| integration_binding(game))
        .flatten()
}
pub(crate) fn recognize_installation(
    root: &Path,
    executables: &[String],
) -> Option<(String, String, String)> {
    INTEGRATIONS
        .iter()
        .find(|game| {
            installation(game, root).is_some()
                || executables
                    .iter()
                    .any(|path| validated_executable(game, Path::new(path)).is_ok())
        })
        .map(|game| (game.id.into(), game.name.into(), game.id.into()))
}
pub(crate) fn merged_catalogue(
    _platform: &dyn Platform,
    settings: &crate::game_installations::Settings,
) -> Vec<Value> {
    settings.games.iter().map(|(id, record)| {
        let selected = match record.selected_installation.as_deref() { Some(key)=>record.installations.get(key), None=>record.installations.values().next() };
        let executable = selected.and_then(|install|install.executable.as_deref()).and_then(|path|validate_executable(path).ok()).and_then(|path|path_string(&path));
        let capability = record.integration.as_deref().and_then(|key|integration_definition(key).ok());
        let spec = capability.filter(|spec|executable.as_deref().is_some_and(|path|validated_executable(spec,Path::new(path)).is_ok()));
        let integration = spec.map(|spec|spec.id);
        let source = selected.and_then(|install|install.sources.iter().find(|source|source.client == "steam")).or_else(||selected.and_then(|install|install.sources.first()));
        let steam = source.filter(|source|source.client == "steam" && spec.is_none_or(|spec|spec.provider != "x-plane"));
        let steam_ready = steam.and_then(|source|source.launcher.as_deref()).is_some_and(|path|Path::new(path).is_file());
        let required_steam = spec.is_some_and(|spec|spec.id.starts_with("assetto-corsa"));
        let paths = spec.and_then(|spec|executable.as_deref().and_then(|path|installation(spec,Path::new(path))).map(|(_,paths,_)|paths.into_iter().filter_map(|path|path_string(&path)).collect::<Vec<_>>())).unwrap_or_else(||executable.iter().cloned().collect::<Vec<_>>());
        let paths = preferred_process_paths(integration,paths,executable.as_deref());
        let installations = record.installations.iter().map(|(key,install)|json!({"id":key,"root":install.root,"installPath":install.root,"executable":install.executable,"executableCandidates":install.executables,"source":install.sources.first().map(|source|source.client.as_str()).unwrap_or("disk"),"sources":install.sources,"available":install.executable.as_deref().is_some_and(|path|validate_executable(path).is_ok())})).collect::<Vec<_>>();
        let launch = cfg!(windows) && executable.is_some() && (!required_steam || steam_ready) && (steam.is_none() || steam_ready);
        json!({"id":id,"name":record.name,"integrationKey":record.integration,"provider":capability.map(|spec|spec.provider),"integrationAvailable":capability.is_some(),"integrationCompatible":spec.is_some(),
            "installed":executable.is_some(),"executable":executable,"installPath":selected.map(|install|&install.root),"source":source.map(|source|source.client.as_str()).unwrap_or("disk"),"manualPath":executable,
            "installations":installations,"selectedInstallationId":record.selected_installation,"launchSupported":launch,"processMonitoringSupported":cfg!(windows),
            "launchUnavailableReason":if !cfg!(windows){Some("Game launch and process monitoring require Windows.")}else if executable.is_none(){Some("Choose an available game executable.")}else if !launch{Some("The installation's launcher is unavailable.")}else{None},
            "steamAppId":steam.and_then(|source|source.app_id.parse::<u32>().ok()),"steamExecutable":steam.and_then(|source|source.launcher.as_deref()),"processExecutables":paths,
            "categoryId":settings.game_categories.get(id),"background":settings.backgrounds.get(id),"defaultBackground":settings.default_backgrounds.get(id).or(record.default_background.as_ref())})
    }).collect()
}
fn preferred_process_paths(
    key: Option<&str>,
    mut paths: Vec<String>,
    selected: Option<&str>,
) -> Vec<String> {
    if paths.is_empty() {
        paths.extend(selected.map(str::to_owned));
    }
    if matches!(
        key,
        Some("assetto-corsa-rally" | "assetto-corsa-competizione")
    ) {
        if let Some(runtime) = paths
            .iter()
            .find(|path| normalized_path(path).contains("\\binaries\\win64\\"))
            .cloned()
        {
            return vec![runtime];
        }
    }
    paths
}
fn paths_match(left: &str, right: &str) -> bool {
    normalized_path(left) == normalized_path(right)
}
fn validated_executable(game: &IntegrationDefinition, path: &Path) -> Result<PathBuf, String> {
    let expected = game.executables[0].rsplit('/').next().unwrap();
    if !path.is_absolute()
        || !path.file_name().and_then(|p| p.to_str()).is_some_and(|p| {
            game.executables
                .iter()
                .any(|exe| p.eq_ignore_ascii_case(exe.rsplit('/').next().unwrap()))
        })
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
    game: &IntegrationDefinition,
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
