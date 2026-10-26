//! Launcher presence checks used to decide which client libraries to expose.
//! A launcher is present only when one of its own executable files is readable
//! as a regular file; game records by themselves never enable a library.

use super::CLIENTS;
use std::{
    fs,
    path::{Path, PathBuf},
};

const CLIENT_EXES: &[(&str, &[&str])] = &[
    ("steam", &["steam.exe"]),
    (
        "epic",
        &["Portal/Binaries/Win64/EpicGamesLauncher.exe","Portal/Binaries/Win32/EpicGamesLauncher.exe","Launcher/Portal/Binaries/Win64/EpicGamesLauncher.exe","Launcher/Portal/Binaries/Win32/EpicGamesLauncher.exe","EpicGamesLauncher.exe"],
    ),
    ("battlenet", &["Battle.net.exe", "Battle.net Launcher.exe"]),
    ("ea", &["EADesktop.exe","Origin.exe"]),
    ("ubisoft", &["UbisoftConnect.exe","upc.exe","Uplay.exe"]),
    ("gog", &["GalaxyClient.exe"]),
    ("xbox", &["XboxPcApp.exe"]),
];

/// Returns only installed launcher libraries, in the same order as `CLIENTS`.
/// Local-path discovery remains available on every platform.
pub(crate) fn installed_clients() -> Vec<(&'static str, &'static str)> {
    CLIENTS
        .iter()
        .copied()
        .filter(|(client, _)| *client == "disk" || launcher_present(client))
        .collect()
}

fn launcher_present(client: &str) -> bool {
    let exes = exes_for(client);
    if exes.is_empty() {
        return false;
    }
    let mut roots = default_roots(client);
    roots.extend(native::registered_roots(client));
    if client == "steam" {
        roots.extend(crate::game_discovery::library_steam_roots());
    }
    roots.iter().any(|root| has_client_exe(root, exes))
}

fn exes_for(client: &str) -> &'static [&'static str] {
    CLIENT_EXES
        .iter()
        .find_map(|(id, paths)| (*id == client).then_some(*paths))
        .unwrap_or(&[])
}

fn has_client_exe(root: &Path, relative_exes: &[&str]) -> bool {
    relative_exes
        .iter()
        .any(|relative| regular_file(&root.join(relative)))
}

fn regular_file(path: &Path) -> bool {
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
    !metadata.file_type().is_symlink() && metadata.is_file()
}

fn default_roots(client: &str) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
        let Some(base) = std::env::var_os(variable).map(PathBuf::from) else {
            continue;
        };
        let name = match client {
            "steam" => "Steam",
            "epic" => "Epic Games/Launcher",
            "battlenet" => "Battle.net",
            "ea" => "Electronic Arts/EA Desktop/EA Desktop",
            "ubisoft" => "Ubisoft/Ubisoft Game Launcher",
            "gog" => "GOG Galaxy",
            // Xbox is packaged under WindowsApps; query its registered package.
            _ => continue,
        };
        roots.push(base.join(name));
        if client=="ea" {roots.push(base.join("Origin"));roots.push(base.join("Electronic Arts/EA Desktop"));}
    }
    roots
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::{ffi::c_void, ptr};

    type HKey = *mut c_void;
    const HKCU: HKey = 0x8000_0001u32 as i32 as isize as HKey;
    const HKLM: HKey = 0x8000_0002u32 as i32 as isize as HKey;
    const KEY_READ: u32 = 0x20019;
    const WOW64_64: u32 = 0x0100;
    const WOW64_32: u32 = 0x0200;
    const REG_SZ: u32 = 0x2;
    const OK: i32 = 0;

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
        fn RegEnumKeyExW(
            key: HKey,
            index: u32,
            name: *mut u16,
            length: *mut u32,
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
        (unsafe { RegOpenKeyExW(root, wide(path).as_ptr(), 0, KEY_READ | view, &mut key) } == OK)
            .then(|| Key(key))
    }
    fn value(key: HKey, name: &str) -> Option<String> {
        let mut data = vec![0u16; 16384];
        let mut bytes = (data.len() * 2) as u32;
        let status = unsafe {
            RegGetValueW(
                key,
                ptr::null(),
                wide(name).as_ptr(),
                REG_SZ,
                ptr::null_mut(),
                data.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        if status != OK || bytes < 2 || bytes as usize > data.len() * 2 {
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
    fn children(key: HKey) -> Vec<String> {
        let mut found = Vec::new();
        for index in 0..4096 {
            let mut name = vec![0u16; 512];
            let mut length = name.len() as u32;
            let status = unsafe {
                RegEnumKeyExW(
                    key,
                    index,
                    name.as_mut_ptr(),
                    &mut length,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            };
            if status != OK {
                break;
            }
            if let Ok(name) = String::from_utf16(&name[..length as usize]) {
                found.push(name);
            }
        }
        found
    }
    fn add_if_client_install(
        roots: &mut Vec<PathBuf>,
        root: HKey,
        view: u32,
        path: &str,
        expected_name: &str,
        exes: &[&str],
    ) {
        let Some(parent) = open(root, path, view) else {
            return;
        };
        for child_name in children(parent.0) {
            let Some(child) = open(parent.0, &child_name, view) else {
                continue;
            };
            let display = value(child.0, "DisplayName")
                .unwrap_or_default()
                .to_ascii_lowercase();
            // Match the launcher product itself; game uninstall entries are ignored.
            if !display.contains(expected_name) {
                continue;
            }
            if let Some(location) = value(child.0, "InstallLocation") {
                roots.push(PathBuf::from(location));
            }
            if let Some(icon) = value(child.0, "DisplayIcon") {
                let icon = icon
                    .split(',')
                    .next()
                    .unwrap_or(&icon)
                    .trim()
                    .trim_matches('"');
                let icon_path = PathBuf::from(icon);
                if exes.iter().any(|exe| {
                    icon_path.file_name().is_some_and(|name| {
                        name.eq_ignore_ascii_case(Path::new(exe).file_name().unwrap_or_default())
                    })
                }) {
                    if let Some(parent) = icon_path.parent() {
                        roots.push(parent.to_owned());
                    }
                }
            }
        }
    }

    fn add_launcher_key_roots(
        roots: &mut Vec<PathBuf>,
        root: HKey,
        view: u32,
        paths: &[&str],
        exes: &[&str],
    ) {
        for path in paths {
            let Some(key) = open(root, path, view) else {
                continue;
            };
            for field in [
                "InstallPath",
                "InstallLocation",
                "InstallDir",
                "Path",
                "ClientPath",
                "ExecutablePath",
            ] {
                let Some(value) = value(key.0, field) else {
                    continue;
                };
                let path = PathBuf::from(value.trim().trim_matches('"'));
                if exes.iter().any(|exe| {
                    path.file_name().is_some_and(|name| {
                        name.eq_ignore_ascii_case(Path::new(exe).file_name().unwrap_or_default())
                    })
                }) {
                    if let Some(parent) = path.parent() {
                        roots.push(parent.to_owned());
                    }
                } else {
                    roots.push(path);
                }
            }
        }
    }

    pub(super) fn registered_roots(client: &str) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        let exes = exes_for(client);
        for view in [WOW64_64, WOW64_32] {
            if client == "steam" {
                for root in [HKLM, HKCU] {
                    for key_path in [
                        "SOFTWARE\\Valve\\Steam",
                        "SOFTWARE\\WOW6432Node\\Valve\\Steam",
                    ] {
                        if let Some(key) = open(root, key_path, view) {
                            if let Some(path) =
                                value(key.0, "InstallPath").or_else(|| value(key.0, "SteamPath"))
                            {
                                roots.push(PathBuf::from(path));
                            }
                        }
                    }
                }
            }
            let (label, package_prefix) = match client {
                "epic" => ("epic games launcher", None),
                "battlenet" => ("battle.net", None),
                "ea" => ("ea app", None),
                "ubisoft" => ("ubisoft connect", None),
                "gog" => ("gog galaxy", None),
                "xbox" => ("", Some("Microsoft.GamingApp_")),
                _ => continue,
            };
            let direct_keys: &[&str] = match client {
                "epic" => &[
                    "SOFTWARE\\Epic Games\\EpicGamesLauncher",
                    "SOFTWARE\\WOW6432Node\\Epic Games\\EpicGamesLauncher",
                ],
                "battlenet" => &[
                    "SOFTWARE\\Blizzard Entertainment\\Battle.net",
                    "SOFTWARE\\WOW6432Node\\Blizzard Entertainment\\Battle.net",
                ],
                "ea" => &[
                    "SOFTWARE\\Electronic Arts\\EA Desktop",
                    "SOFTWARE\\WOW6432Node\\Electronic Arts\\EA Desktop",
                ],
                "ubisoft" => &[
                    "SOFTWARE\\Ubisoft\\Launcher",
                    "SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher",
                ],
                "gog" => &[
                    "SOFTWARE\\GOG.com\\GalaxyClient",
                    "SOFTWARE\\WOW6432Node\\GOG.com\\GalaxyClient",
                ],
                _ => &[],
            };
            for root in [HKLM, HKCU] {
                add_launcher_key_roots(&mut roots, root, view, direct_keys, exes);
                add_if_client_install(
                    &mut roots,
                    root,
                    view,
                    "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
                    label,
                    exes,
                );
            }
            if let Some(prefix) = package_prefix {
                for root in [HKCU, HKLM] {
                    let path = "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\Repository\\Packages";
                    if let Some(packages) = open(root, path, view) {
                        for package in children(packages.0)
                            .into_iter()
                            .filter(|name| name.starts_with(prefix))
                            .take(64)
                        {
                            if let Some(key) = open(packages.0, &package, view) {
                                if let Some(location) = value(key.0, "InstallLocation") {
                                    roots.push(PathBuf::from(location));
                                }
                            }
                        }
                    }
                }
            }
        }
        roots
    }
}

#[cfg(not(windows))]
mod native {
    use super::*;
    pub(super) fn registered_roots(_: &str) -> Vec<PathBuf> {
        Vec::new()
    }
}

#[cfg(test)]
#[path = "platforms_tests.rs"]
mod tests;
