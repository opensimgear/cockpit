//! Bounded, read-only discovery for Xbox app PC GDK installations.
use super::{
    canonical_text, clean_name, executable_candidates, regular, sorted_children, Candidate, Scan,
};
use quick_xml::{
    events::{BytesStart, Event},
    Reader,
};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_ROOTS: usize = 26;
const MAX_GAMES_PER_ROOT: usize = 2048;
#[cfg(windows)]
const DRIVE_FIXED: u32 = 3;

/// Find only conventional XboxGames folders on local drive roots. A marker is
/// not required because newer flat-file installs may not expose one consistently.
pub(super) fn scan(scan: &mut Scan) {
    let mut roots = Vec::new();
    #[cfg(windows)]
    {
        use windows::{
            core::PCWSTR,
            Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives},
        };
        let drives = unsafe { GetLogicalDrives() };
        for index in 0..26 {
            if drives & (1 << index) == 0 {
                continue;
            }
            let letter = b'A' + index as u8;
            let drive = format!("{}:\\", letter as char);
            let mut wide_drive = drive.encode_utf16().collect::<Vec<_>>();
            wide_drive.push(0);
            // GetDriveTypeW returns 3 for fixed drives. Exclude network,
            // removable, optical, and unknown roots to keep discovery local.
            if unsafe { GetDriveTypeW(PCWSTR(wide_drive.as_ptr())) } != DRIVE_FIXED {
                continue;
            }
            let drive = PathBuf::from(drive);
            let games = drive.join("XboxGames");
            if regular(&games, true) || regular(&drive.join(".GamingRoot"), false) {
                if regular(&games, true) {
                    roots.push(games);
                }
            }
        }
    }
    scan_roots(&roots, scan);
    scan.diagnostic(
        "Xbox discovery checks conventional XboxGames folders on drive roots. Custom install folders and protected WindowsApps packages may not be visible; add those games manually.".into(),
    );
}

/// Scan injected XboxGames roots; each root is inspected one directory level deep.
pub(super) fn scan_roots(roots: &[PathBuf], scan: &mut Scan) {
    let mut seen = BTreeSet::new();
    for root in roots.iter().take(MAX_ROOTS) {
        if !regular(root, true) {
            continue;
        }
        for game_dir in sorted_children(root).into_iter().take(MAX_GAMES_PER_ROOT) {
            if scan.candidates.len() >= 512 {
                scan.diagnostic("The local library scan reached its 512-entry limit.".into());
                return;
            }
            if !regular(&game_dir, true) {
                continue;
            }
            let content = game_dir.join("Content");
            if !regular(&content, true) {
                continue;
            }
            let config = content.join("MicrosoftGame.config");
            if !regular(&config, false) {
                continue;
            }
            let Some(config_text) = read_config(&config) else {
                scan.diagnostic(format!(
                    "Xbox game configuration could not be read: {}",
                    config.display()
                ));
                continue;
            };
            let Some(metadata) = parse_config(&config_text) else {
                scan.diagnostic(format!(
                    "Xbox game configuration is malformed or lacks a valid identity: {}",
                    config.display()
                ));
                continue;
            };
            if !seen.insert(metadata.app_id.clone()) {
                continue;
            }
            let Some(install_path) = canonical_text(&game_dir) else {
                continue;
            };
            let preferred = metadata
                .executable
                .as_deref()
                .and_then(|name| safe_relative(name))
                .map(|name| content.join(name));
            let executables = executable_candidates(&game_dir, preferred.as_deref());
            let preferred_executable = preferred
                .as_deref()
                .and_then(canonical_text)
                .filter(|path| executables.contains(path));
            scan.candidates.push(Candidate {
                id: format!("xbox:{}", metadata.app_id),
                client: "xbox".into(),
                name: metadata.name.unwrap_or_else(|| {
                    game_dir
                        .file_name()
                        .and_then(|name| name.to_str())
                        .and_then(clean_name)
                        .unwrap_or_else(|| "Xbox game".into())
                }),
                install_path,
                executable_candidates: executables,
                preferred_executable,
                game_id: None,
                kind: "game".into(),
                app_id: metadata.app_id,
                launcher: None,
                artwork: None,
            });
        }
    }
}

struct GameMetadata {
    app_id: String,
    name: Option<String>,
    executable: Option<String>,
}

fn read_config(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn parse_config(xml: &str) -> Option<GameMetadata> {
    let mut app_id = None;
    let mut name = None;
    let mut executable = None;
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut depth = 0usize;
    let mut root_closed = false;
    loop {
        match reader.read_event().ok()? {
            Event::Start(element) => {
                if root_closed {
                    return None;
                }
                let tag = element.local_name().as_ref().to_ascii_lowercase();
                if depth == 0 && tag != "game" {
                    return None;
                }
                read_config_element(&element, &mut app_id, &mut name, &mut executable)?;
                depth += 1;
            }
            Event::Empty(element) => {
                if root_closed {
                    return None;
                }
                let tag = element.local_name().as_ref().to_ascii_lowercase();
                if depth == 0 {
                    if tag != "game" {
                        return None;
                    }
                    root_closed = true;
                }
                read_config_element(&element, &mut app_id, &mut name, &mut executable)?;
            }
            Event::End(_) => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    root_closed = true;
                }
            }
            Event::Eof => break,
            Event::Text(text) if depth == 0 && !text.xml10_content().trim().is_empty() => {
                return None
            }
            _ => {}
        }
    }
    if !root_closed || depth != 0 {
        return None;
    }
    let app_id = app_id.filter(|value| {
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    })?;
    executable.as_ref()?;
    Some(GameMetadata {
        app_id,
        name,
        executable,
    })
}

fn read_config_element(
    element: &BytesStart<'_>,
    app_id: &mut Option<String>,
    name: &mut Option<String>,
    executable: &mut Option<String>,
) -> Option<()> {
    let tag = element.local_name().as_ref().to_ascii_lowercase();
    let mut attributes = std::collections::BTreeMap::new();
    for attribute in element.attributes().with_checks(true) {
        let attribute = attribute.ok()?;
        let key = attribute.key.local_name().as_ref().to_ascii_lowercase();
        let value = attribute
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .ok()?
            .into_owned();
        if attributes.insert(key, value).is_some() {
            return None;
        }
    }
    match tag.as_str() {
        "identity" if app_id.is_none() => *app_id = attributes.get("name").cloned(),
        "shellvisuals" if name.is_none() => {
            *name = attributes
                .get("defaultdisplayname")
                .and_then(|value| clean_name(value));
        }
        "executable" if executable.is_none() => {
            let family = attributes.get("targetdevicefamily");
            if family.is_none_or(|family| family.eq_ignore_ascii_case("pc")) {
                *executable = attributes.get("name").cloned();
            }
        }
        _ => {}
    }
    Some(())
}

fn safe_relative(value: &str) -> Option<PathBuf> {
    let normalized = value.replace('\\', "/");
    let path = Path::new(&normalized);
    if value.contains(':') || path.is_absolute() || !value.to_ascii_lowercase().ends_with(".exe") {
        return None;
    }
    path.components()
        .all(|component| matches!(component, Component::Normal(_)))
        .then(|| path.to_owned())
}
