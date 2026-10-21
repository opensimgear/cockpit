//! Read-only discovery of locally installed client libraries. No account tokens,
//! network requests, client configuration writes or automatic imports occur.
use crate::game_discovery::{self, Vdf};
use crate::game_installations::LibrarySource;
use serde::Serialize;
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_CANDIDATES: usize = 512;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_SCAN_BYTES: usize = 2 * 1024 * 1024;
const MAX_DIAGNOSTICS: usize = 64;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Candidate {
    pub id: String,
    pub client: String,
    pub name: String,
    pub install_path: String,
    pub executable_candidates: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_id: Option<String>,
    pub kind: String,
    pub app_id: String,
    #[serde(skip)]
    pub launcher: Option<String>,
    #[serde(skip)]
    pub artwork: Option<PathBuf>,
}
impl Candidate {
    pub(crate) fn source(&self) -> LibrarySource {
        LibrarySource {
            client: self.client.clone(),
            app_id: self.app_id.clone(),
            install_path: self.install_path.clone(),
            launcher: self.launcher.clone(),
        }
    }
}
#[derive(Default)]
pub(crate) struct Scan {
    pub candidates: Vec<Candidate>,
    pub diagnostics: Vec<String>,
}
impl Scan {
    fn diagnostic(&mut self, message: String) {
        if self.diagnostics.len() < MAX_DIAGNOSTICS {
            self.diagnostics.push(message.chars().take(512).collect());
        } else if let Some(last) = self.diagnostics.last_mut() {
            *last = "Additional library scan diagnostics were omitted.".into();
        }
    }
}
fn bound_metadata(scan: &mut Scan) {
    // Reserve the exact encoded diagnostics plus a bounded truncation notice.
    // Paths are encoded only once, so escaping/Unicode count toward the limit.
    let mut bytes =
        serde_json::to_vec(&scan.diagnostics).map_or(MAX_SCAN_BYTES, |value| value.len()) + 256;
    let before = scan.candidates.len();
    scan.candidates.retain(|candidate| {
        bytes = bytes.saturating_add(
            serde_json::to_vec(candidate).map_or(MAX_SCAN_BYTES, |value| value.len()) + 1,
        );
        bytes <= MAX_SCAN_BYTES
    });
    if scan.candidates.len() != before {
        if scan.diagnostics.len() >= MAX_DIAGNOSTICS {
            scan.diagnostics.pop();
        }
        scan.diagnostic("Library results reached the native response limit; some results or diagnostics were omitted. Add remaining games manually.".into());
    }
}

fn regular(path: &Path, directory: bool) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    !metadata.file_type().is_symlink()
        && if directory {
            metadata.is_dir()
        } else {
            metadata.is_file()
        }
}
fn sorted_children(path: &Path) -> Vec<PathBuf> {
    let mut children = fs::read_dir(path)
        .into_iter()
        .flatten()
        .take(8192)
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    children.sort();
    children
}
fn clean_name(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.chars().count() <= 80 && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}
fn canonical_text(path: &Path) -> Option<String> {
    fs::canonicalize(path)
        .ok()?
        .to_str()
        .filter(|path| path.len() <= crate::game_installations::MAX_EXECUTABLE_BYTES)
        .map(str::to_owned)
}
fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && !matches!(value, "." | "..")
        && !value.contains(['/', '\\', ':', '\0'])
}
fn unique_vdf<'a>(value: &'a Vdf, key: &str) -> Option<&'a Vdf> {
    let Vdf::Object(entries) = value else {
        return None;
    };
    let mut matches = entries
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case(key));
    let first = &matches.next()?.1;
    matches.next().is_none().then_some(first)
}

/// Suggestions only: a directory with several executables needs an explicit
/// runtime choice. Installers and crash handlers are never auto-selected.
fn executable_candidates(root: &Path, preferred: Option<&Path>) -> Vec<String> {
    let mut found = Vec::new();
    if let Some(path) = preferred.filter(|path| regular(path, false)) {
        if let Ok(path) = game_discovery::validate_custom_executable(&path.to_string_lossy()) {
            if let Some(path) = path.to_str() {
                found.push(path.to_owned());
            }
        }
    }
    let mut pending = vec![(root.to_owned(), 0)];
    let mut inspected = 0;
    while let Some((directory, depth)) = pending.pop() {
        for path in sorted_children(&directory) {
            inspected += 1;
            if inspected > 8192 || found.len() >= 32 {
                return found;
            }
            let filename = path
                .file_name()
                .map(|name| name.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if depth < 5
                && regular(&path, true)
                && !matches!(
                    filename.as_str(),
                    "_commonredist" | "directx" | "redist" | "redistributables" | ".egstore"
                )
            {
                pending.push((path, depth + 1));
                continue;
            }
            if !regular(&path, false)
                || !filename.ends_with(".exe")
                || [
                    "unins",
                    "setup",
                    "install",
                    "crash",
                    "vcredist",
                    "dxsetup",
                    "ue4prereq",
                    "unitycrash",
                ]
                .iter()
                .any(|excluded| filename.contains(excluded))
            {
                continue;
            }
            if let Some(path) = canonical_text(&path) {
                if !found.contains(&path) {
                    found.push(path);
                }
            }
        }
    }
    found
}
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
fn steam_candidates(roots: &[PathBuf], scan: &mut Scan) {
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
                    game_id: game_discovery::builtin_for_steam(app_id).map(str::to_owned),
                    kind: kind.into(),
                    app_id: app_id.to_string(),
                    launcher: launcher.clone(),
                    artwork: steam_art(root, app_id),
                });
            }
        }
    }
}
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
fn epic_kind(manifest: &serde_json::Value, app_id: &str) -> Option<&'static str> {
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
fn epic_candidates(roots: &[PathBuf], scan: &mut Scan) {
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
            scan.candidates.push(Candidate {
                id: format!("epic:{app_id}"),
                client: "epic".into(),
                name,
                install_path,
                executable_candidates: executable_candidates(&install, preferred.as_deref()),
                game_id: None,
                kind: kind.into(),
                app_id: app_id.into(),
                launcher: None,
                artwork: None,
            });
        }
    }
}
pub(crate) fn scan() -> Scan {
    let epic = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .map(|root| root.join("Epic/EpicGamesLauncher/Data/Manifests"))
        .into_iter()
        .collect::<Vec<_>>();
    scan_roots(&game_discovery::library_steam_roots(), &epic)
}
fn scan_roots(steam: &[PathBuf], epic: &[PathBuf]) -> Scan {
    let mut scan = Scan::default();
    steam_candidates(steam, &mut scan);
    epic_candidates(epic, &mut scan);
    scan.candidates.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    scan.diagnostic("Steam and Epic show installed games only. Steam manifests do not reliably distinguish games from tools; review each selection.".into());
    scan.diagnostic("Epic imports use the selected runtime executable; games that require client authentication may need to be started in Epic first.".into());
    scan.diagnostic("GOG, EA, Ubisoft, Battle.net and Xbox libraries are not scanned yet. Add their game's runtime executable manually.".into());
    bound_metadata(&mut scan);
    scan
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_executable_lists_and_many_diagnostics_share_one_serialized_budget() {
        let mut scan = Scan::default();
        for index in 0..96 {
            scan.candidates.push(Candidate {
                id: format!("steam:{index}"),
                client: "steam".into(),
                name: format!("Game {index}"),
                install_path: "C:\\Games".into(),
                executable_candidates: (0..16)
                    .map(|file| format!("C:\\{}\\Game{file}.exe", "x".repeat(3000)))
                    .collect(),
                game_id: None,
                kind: "unknown".into(),
                app_id: index.to_string(),
                launcher: None,
                artwork: None,
            });
        }
        for _ in 0..1000 {
            scan.diagnostic("Malformed manifest ".repeat(1000));
        }
        bound_metadata(&mut scan);
        assert!(!scan.candidates.is_empty());
        assert!(scan.candidates.len() < 96);
        assert_eq!(scan.diagnostics.len(), MAX_DIAGNOSTICS);
        assert!(scan
            .diagnostics
            .iter()
            .any(|message| message.contains("omitted")));
        assert!(scan
            .diagnostics
            .iter()
            .any(|message| message.contains("native response limit")));
        let response = serde_json::json!({"libraryCandidates":scan.candidates,"libraryDiagnostics":scan.diagnostics});
        assert!(serde_json::to_vec(&response).unwrap().len() <= MAX_SCAN_BYTES);
    }
    #[test]
    fn steam_multiple_libraries_reuse_builtin_ids_and_discover_nested_art_without_importing() {
        let directory = crate::test_directory();
        let root = directory.path().join("Steam");
        let library = directory.path().join("Second");
        fs::create_dir_all(root.join("steamapps")).unwrap();
        fs::create_dir_all(library.join("steamapps/common/Racer/bin")).unwrap();
        fs::create_dir_all(root.join("appcache/librarycache/42")).unwrap();
        fs::write(root.join("steam.exe"), b"fixture").unwrap();
        fs::write(
            root.join("steamapps/libraryfolders.vdf"),
            format!(
                "\"libraryfolders\" {{ \"1\" {{ \"path\" \"{}\" }} }}",
                library.display()
            ),
        )
        .unwrap();
        fs::write(
            library.join("steamapps/appmanifest_42.acf"),
            b"\"AppState\" {\"appid\" \"42\" \"name\" \"Racer\" \"installdir\" \"Racer\"}",
        )
        .unwrap();
        fs::write(
            library.join("steamapps/common/Racer/bin/Racer.exe"),
            b"fixture",
        )
        .unwrap();
        fs::write(
            root.join("appcache/librarycache/42/hashed_header.jpg"),
            b"image",
        )
        .unwrap();
        fs::create_dir_all(root.join("steamapps/common/DCS")).unwrap();
        fs::write(
            root.join("steamapps/appmanifest_223750.acf"),
            b"\"AppState\" {\"appid\" \"223750\" \"name\" \"DCS\" \"installdir\" \"DCS\"}",
        )
        .unwrap();
        let scan = scan_roots(&[root], &[]);
        assert_eq!(scan.candidates.len(), 2);
        let racer = scan
            .candidates
            .iter()
            .find(|game| game.app_id == "42")
            .unwrap();
        assert_eq!(racer.executable_candidates.len(), 1);
        assert!(racer.artwork.is_some());
        assert_eq!(
            scan.candidates
                .iter()
                .find(|game| game.app_id == "223750")
                .unwrap()
                .game_id
                .as_deref(),
            Some("dcs")
        );
    }
    #[test]
    fn epic_uses_manifest_runtime_and_skips_incomplete_and_traversal_installs() {
        let directory = crate::test_directory();
        let manifests = directory.path().join("manifests");
        let game = directory.path().join("game");
        fs::create_dir_all(&manifests).unwrap();
        fs::create_dir_all(game.join("Binaries")).unwrap();
        fs::write(game.join("Binaries/Game.exe"), b"exe").unwrap();
        let mut value = serde_json::json!({"AppName":"EpicGame","DisplayName":"Epic game","InstallLocation":game,"LaunchExecutable":"Binaries/Game.exe"});
        fs::write(
            manifests.join("game.item"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        value["AppName"] = serde_json::json!("Incomplete");
        value["bIsIncompleteInstall"] = serde_json::json!(true);
        fs::write(
            manifests.join("incomplete.item"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        let scan = scan_roots(&[], &[manifests]);
        assert_eq!(scan.candidates.len(), 1);
        assert_eq!(scan.candidates[0].client, "epic");
        assert!(scan.candidates[0].executable_candidates[0].ends_with("Game.exe"));
    }
    #[test]
    fn epic_engine_plugins_and_dlc_are_not_game_candidates() {
        for value in [
            serde_json::json!({"bIsApplication":false,"AppCategories":["engines/ue5","engines"],"CatalogNamespace":"ue"}),
            serde_json::json!({"bIsApplication":false,"AppCategories":["plugins/engine","plugins"]}),
            serde_json::json!({"AppCategories":["dlc"],"MainGameAppName":"BaseGame"}),
            serde_json::json!({"MainGameAppName":"BaseGame"}),
        ] {
            assert_eq!(epic_kind(&value, "App"), None);
        }
        assert_eq!(
            epic_kind(
                &serde_json::json!({"bIsApplication":true,"AppCategories":["games"],"MainGameAppName":"App"}),
                "App"
            ),
            Some("game")
        );
    }
    #[test]
    fn steam_rejects_ambiguous_manifest_identities_and_path_traversal() {
        let directory = crate::test_directory();
        let root = directory.path();
        fs::create_dir_all(root.join("steamapps/common/Game")).unwrap();
        for (filename, manifest) in [
            (
                "appmanifest_1.acf",
                r#""AppState" {"appid" "2" "installdir" "Game"}"#,
            ),
            (
                "appmanifest_3.acf",
                r#""AppState" {"appid" "3" "appid" "4" "installdir" "Game"}"#,
            ),
            (
                "appmanifest_5.acf",
                r#""AppState" {"appid" "5" "installdir" "../Game"}"#,
            ),
        ] {
            fs::write(root.join("steamapps").join(filename), manifest).unwrap();
        }
        let scan = scan_roots(&[root.to_owned()], &[]);
        assert!(scan.candidates.is_empty());
        assert!(scan
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("identity does not match")));
    }
}
