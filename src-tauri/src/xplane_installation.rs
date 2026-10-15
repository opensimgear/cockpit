//! Installation metadata is read from X-Plane's own bounded version catalogue.
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

pub(crate) fn version(executable: &Path) -> Result<u64, String> {
    let root = executable
        .parent()
        .ok_or("Choose the X-Plane executable.")?;
    let catalogue = root.join("Resources/plugins/DataRefs.txt");
    let mut header = String::new();
    fs::File::open(&catalogue)
        .map_err(|e| format!("Cannot read the installation's Resources/plugins/DataRefs.txt: {e}"))?
        .take(256)
        .read_to_string(&mut header)
        .map_err(|e| e.to_string())?;
    // Laminar's generated catalogue starts with format and simulator version.
    let first = header
        .lines()
        .next()
        .unwrap_or("")
        .trim_start_matches('\u{feff}');
    let fields: Vec<_> = first.split_whitespace().collect();
    let version = fields
        .get(1)
        .and_then(|v| v.parse::<u64>().ok())
        .map(|v| v / 10000);
    if fields.first() != Some(&"2") || !matches!(version, Some(11 | 12)) {
        return Err("Cannot identify X-Plane 11/12 from its installed DataRefs.txt header. Repair the installation or start it once and rescan.".into());
    }
    Ok(version.unwrap())
}

pub(crate) fn roots() -> Vec<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(|home| roots_from_home(&PathBuf::from(home)))
        .unwrap_or_default()
}
fn roots_from_home(home: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let base = if cfg!(windows) {
        home.join("AppData/Local")
    } else if cfg!(target_os = "macos") {
        home.join("Library/Preferences")
    } else {
        home.join(".x-plane")
    };
    for version in [11, 12] {
        if let Ok(file) = fs::File::open(base.join(format!("x-plane_install_{version}.txt"))) {
            let mut text = String::new();
            if file.take(65537).read_to_string(&mut text).is_ok() && text.len() <= 65536 {
                result.extend(
                    text.lines()
                        .take(64)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(PathBuf::from)
                        .filter(|p| p.is_absolute()),
                );
            }
        }
        result.push(home.join("Desktop").join(format!("X-Plane {version}")));
    }
    result
}

pub(crate) fn available(executable: &Path) -> Result<std::collections::BTreeSet<String>, String> {
    let path = executable
        .parent()
        .ok_or("Choose an X-Plane executable.")?
        .join("Resources/plugins/DataRefs.txt");
    let mut text = String::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(4 * 1024 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    if text.len() > 4 * 1024 * 1024 {
        return Err("X-Plane dataref catalogue exceeds 4 MiB.".into());
    }
    Ok(text
        .lines()
        .skip(1)
        .filter_map(|s| s.split_whitespace().next())
        .filter(|s| s.starts_with("sim/"))
        .map(str::to_owned)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standalone_lists_are_bounded_and_accept_multiple_absolute_copies() {
        let root = std::env::temp_dir().join(format!("osg-xplane-roots-{}", uuid::Uuid::new_v4()));
        let base = if cfg!(windows) {
            root.join("AppData/Local")
        } else if cfg!(target_os = "macos") {
            root.join("Library/Preferences")
        } else {
            root.join(".x-plane")
        };
        fs::create_dir_all(&base).unwrap();
        let first = root.join("X-Plane 11");
        let second = root.join("Backup 11");
        fs::write(
            base.join("x-plane_install_11.txt"),
            format!("{}\n{}\nrelative\n\n", first.display(), second.display()),
        )
        .unwrap();
        let found = roots_from_home(&root);
        assert!(found.contains(&first));
        assert!(found.contains(&second));
        assert!(!found.contains(&PathBuf::from("relative")));
        fs::write(base.join("x-plane_install_12.txt"), "x".repeat(65537)).unwrap();
        assert_eq!(roots_from_home(&root), found);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn catalogue_versions_are_read_from_the_selected_installation() {
        let root = std::env::temp_dir().join(format!("osg-xplane-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("Resources/plugins")).unwrap();
        let exe = root.join("X-Plane.exe");
        fs::write(&exe, b"fixture").unwrap();
        for v in [11, 12] {
            fs::write(
                root.join("Resources/plugins/DataRefs.txt"),
                format!("2 {v}0050\nsim/time/paused int n bool\n"),
            )
            .unwrap();
            assert_eq!(version(&exe).unwrap(), v);
            assert!(available(&exe).unwrap().contains("sim/time/paused"));
        }
        for header in ["", "2 100000", "3 120000", "2 bad"] {
            fs::write(root.join("Resources/plugins/DataRefs.txt"), header).unwrap();
            assert!(version(&exe).is_err());
        }
        fs::remove_dir_all(root).unwrap();
        assert!(version(&exe).is_err());
    }
}
