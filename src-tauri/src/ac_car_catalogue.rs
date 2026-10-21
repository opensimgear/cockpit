//! Read-only installed-car discovery for original Assetto Corsa. This catalogue
//! describes content present on disk, never account entitlement or a live car.
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::{Component, Path},
};

#[derive(Clone, Copy)]
struct Limits {
    car_entries: usize,
    vehicles: usize,
    ui_bytes: usize,
    total_ui_bytes: usize,
    scan_entries: usize,
    skin_entries: usize,
    skin_files: usize,
}
const LIMITS: Limits = Limits {
    car_entries: 4096,
    vehicles: 1024,
    ui_bytes: 256 * 1024,
    total_ui_bytes: 16 * 1024 * 1024,
    scan_entries: 20_000,
    skin_entries: 512,
    skin_files: 256,
};
#[derive(Default)]
struct Budget {
    entries: usize,
    ui_bytes: usize,
}
fn redirected(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        return metadata.file_attributes() & 0x400 != 0;
    }
    #[cfg(not(windows))]
    false
}
/// Validate every descendant component, including junctions on Windows, before
/// following it. The selected installation itself is canonicalized once.
fn safe_metadata(root: &Path, path: &Path) -> Option<fs::Metadata> {
    let relative = path.strip_prefix(root).ok()?;
    let mut current = root.to_path_buf();
    let mut metadata = fs::symlink_metadata(root).ok()?;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return None;
        };
        current.push(name);
        metadata = fs::symlink_metadata(&current).ok()?;
        if redirected(&metadata) {
            return None;
        }
    }
    fs::canonicalize(path)
        .ok()?
        .starts_with(root)
        .then_some(metadata)
}
fn directory(root: &Path, path: &Path) -> bool {
    safe_metadata(root, path).is_some_and(|metadata| metadata.is_dir())
}
fn nonempty_file(root: &Path, path: &Path) -> bool {
    safe_metadata(root, path).is_some_and(|metadata| metadata.is_file() && metadata.len() != 0)
}
fn entries(
    root: &Path,
    path: &Path,
    maximum: usize,
    budget: &mut Budget,
    limits: Limits,
) -> Result<Option<Vec<fs::DirEntry>>, String> {
    if !directory(root, path) {
        return Ok(None);
    }
    let Ok(read) = fs::read_dir(path) else {
        return Ok(None);
    };
    let mut values = Vec::new();
    for (index, entry) in read.take(maximum + 1).enumerate() {
        budget.entries += 1;
        if budget.entries > limits.scan_entries {
            return Err("Assetto Corsa car discovery exceeded its bounded directory scan. Narrow the installed content and repair setup again.".into());
        }
        if index == maximum {
            return Ok(None);
        }
        if let Ok(entry) = entry {
            values.push(entry);
        }
    }
    values.sort_by_key(|entry| entry.file_name());
    Ok(Some(values))
}
fn has_skin(root: &Path, car: &Path, budget: &mut Budget, limits: Limits) -> Result<bool, String> {
    let Some(skins) = entries(
        root,
        &car.join("skins"),
        limits.skin_entries,
        budget,
        limits,
    )?
    else {
        return Ok(false);
    };
    for skin in skins {
        let Some(files) = entries(root, &skin.path(), limits.skin_files, budget, limits)? else {
            continue;
        };
        if files.iter().any(|file| nonempty_file(root, &file.path())) {
            return Ok(true);
        }
    }
    Ok(false)
}
/// Shipped ui_car.json files contain literal newlines/tabs in description
/// strings. Normalize only those unescaped controls; leave JSON escape grammar,
/// quote boundaries, and all UTF-8 characters for serde_json to validate.
fn normalize_controls(text: &str) -> Option<String> {
    let mut quoted = false;
    let mut escaped = false;
    let mut changed = false;
    let mut result = String::with_capacity(text.len());
    for character in text.chars() {
        if quoted && character.is_ascii_control() {
            if escaped {
                return None;
            }
            use std::fmt::Write;
            write!(result, "\\u{:04x}", character as u32).ok()?;
            changed = true;
            continue;
        }
        result.push(character);
        if escaped {
            escaped = false;
        } else if quoted && character == '\\' {
            escaped = true;
        } else if character == '"' {
            quoted = !quoted;
        }
    }
    changed.then_some(result)
}
fn parse_ui(bytes: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(bytes)
        .ok()?
        .trim_start_matches('\u{feff}');
    serde_json::from_str(text)
        .ok()
        .or_else(|| serde_json::from_str(&normalize_controls(text)?).ok())
}
fn read_ui(
    root: &Path,
    path: &Path,
    budget: &mut Budget,
    limits: Limits,
) -> Result<Option<Value>, String> {
    let Some(before) = safe_metadata(root, path).filter(|metadata| metadata.is_file()) else {
        return Ok(None);
    };
    if before.len() == 0 || before.len() > limits.ui_bytes as u64 {
        return Ok(None);
    }
    let Ok(file) = fs::File::open(path) else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    if file
        .take(limits.ui_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return Ok(None);
    }
    budget.ui_bytes += bytes.len();
    if budget.ui_bytes > limits.total_ui_bytes {
        return Err("Assetto Corsa car UI metadata exceeds its bounded catalogue size. Narrow the installed content and repair setup again.".into());
    }
    if bytes.len() > limits.ui_bytes {
        return Ok(None);
    }
    let Some(after) = safe_metadata(root, path) else {
        return Ok(None);
    };
    if !after.is_file()
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Ok(None);
    }
    Ok(parse_ui(&bytes))
}
fn valid_text(text: &str, maximum: usize) -> bool {
    !text.is_empty() && text.len() <= maximum && !text.chars().any(char::is_control)
}
fn scan_with_limits(install_root: &Path, limits: Limits) -> Result<Vec<Value>, String> {
    if !install_root.is_absolute() {
        return Err("Choose an absolute Assetto Corsa installation directory.".into());
    }
    let root = fs::canonicalize(install_root)
        .map_err(|error| format!("Cannot access the Assetto Corsa installation: {error}"))?;
    if !root.is_dir() {
        return Err("Choose an Assetto Corsa installation directory.".into());
    }
    let cars = root.join("content/cars");
    let mut budget = Budget::default();
    let candidates = entries(&root, &cars, limits.car_entries, &mut budget, limits)?
        .ok_or("Assetto Corsa content/cars is missing, redirected, unreadable, or exceeds its bounded directory size.")?;
    let mut vehicles = Vec::new();
    for entry in candidates {
        let Some(id) = entry
            .file_name()
            .to_str()
            .map(str::to_owned)
            .filter(|id| valid_text(id, 256) && id.trim() == id && id != "NONE")
        else {
            continue;
        };
        let car = entry.path();
        if !directory(&root, &car)
            || !(nonempty_file(&root, &car.join("data.acd"))
                || nonempty_file(&root, &car.join("data/car.ini")))
        {
            continue;
        }
        let Some(ui) = read_ui(&root, &car.join("ui/ui_car.json"), &mut budget, limits)? else {
            continue;
        };
        let Some(name) = ui["name"]
            .as_str()
            .map(str::trim)
            .filter(|name| valid_text(name, 256))
        else {
            continue;
        };
        if !has_skin(&root, &car, &mut budget, limits)? {
            continue;
        }
        if vehicles.len() == limits.vehicles {
            return Err("The installed Assetto Corsa car catalogue exceeds 1024 vehicles. Narrow the installed content and repair setup again.".into());
        }
        vehicles.push(json!({"id":id,"name":name}));
    }
    Ok(vehicles)
}

/// Discover complete installed original-AC cars and mods without starting the
/// game. Folder IDs remain exact so native car_model telemetry can match them.
pub(crate) fn scan(install_root: &Path) -> Result<Vec<Value>, String> {
    scan_with_limits(install_root, LIMITS)
}

#[cfg(test)]
#[path = "ac_car_catalogue_tests.rs"]
mod tests;
