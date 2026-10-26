use super::*;

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_MANIFEST_BYTES)
        .then(|| serde_json::from_slice(&bytes).ok())
        .flatten()
}
pub(super) fn epic_kind(manifest: &serde_json::Value, app_id: &str) -> Option<&'static str> {
    let categories = manifest["AppCategories"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    if manifest["bIsApplication"] == false
        || manifest["CatalogNamespace"] == "ue"
        || manifest["MainGameAppName"]
            .as_str()
            .is_some_and(|main| !main.is_empty() && main != app_id)
        || categories.iter().any(|category| {
            ["engines", "plugins", "dlc", "mods"]
                .iter()
                .any(|kind| category == kind || category.starts_with(&format!("{kind}/")))
        })
    {
        return None;
    }
    Some(
        if categories
            .iter()
            .any(|category| category == "games" || category.starts_with("games/"))
        {
            "game"
        } else {
            "unknown"
        },
    )
}
pub(super) fn epic_candidates(roots: &[PathBuf], scan: &mut Scan) {
    let mut seen = BTreeSet::new();
    for root in roots.iter().take(8) {
        for path in sorted_children(root) {
            if scan.candidates.len() >= MAX_CANDIDATES {
                return;
            }
            if !regular(&path, false)
                || path.extension().and_then(|ext| ext.to_str()) != Some("item")
            {
                continue;
            }
            let Some(manifest) = read_json(&path) else {
                scan.diagnostic(format!(
                    "Epic manifest could not be read: {}",
                    path.display()
                ));
                continue;
            };
            if manifest["bIsIncompleteInstall"] == true {
                continue;
            }
            let Some(app_id) = manifest["AppName"].as_str().filter(|id| {
                !id.is_empty()
                    && id.len() <= 128
                    && id
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
            }) else {
                continue;
            };
            let Some(kind) = epic_kind(&manifest, app_id) else {
                continue;
            };
            let Some(name) = manifest["DisplayName"].as_str().and_then(clean_name) else {
                continue;
            };
            let Some(install) = manifest["InstallLocation"]
                .as_str()
                .map(PathBuf::from)
                .filter(|path| path.is_absolute() && regular(path, true))
            else {
                continue;
            };
            let Some(install_path) = canonical_text(&install) else {
                continue;
            };
            if !seen.insert(app_id.to_owned()) {
                continue;
            }
            let preferred = manifest["LaunchExecutable"]
                .as_str()
                .map(Path::new)
                .filter(|path| {
                    !path.is_absolute()
                        && !path
                            .components()
                            .any(|part| matches!(part, std::path::Component::ParentDir))
                })
                .map(|path| install.join(path));
            let executables = executable_candidates(&install, preferred.as_deref());
            let preferred_executable = preferred
                .as_deref()
                .and_then(canonical_text)
                .filter(|path| executables.contains(path));
            scan.candidates.push(Candidate {
                id: format!("epic:{app_id}"),
                client: "epic".into(),
                name,
                install_path,
                executable_candidates: executables,
                preferred_executable,
                game_id: None,
                kind: kind.into(),
                app_id: app_id.into(),
                launcher: None,
                artwork: None,
            });
        }
    }
}
