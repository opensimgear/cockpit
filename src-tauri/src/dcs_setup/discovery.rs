use std::ffi::OsStr;

pub(crate) fn is_profile_folder_name(name: &OsStr) -> bool {
    name.to_str().is_some_and(|name| {
        name.eq_ignore_ascii_case("dcs")
            || name.get(..4).is_some_and(|prefix| prefix.eq_ignore_ascii_case("dcs."))
    })
}

#[cfg(any(windows, test))]
use super::{read, BTreeSet, Path, PathBuf};
#[cfg(any(windows, test))]
use std::fs;

#[cfg(any(windows, test))]
const STANDARD_FOLDERS: [&str; 2] = ["DCS", "DCS.openbeta"];
#[cfg(any(windows, test))]
const MAX_DISCOVERED_VARIANTS: usize = 128;

/// Enumerate the Saved Games root without retaining unrelated entries. Keep a
/// deterministic, bounded set of variants and preserve the installation hint.
#[cfg(any(windows, test))]
pub(super) fn candidates(saved_games: &Path, install_path: Option<&Path>) -> Result<Vec<PathBuf>, String> {
    if !saved_games.is_absolute() {
        return Err("The Saved Games location must be absolute.".into());
    }
    let mut paths = BTreeSet::new();
    if let Ok(entries) = fs::read_dir(saved_games) {
        let mut variants = BTreeSet::new();
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let name = entry.file_name();
            if !is_profile_folder_name(&name) {
                continue;
            }
            if STANDARD_FOLDERS.iter().any(|standard| name.eq_ignore_ascii_case(OsStr::new(standard))) {
                let path = entry.path();
                if path.is_dir() {
                    paths.insert(path);
                }
            } else if entry.file_type().map_err(|error| error.to_string())?.is_dir() {
                variants.insert(entry.path());
                if variants.len() > MAX_DISCOVERED_VARIANTS {
                    variants.pop_last();
                }
            }
        }
        paths.extend(variants);
    } else {
        paths.extend(STANDARD_FOLDERS.map(|name| saved_games.join(name)).into_iter().filter(|path| path.is_dir()));
    }
    if let Some(root) = install_path {
        let variant = root.join("dcs_variant.txt");
        if variant.exists() {
            let raw = read(&variant, 256)?;
            let text = String::from_utf8(raw)
                .map_err(|_| "The DCS user-folder override is not valid UTF-8.")?;
            let suffix = text.trim_start_matches('\u{feff}').trim();
            if suffix.is_empty()
                || suffix.len() > 128
                || suffix.contains(['/', '\\', ':'])
                || suffix.chars().any(char::is_control)
                || suffix == "."
                || suffix == ".."
            {
                return Err(
                    "The DCS user-folder override is ambiguous. Select its user folder explicitly."
                        .into(),
                );
            }
            let selected = saved_games.join(format!("DCS.{suffix}"));
            if !selected.is_dir() {
                return Err(format!("DCS selects {}. Start this installation once or select its user folder explicitly.", selected.display()));
            }
            if !paths.contains(&selected) && !matches_existing_variant(&paths, &selected) {
                paths.insert(selected);
            }
        }
    }
    Ok(paths.into_iter().collect())
}

#[cfg(any(windows, test))]
fn matches_existing_variant(paths: &BTreeSet<PathBuf>, selected: &Path) -> bool {
    let Some(name) = selected.file_name() else { return false; };
    paths.iter().any(|path| {
        path.file_name().is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
            && fs::canonicalize(path).ok().is_some_and(|actual| fs::canonicalize(selected).is_ok_and(|hint| hint == actual))
    })
}

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod tests;
