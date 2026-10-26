//! Bounded discovery of standalone installations, including existing simulator recognizers.
use super::*;
use crate::game_discovery::Platform;

fn roots(extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = extra.to_vec();
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
            let drive = format!("{}:\\", (b'A' + index as u8) as char);
            let wide = drive.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
            if unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) } != 3 {
                continue;
            }
            for folder in ["Games", "GOG Games", "EA Games", "Ubisoft Games"] {
                let root = PathBuf::from(&drive).join(folder);
                if regular(&root, true) {
                    roots.push(root);
                }
            }
        }
        if let Some(home) = std::env::var_os("USERPROFILE") {
            for folder in ["Games", "Saved Games"] {
                let root = PathBuf::from(&home).join(folder);
                if regular(&root, true) {
                    roots.push(root);
                }
            }
        }
    }
    roots.sort();
    roots.dedup();
    roots
}

pub(super) fn scan(extra: &[PathBuf], scan: &mut Scan) {
    let mut seen = BTreeSet::new();
    // Recognizers discover real installations; absent integrations create no entries.
    for game in crate::game_discovery::NativePlatform.catalogue(&Default::default()) {
        if game["installed"] != true {
            continue;
        }
        let Some(root) = game["installPath"].as_str().map(PathBuf::from) else {
            continue;
        };
        let runtimes = game["processExecutables"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        add(&root, runtimes, scan, &mut seen);
        if let Some(instances) = game["installations"].as_array() {
            for instance in instances {
                if let Some(root) = instance["installPath"].as_str().map(PathBuf::from) {
                    add(&root, executable_candidates(&root, None), scan, &mut seen);
                }
            }
        }
    }
    for root in roots(extra) {
        if root.is_file() {
            if let Some(parent) = root.parent() {
                add(
                    parent,
                    executable_candidates(parent, Some(&root)),
                    scan,
                    &mut seen,
                );
            }
            continue;
        }
        if !regular(&root, true) {
            if !fs::metadata(&root).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
            {
                scan.diagnostic(format!(
                    "Local discovery location is inaccessible: {}",
                    root.display()
                ));
            }
            continue;
        }
        let mut pending = vec![(root, 0)];
        let mut inspected = 0;
        while let Some((path, depth)) = pending.pop() {
            if inspected >= 4096 || scan.candidates.len() >= MAX_CANDIDATES {
                break;
            }
            inspected += 1;
            let children = sorted_children(&path);
            let direct = children
                .iter()
                .any(|file| regular(file, false) && runtime_filename(file));
            let recognized = game_discovery::recognize_installation(&path, &[]).is_some();
            if direct || recognized {
                add(&path, executable_candidates(&path, None), scan, &mut seen);
                continue;
            }
            if depth < 4 {
                for child in children
                    .into_iter()
                    .rev()
                    .filter(|path| regular(path, true))
                {
                    let name = child
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase();
                    if !matches!(
                        name.as_str(),
                        "windows"
                            | "$recycle.bin"
                            | "system volume information"
                            | "redist"
                            | "_commonredist"
                            | ".egstore"
                            | "data"
                    ) {
                        pending.push((child, depth + 1));
                    }
                }
            }
        }
    }
}

fn add(root: &Path, runtimes: Vec<String>, scan: &mut Scan, seen: &mut BTreeSet<String>) {
    if scan.candidates.len() >= MAX_CANDIDATES || !regular(root, true) {
        return;
    }
    let Some(install_path) = canonical_text(root) else {
        return;
    };
    if !seen.insert(game_discovery::normalized_path(&install_path)) {
        return;
    }
    let recognized = game_discovery::recognize_installation(root, &runtimes);
    let name = recognized
        .as_ref()
        .map(|(_, name, _)| name.clone())
        .or_else(|| {
            root.file_name()
                .and_then(|value| value.to_str())
                .and_then(clean_name)
        });
    let Some(name) = name else {
        return;
    };
    if runtimes.is_empty() && recognized.is_none() {
        return;
    }
    use sha2::{Digest, Sha256};
    let app_id = runtimes
        .first()
        .and_then(|path| file_identity(Path::new(path)))
        .unwrap_or_else(|| {
            format!(
                "{:x}",
                Sha256::digest(game_discovery::normalized_path(&install_path).as_bytes())
            )
        });
    scan.candidates.push(Candidate {
        id: format!("disk:{app_id}"),
        client: "disk".into(),
        name,
        install_path,
        executable_candidates: runtimes,
        preferred_executable: None,
        game_id: recognized.map(|(id, _, _)| id),
        kind: "game".into(),
        app_id,
        launcher: None,
        artwork: None,
    });
}

// Windows file identities survive moves on a volume and distinguish separate
// copies of the same runtime. They are discovery evidence, never process proof.
#[cfg(windows)]
fn file_identity(path: &Path) -> Option<String> {
    use std::{ffi::c_void, os::windows::io::AsRawHandle};
    #[repr(C)]
    struct Information {
        attributes: u32,
        created_low: u32,
        created_high: u32,
        accessed_low: u32,
        accessed_high: u32,
        written_low: u32,
        written_high: u32,
        volume: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileInformationByHandle(handle: *mut c_void, information: *mut Information) -> i32;
    }
    let file = fs::File::open(path).ok()?;
    let mut information = std::mem::MaybeUninit::<Information>::uninit();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) } == 0 {
        return None;
    }
    let information = unsafe { information.assume_init() };
    Some(format!(
        "file-{:08x}-{:08x}{:08x}",
        information.volume, information.index_high, information.index_low
    ))
}
#[cfg(not(windows))]
fn file_identity(_path: &Path) -> Option<String> {
    None
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn moving_a_runtime_preserves_discovery_identity_and_copying_creates_an_instance() {
        let directory = crate::test_directory();
        let first = directory.path().join("Game.exe");
        let moved = directory.path().join("Moved.exe");
        let copy = directory.path().join("Copy.exe");
        fs::write(&first, b"fixture").unwrap();
        let identity = file_identity(&first).unwrap();
        fs::rename(&first, &moved).unwrap();
        assert_eq!(file_identity(&moved), Some(identity.clone()));
        fs::copy(&moved, &copy).unwrap();
        assert_ne!(file_identity(&copy), Some(identity));
    }
}
