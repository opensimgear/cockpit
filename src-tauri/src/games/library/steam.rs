use super::*;

fn steam_art(root: &Path, app_id: u32) -> Option<PathBuf> {
    let cache = root.join("appcache/librarycache");
    for relative in [
        format!("{app_id}_library_hero.jpg"),
        format!("{app_id}_header.jpg"),
        format!("{app_id}_library_600x900.jpg"),
        format!("{app_id}/library_hero.jpg"),
        format!("{app_id}/header.jpg"),
    ] {
        let path = cache.join(relative);
        if regular(&path, false) {
            return Some(path);
        }
    }
    // Recent Steam caches place content-hashed artwork inside an app-id folder.
    sorted_children(&cache.join(app_id.to_string()))
        .into_iter()
        .find(|path| {
            regular(path, false)
                && path.file_name().is_some_and(|name| {
                    let name = name.to_string_lossy().to_lowercase();
                    (name.contains("hero") || name.contains("header"))
                        && matches!(
                            path.extension().and_then(|ext| ext.to_str()),
                            Some("jpg" | "png" | "webp")
                        )
                })
        })
}
pub(super) fn steam_candidates(roots: &[PathBuf], scan: &mut Scan) {
    let mut seen = BTreeSet::new();
    for root in roots.iter().take(16) {
        let launcher = regular(&root.join("steam.exe"), false)
            .then(|| canonical_text(&root.join("steam.exe")))
            .flatten();
        for library in game_discovery::library_paths(root).into_iter().take(32) {
            for path in sorted_children(&library.join("steamapps")) {
                if scan.candidates.len() >= MAX_CANDIDATES {
                    scan.diagnostic("The local library scan reached its 512-entry limit.".into());
                    return;
                }
                if !regular(&path, false)
                    || !path.file_name().is_some_and(|name| {
                        let name = name.to_string_lossy();
                        name.starts_with("appmanifest_") && name.ends_with(".acf")
                    })
                {
                    continue;
                }
                let Some(manifest) = game_discovery::read_vdf(&path) else {
                    scan.diagnostic(format!(
                        "Steam manifest could not be read: {}",
                        path.display()
                    ));
                    continue;
                };
                let Some(state) = unique_vdf(&manifest, "AppState") else {
                    continue;
                };
                let Some(app_id) = unique_vdf(state, "appid")
                    .and_then(Vdf::text)
                    .and_then(|text| text.parse::<u32>().ok())
                    .filter(|id| *id > 0)
                else {
                    continue;
                };
                let filename_id = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(|name| name.strip_prefix("appmanifest_"))
                    .and_then(|name| name.strip_suffix(".acf"))
                    .and_then(|id| id.parse::<u32>().ok());
                if filename_id != Some(app_id) {
                    scan.diagnostic(format!(
                        "Steam manifest app identity does not match its filename: {}",
                        path.display()
                    ));
                    continue;
                }
                let Some(directory) = unique_vdf(state, "installdir")
                    .and_then(Vdf::text)
                    .filter(|value| safe_component(value))
                else {
                    continue;
                };
                let install = library.join("steamapps/common").join(directory);
                if !regular(&install, true) {
                    continue;
                }
                let Some(install_path) = canonical_text(&install) else {
                    continue;
                };
                if !seen.insert(app_id) {
                    continue;
                }
                let name = unique_vdf(state, "name")
                    .and_then(Vdf::text)
                    .and_then(clean_name)
                    .or_else(|| clean_name(directory))
                    .unwrap_or_else(|| format!("Steam game {app_id}"));
                let kind = if name.to_lowercase().contains("dedicated server")
                    || name.to_lowercase().contains("redistributable")
                    || name.to_lowercase().contains("steamworks")
                {
                    "tool"
                } else {
                    "unknown"
                };
                scan.candidates.push(Candidate {
                    id: format!("steam:{app_id}"),
                    client: "steam".into(),
                    name,
                    install_path,
                    executable_candidates: executable_candidates(&install, None),
                    preferred_executable: None,
                    game_id: game_discovery::recognized_for_steam(app_id).map(str::to_owned),
                    kind: kind.into(),
                    app_id: app_id.to_string(),
                    launcher: launcher.clone(),
                    artwork: steam_art(root, app_id),
                });
            }
        }
    }
}
