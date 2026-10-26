//! Read-only enumeration of game install records written by common launchers.
//!
//! These registry layouts are based on Playnite's open source library
//! integrations and corroborating Windows game discovery documentation:
//! EA/Origin game keys, Ubisoft's per-app InstallDir keys, and GOG's PATH.
use super::{canonical_text, clean_name, executable_candidates, regular, Candidate, Scan};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct RegistryEntry {
    client: &'static str,
    key_id: String,
    values: BTreeMap<String, String>,
}

/// Converts enumerated records separately from native access so registry
/// layouts, stale entries, and duplicate views can be exercised on any OS.
fn convert(entries: impl IntoIterator<Item = RegistryEntry>) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in entries {
        if candidates.len() >= 512 {
            break;
        }
        let Some(app_id) = safe_app_id(entry.client, &entry.key_id) else {
            continue;
        };
        if !matches!(entry.client, "ea" | "ubisoft" | "gog") {
            continue;
        }
        let Some(install) =
            install_path(&entry).filter(|path| path.is_absolute() && regular(path, true))
        else {
            continue;
        };
        let Some(install_path) = canonical_text(&install) else {
            continue;
        };
        let key = format!("{}:{app_id}", entry.client);
        if !seen.insert(key.clone()) {
            continue;
        }
        let name = named_value(
            &entry,
            &["displayname", "gamename", "name", "title", "productname"],
        )
        .and_then(clean_name)
        .or_else(|| {
            install
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(clean_name)
        })
        .unwrap_or_else(|| format!("{} game", client_label(entry.client)));
        if is_launcher_record(&name) || is_launcher_record(&entry.key_id) {
            continue;
        }
        candidates.push(Candidate {
            id: key,
            client: entry.client.into(),
            name,
            executable_candidates: executable_candidates(&install, None),
            preferred_executable: None,
            install_path,
            game_id: None,
            kind: "unknown".into(),
            app_id,
            launcher: None,
            artwork: None,
        });
    }
    candidates
}

fn client_label(client: &str) -> &'static str {
    match client {
        "ea" => "EA",
        "ubisoft" => "Ubisoft",
        _ => "GOG",
    }
}

fn is_launcher_record(value: &str) -> bool {
    matches!(
        normalized_key(value).as_str(),
        "eaapp"
            | "eadesktop"
            | "origin"
            | "originclient"
            | "ubisoftconnect"
            | "uplay"
            | "goggalaxy"
            | "galaxyclient"
    )
}

fn safe_app_id(client: &str, value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 255 || value.chars().any(char::is_control) {
        return None;
    }
    let normalized = if client == "ea" {
        value.to_ascii_lowercase()
    } else {
        value.to_owned()
    };
    if (client == "ubisoft" || client == "gog")
        && normalized.bytes().all(|byte| byte.is_ascii_digit())
    {
        return (normalized.len() <= 128).then_some(normalized);
    }
    if normalized.len() <= 128
        && normalized
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Some(normalized);
    }
    // Hash the full normalized key so punctuation and truncation cannot alias;
    // the output stays within LibrarySource's ASCII alphanumeric/_/- contract.
    let digest = Sha256::digest(normalized.as_bytes());
    Some(format!("registry-{}", hex_encode(&digest)))
}

fn normalized_key(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}
fn hex_encode(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join("")
}
fn named_value<'a>(entry: &'a RegistryEntry, names: &[&str]) -> Option<&'a str> {
    names.iter().find_map(|name| {
        let wanted = normalized_key(name);
        entry
            .values
            .iter()
            .find(|(key, value)| normalized_key(key) == wanted && !value.trim().is_empty())
            .map(|(_, value)| value.as_str())
    })
}
fn install_path(entry: &RegistryEntry) -> Option<PathBuf> {
    let names: &[&str] = match entry.client {
        "ea" => &["InstallDir", "Install Dir", "InstallLocation", "Path"],
        "ubisoft" => &["InstallDir", "Install Dir"],
        "gog" => &["PATH", "InstallDir", "Install Dir"],
        _ => return None,
    };
    named_value(entry, names)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

pub(super) fn scan_client(client: &str, scan: &mut Scan) {
    for candidate in convert(
        native::entries()
            .into_iter()
            .filter(|entry| entry.client == client),
    ) {
        if scan.candidates.len() >= 512 {
            break;
        }
        scan.candidates.push(candidate);
    }
}
pub(super) fn scan(scan: &mut Scan) {
    let entries = native::entries();
    for candidate in convert(entries) {
        if scan.candidates.len() >= 512 {
            break;
        }
        scan.candidates.push(candidate);
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::{ffi::c_void, ptr};
    pub(super) type HKey = *mut c_void;
    const HKCU: HKey = 0x8000_0001u32 as i32 as isize as HKey;
    const HKLM: HKey = 0x8000_0002u32 as i32 as isize as HKey;
    const KEY_READ: u32 = 0x20019;
    pub(super) const KEY_WOW64_64KEY: u32 = 0x0100;
    const KEY_WOW64_32KEY: u32 = 0x0200;
    const RRF_RT_REG_SZ: u32 = 0x2;
    const ERROR_SUCCESS: i32 = 0;
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegOpenKeyExW(
            key: HKey,
            subkey: *const u16,
            options: u32,
            access: u32,
            result: *mut HKey,
        ) -> i32;
        fn RegCloseKey(key: HKey) -> i32;
        fn RegQueryInfoKeyW(
            key: HKey,
            class: *mut u16,
            class_len: *mut u32,
            reserved: *mut u32,
            subkeys: *mut u32,
            max_subkey: *mut u32,
            max_class: *mut u32,
            values: *mut u32,
            max_value_name: *mut u32,
            max_value: *mut u32,
            security: *mut u32,
            modified: *mut u64,
        ) -> i32;
        fn RegEnumKeyExW(
            key: HKey,
            index: u32,
            name: *mut u16,
            name_len: *mut u32,
            reserved: *mut u32,
            class: *mut u16,
            class_len: *mut u32,
            modified: *mut u64,
        ) -> i32;
        fn RegGetValueW(
            key: HKey,
            subkey: *const u16,
            name: *const u16,
            flags: u32,
            kind: *mut u32,
            data: *mut c_void,
            size: *mut u32,
        ) -> i32;
    }
    struct Key(HKey);
    impl Drop for Key {
        fn drop(&mut self) {
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }
    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }
    fn open(root: HKey, path: &str, view: u32) -> Option<Key> {
        let mut key = ptr::null_mut();
        (unsafe { RegOpenKeyExW(root, wide(path).as_ptr(), 0, KEY_READ | view, &mut key) }
            == ERROR_SUCCESS)
            .then(|| Key(key))
    }
    fn value(key: HKey, name: &str) -> Option<String> {
        let mut data = vec![0u16; 32768];
        let mut bytes = (data.len() * 2) as u32;
        let result = unsafe {
            RegGetValueW(
                key,
                ptr::null(),
                wide(name).as_ptr(),
                RRF_RT_REG_SZ,
                ptr::null_mut(),
                data.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        if result != ERROR_SUCCESS || bytes < 2 || bytes as usize > data.len() * 2 {
            return None;
        }
        data.truncate(bytes as usize / 2);
        let end = data
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(data.len());
        String::from_utf16(&data[..end])
            .ok()
            .filter(|value| !value.trim().is_empty())
    }
    pub(super) fn entries_at(
        root: HKey,
        root_path: &str,
        view: u32,
        client: &'static str,
    ) -> Vec<RegistryEntry> {
        let Some(parent) = open(root, root_path, view) else {
            return Vec::new();
        };
        let mut subkeys = 0;
        if unsafe {
            RegQueryInfoKeyW(
                parent.0,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut subkeys,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        } != ERROR_SUCCESS
        {
            return Vec::new();
        }
        let mut found = Vec::new();
        for index in 0..subkeys.min(4096) {
            let mut name = vec![0u16; 256];
            let mut length = name.len() as u32;
            if unsafe {
                RegEnumKeyExW(
                    parent.0,
                    index,
                    name.as_mut_ptr(),
                    &mut length,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            } != ERROR_SUCCESS
            {
                continue;
            }
            let Ok(id) = String::from_utf16(&name[..length as usize]) else {
                continue;
            };
            if safe_app_id(client, &id).is_none() {
                continue;
            }
            let Some(child) = open(parent.0, &id, view) else {
                continue;
            };
            let fields: &[&str] = match client {
                "ea" => &[
                    "InstallDir",
                    "Install Dir",
                    "InstallLocation",
                    "Path",
                    "DisplayName",
                    "GameName",
                    "Name",
                    "Title",
                    "ProductName",
                ],
                "ubisoft" => &[
                    "InstallDir",
                    "Install Dir",
                    "DisplayName",
                    "GameName",
                    "Name",
                    "Title",
                    "ProductName",
                ],
                _ => &[
                    "PATH",
                    "InstallDir",
                    "Install Dir",
                    "DisplayName",
                    "GameName",
                    "Name",
                    "Title",
                    "ProductName",
                ],
            };
            let mut values = fields
                .iter()
                .filter_map(|field| value(child.0, field).map(|value| ((*field).to_owned(), value)))
                .collect::<BTreeMap<String, String>>();
            if client == "ubisoft"
                && named_value(
                    &RegistryEntry {
                        client,
                        key_id: id.clone(),
                        values: values.clone(),
                    },
                    &["DisplayName", "Name"],
                )
                .is_none()
            {
                let uninstall = format!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\UPlay Install {{{id}}}");
                if let Some(uninstall_key) = open(root, &uninstall, view) {
                    if let Some(display_name) = value(uninstall_key.0, "DisplayName") {
                        values.insert("DisplayName".into(), display_name);
                    }
                }
            }
            found.push(RegistryEntry {
                client,
                key_id: id,
                values,
            });
        }
        found
    }
    pub(super) fn entries() -> Vec<RegistryEntry> {
        let mut result = Vec::new();
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            for (root, path, client) in [
                (HKLM, "SOFTWARE\\EA Games", "ea"),
                (HKCU, "SOFTWARE\\EA Games", "ea"),
                (HKLM, "SOFTWARE\\Electronic Arts\\EA Games", "ea"),
                (HKCU, "SOFTWARE\\Electronic Arts\\EA Games", "ea"),
                (
                    HKLM,
                    "SOFTWARE\\Electronic Arts\\EA Core\\Installed Games",
                    "ea",
                ),
                (
                    HKCU,
                    "SOFTWARE\\Electronic Arts\\EA Core\\Installed Games",
                    "ea",
                ),
                (HKLM, "SOFTWARE\\Origin Games", "ea"),
                (HKCU, "SOFTWARE\\Origin Games", "ea"),
                (HKLM, "SOFTWARE\\Ubisoft\\Launcher\\Installs", "ubisoft"),
                (HKCU, "SOFTWARE\\Ubisoft\\Launcher\\Installs", "ubisoft"),
                (HKLM, "SOFTWARE\\GOG.com\\Games", "gog"),
                (HKCU, "SOFTWARE\\GOG.com\\Games", "gog"),
            ] {
                result.extend(entries_at(root, path, view, client));
            }
        }
        result
    }
}

#[cfg(not(windows))]
mod native {
    use super::*;
    pub(super) fn entries() -> Vec<RegistryEntry> {
        Vec::new()
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
