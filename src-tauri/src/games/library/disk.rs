//! User-selected containers: only their immediate children become game folders.
use super::*;

#[cfg(test)]
#[path="disk_scan_tests.rs"]
mod scan_tests;

fn path_key(path: &Path) -> String {
    let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    game_discovery::normalized_path(&resolved.to_string_lossy()).trim_end_matches('\\').to_owned()
}
fn within(path: &str, root: &str) -> bool {
    path == root || path.starts_with(&format!("{root}\\"))
}
fn overlaps(path: &Path, occupied: &[PathBuf]) -> bool {
    let path = path_key(path);
    occupied.iter().any(|root| {let root=path_key(root);within(&path,&root)||within(&root,&path)})
}

pub(super) fn scan(extra: &[PathBuf], occupied: &[PathBuf], scan: &mut Scan) {
    let mut roots = extra.iter().map(|path|fs::canonicalize(path).unwrap_or_else(|_|path.clone())).collect::<Vec<_>>();
    roots.sort();roots.dedup();
    let mut seen = BTreeSet::new();
    for root in roots {
        if !regular(&root, true) {
            if !fs::metadata(&root).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) {
                scan.diagnostic(format!("Added discovery folder is inaccessible or is not a directory: {}",root.display()));
            }
            continue;
        }
        let root_key=path_key(&root);
        if occupied.iter().any(|path|within(&root_key,&path_key(path)))
            || scan.candidates.iter().any(|game|within(&root_key,&path_key(Path::new(&game.install_path)))) {continue;}
        // Executable lookup may descend inside a game, but folder discovery never does.
        for child in sorted_children(&root).into_iter().filter(|path|regular(path,true)).take(4096) {
            if scan.candidates.len() >= MAX_CANDIDATES {return;}
            let name=child.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
            if matches!(name.as_str(),"windows"|"$recycle.bin"|"system volume information"|"redist"|"_commonredist"|".egstore"|"steamapps"|"windowsapps")
                || overlaps(&child,occupied) {continue;}
            let runtimes=executable_candidates(&child,None);
            add(&child,runtimes,scan,&mut seen);
        }
    }
}

pub(super) fn add(root: &Path, runtimes: Vec<String>, scan: &mut Scan, seen: &mut BTreeSet<String>) {
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
