//! Read-only detection of the installed vJoy and HidHide kernel drivers.
//!
//! This deliberately inspects the fixed Windows driver files, never the bundled
//! installer manifest, so the reported version belongs to the installed driver.
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct DriverMetadata {
    installed: Option<bool>,
    version: Option<String>,
    error: Option<String>,
}

impl DriverMetadata {
    fn unsupported() -> Self {
        Self {
            installed: None,
            version: None,
            error: None,
        }
    }

    fn value(&self) -> Value {
        json!({"installed":self.installed,"version":self.version,"error":self.error})
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CachedVersion {
    length: u64,
    modified: SystemTime,
    version: String,
}

#[derive(Default)]
struct VersionCache(Mutex<HashMap<PathBuf, CachedVersion>>);

impl VersionCache {
    fn get(&self, path: &Path, length: u64, modified: SystemTime) -> Option<String> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(path)
            .filter(|entry| entry.length == length && entry.modified == modified)
            .map(|entry| entry.version.clone())
    }

    fn remove(&self, path: &Path) {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(path);
    }

    fn insert(&self, path: &Path, length: u64, modified: SystemTime, version: String) {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                path.to_owned(),
                CachedVersion {
                    length,
                    modified,
                    version,
                },
            );
    }
}

static VERSION_CACHE: OnceLock<VersionCache> = OnceLock::new();

fn inspect_driver(
    path: &Path,
    cache: &VersionCache,
    version: impl FnOnce(&Path) -> Result<String, String>,
) -> DriverMetadata {
    match std::fs::metadata(path) {
        Ok(metadata) if !metadata.is_file() => {
            cache.remove(path);
            DriverMetadata {
                installed: None,
                version: None,
                error: Some("The expected driver path is not a file.".into()),
            }
        }
        Ok(metadata) => {
            let modified = match metadata.modified() {
                Ok(modified) => modified,
                Err(error) => {
                    cache.remove(path);
                    return DriverMetadata {
                        installed: None,
                        version: None,
                        error: Some(format!("Could not read driver modification time: {error}")),
                    };
                }
            };
            if let Some(version) = cache.get(path, metadata.len(), modified) {
                return DriverMetadata {
                    installed: Some(true),
                    version: Some(version),
                    error: None,
                };
            }
            cache.remove(path);
            match version(path) {
                Ok(version) if !version.trim().is_empty() => {
                    let version: String = version.trim().chars().take(128).collect();
                    cache.insert(path, metadata.len(), modified, version.clone());
                    DriverMetadata {
                        installed: Some(true),
                        version: Some(version),
                        error: None,
                    }
                }
                Ok(_) => DriverMetadata {
                    installed: None,
                    version: None,
                    error: Some("The driver file has no version metadata.".into()),
                },
                Err(error) => DriverMetadata {
                    installed: None,
                    version: None,
                    error: Some(error),
                },
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            cache.remove(path);
            DriverMetadata {
                installed: Some(false),
                version: None,
                error: None,
            }
        }
        Err(error) => {
            cache.remove(path);
            DriverMetadata {
                installed: None,
                version: None,
                error: Some(error.to_string()),
            }
        }
    }
}

#[cfg(windows)]
fn file_version(path: &Path) -> Result<String, String> {
    use std::os::windows::process::CommandExt;

    let system =
        std::env::var_os("SystemRoot").ok_or("Windows system directory is unavailable.")?;
    let powershell = PathBuf::from(system).join("System32/WindowsPowerShell/v1.0/powershell.exe");
    // The path is passed as environment data, not interpolated into script text.
    // Select just FileVersion so subprocess output remains small and predictable.
    let script = r#"$ErrorActionPreference = 'Stop'
$item = Get-Item -LiteralPath $env:OSG_DRIVER_FILE
$version = $item.VersionInfo.FileVersion
if ([string]::IsNullOrWhiteSpace($version)) { throw 'Driver version metadata is unavailable.' }
[Console]::Out.Write($version.Trim())"#;
    let output = std::process::Command::new(powershell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .env("OSG_DRIVER_FILE", path)
        .creation_flags(0x08000000)
        .output()
        .map_err(|error| format!("Could not read driver version metadata: {error}"))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr)
            .trim()
            .chars()
            .take(256)
            .collect::<String>();
        return Err(if message.is_empty() {
            "Could not read driver version metadata.".into()
        } else {
            message
        });
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if version.is_empty() {
        Err("Driver version metadata is unavailable.".into())
    } else {
        Ok(version)
    }
}

#[cfg(not(windows))]
fn file_version(_path: &Path) -> Result<String, String> {
    Err("Driver version inspection is only available on Windows.".into())
}

fn driver_at(root: &Path, filename: &str) -> Value {
    let cache = VERSION_CACHE.get_or_init(VersionCache::default);
    inspect_driver(
        &root.join("System32/drivers").join(filename),
        cache,
        file_version,
    )
    .value()
}

pub(crate) fn unsupported() -> Value {
    json!({"supported":false,"vjoy":DriverMetadata::unsupported().value(),"hidhide":DriverMetadata::unsupported().value()})
}

pub(crate) fn inspect() -> Value {
    if !cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        return unsupported();
    }
    #[cfg(windows)]
    {
        match std::env::var_os("SystemRoot") {
            Some(root) => {
                let root = PathBuf::from(root);
                json!({"supported":true,"vjoy":driver_at(&root,"vjoy.sys"),"hidhide":driver_at(&root,"HidHide.sys")})
            }
            None => {
                let error = "Windows system directory is unavailable.";
                json!({"supported":true,
                    "vjoy":{"installed":null,"version":null,"error":error},
                    "hidhide":{"installed":null,"version":null,"error":error}})
            }
        }
    }
    #[cfg(not(windows))]
    {
        unsupported()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn missing_driver_is_reported_as_absent() {
        let root = tempfile::tempdir().unwrap();
        let cache = VersionCache::default();
        let result = inspect_driver(&root.path().join("vjoy.sys"), &cache, |_| {
            panic!("version read for absent driver")
        });
        assert_eq!(
            result,
            DriverMetadata {
                installed: Some(false),
                version: None,
                error: None
            }
        );
    }

    #[test]
    fn present_driver_reports_its_file_version() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("HidHide.sys");
        fs::write(&path, b"driver fixture").unwrap();
        let cache = VersionCache::default();
        let result = inspect_driver(&path, &cache, |_| Ok("1.2.3.4".into()));
        assert_eq!(
            result,
            DriverMetadata {
                installed: Some(true),
                version: Some("1.2.3.4".into()),
                error: None
            }
        );
    }

    #[test]
    fn version_inspection_failure_is_unknown_with_error() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("vjoy.sys");
        fs::write(&path, b"driver fixture").unwrap();
        let cache = VersionCache::default();
        let result = inspect_driver(&path, &cache, |_| {
            Err("version resource could not be read".into())
        });
        assert_eq!(
            result,
            DriverMetadata {
                installed: None,
                version: None,
                error: Some("version resource could not be read".into())
            }
        );
    }

    #[test]
    fn successful_version_reads_are_cached_until_file_metadata_changes_or_disappears() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("vjoy.sys");
        fs::write(&path, b"first driver contents").unwrap();
        let cache = VersionCache::default();
        let reads = std::cell::Cell::new(0);
        let read_version = |_: &Path| {
            let next = reads.get() + 1;
            reads.set(next);
            Ok(format!("version-{next}"))
        };

        assert_eq!(
            inspect_driver(&path, &cache, &read_version)
                .version
                .as_deref(),
            Some("version-1")
        );
        assert_eq!(
            inspect_driver(&path, &cache, &read_version)
                .version
                .as_deref(),
            Some("version-1")
        );
        assert_eq!(
            reads.get(),
            1,
            "unchanged files should use the cached version"
        );

        fs::write(&path, b"updated driver with a different length").unwrap();
        assert_eq!(
            inspect_driver(&path, &cache, &read_version)
                .version
                .as_deref(),
            Some("version-2")
        );
        assert_eq!(
            reads.get(),
            2,
            "a changed file length should trigger a fresh read"
        );

        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        fs::remove_file(&path).unwrap();
        assert_eq!(
            inspect_driver(&path, &cache, &read_version).installed,
            Some(false)
        );
        fs::write(&path, b"updated driver with a different length").unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        assert_eq!(
            inspect_driver(&path, &cache, &read_version)
                .version
                .as_deref(),
            Some("version-3")
        );
        assert_eq!(
            reads.get(),
            3,
            "removal must evict a prior entry even if the file returns unchanged"
        );
    }

    #[test]
    fn failed_version_reads_are_not_cached_and_refresh_retries() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("HidHide.sys");
        fs::write(&path, b"driver fixture").unwrap();
        let cache = VersionCache::default();
        assert!(
            inspect_driver(&path, &cache, |_| Err("temporarily unavailable".into()))
                .version
                .is_none()
        );
        let result = inspect_driver(&path, &cache, |_| Ok("2.0.0.0".into()));
        assert_eq!(result.version.as_deref(), Some("2.0.0.0"));
    }

    #[test]
    fn unsupported_platform_uses_null_driver_metadata() {
        let result = unsupported();
        assert_eq!(result["supported"], false);
        assert!(result["vjoy"]["installed"].is_null());
        assert!(result["hidhide"]["version"].is_null());
    }
}
