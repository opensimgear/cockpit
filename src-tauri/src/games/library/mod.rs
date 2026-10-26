//! Read-only discovery of locally installed client libraries. No account tokens,
//! network requests, client configuration writes or automatic imports occur.
use crate::game_discovery::{self, Vdf};
mod artwork;
mod artwork_choices;
mod battlenet;
mod disk;
mod epic;
pub(crate) mod executable_ranking;
mod platforms;
mod registry;
mod steam;
mod xbox;
use crate::game_installations::LibrarySource;
pub(crate) use platforms::installed_clients;
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

pub(crate) use artwork_choices::{ArtworkChoice, artwork_choices};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Candidate {
    pub id: String,
    pub client: String,
    pub name: String,
    pub install_path: String,
    pub executable_candidates: Vec<String>,
    #[serde(skip)]
    pub preferred_executable: Option<String>,
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

fn runtime_filename(path: &Path) -> bool {
    let filename = path
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    filename.ends_with(".exe")
        && ![
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
}

/// Suggestions only: a directory with several executables needs an explicit
/// runtime choice. Installers and crash handlers are never auto-selected.
fn executable_candidates(root: &Path, preferred: Option<&Path>) -> Vec<String> {
    let mut found = Vec::new();
    let canonical_root = fs::canonicalize(root).ok();
    if let Some(path) = preferred.filter(|path| regular(path, false) && runtime_filename(path)) {
        if let Ok(path) = game_discovery::validate_executable(&path.to_string_lossy()) {
            if !canonical_root
                .as_ref()
                .is_some_and(|root| path.starts_with(root))
            {
                return executable_candidates(root, None);
            }
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
            if !regular(&path, false) || !runtime_filename(&path) {
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
pub(crate) const CLIENTS: &[(&str, &str)] = &[
    ("steam", "Steam"),
    ("epic", "Epic Games"),
    ("battlenet", "Battle.net"),
    ("ea", "EA app"),
    ("ubisoft", "Ubisoft Connect"),
    ("gog", "GOG"),
    ("xbox", "Xbox"),
    ("disk", "Local paths"),
];
pub(crate) fn scan_client(client: &str, paths: &[PathBuf]) -> Scan {
    let mut scan = Scan::default();
    if client != "disk" && !installed_clients().iter().any(|(id, _)| *id == client) {
        scan.diagnostic("The platform client executable is not installed.".into());
        return scan;
    }
    match client {
        "steam" => steam::steam_candidates(&game_discovery::library_steam_roots(), &mut scan),
        "epic" => {
            let roots = std::env::var_os("ProgramData")
                .map(PathBuf::from)
                .map(|root| root.join("Epic/EpicGamesLauncher/Data/Manifests"))
                .into_iter()
                .collect::<Vec<_>>();
            epic::epic_candidates(&roots, &mut scan);
        }
        "battlenet" => battlenet::scan(&mut scan),
        "ea" | "ubisoft" | "gog" => registry::scan_client(client, &mut scan),
        "xbox" => xbox::scan(&mut scan),
        "disk" => disk::scan(paths, &mut scan),
        _ => scan.diagnostic("Unknown discovery library.".into()),
    }
    finish(scan)
}
pub(crate) fn scan() -> Scan {
    let mut result = Scan::default();
    for (client, _) in installed_clients() {
        let scan = scan_client(client, &[]);
        result.candidates.extend(scan.candidates);
        result.diagnostics.extend(scan.diagnostics);
    }
    finish(result)
}
fn finish(mut scan: Scan) -> Scan {
    for candidate in &mut scan.candidates {
        if candidate.artwork.is_none() {candidate.artwork=artwork::local(Path::new(&candidate.install_path));}
    }
    scan.candidates.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    bound_metadata(&mut scan);
    scan
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod xbox_tests;
