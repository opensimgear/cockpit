//! Explicit, interactive setup of repository-pinned Windows dependencies.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path, sync::Mutex};

const MANIFEST: &str = include_str!("../resources/dependencies/manifest.json");
static SETUP: Mutex<()> = Mutex::new(());

fn catalog() -> Vec<Value> {
    serde_json::from_str(MANIFEST).expect("valid bundled dependency manifest")
}

pub(crate) fn list() -> Value {
    json!({"supported": cfg!(all(target_os = "windows", target_arch = "x86_64")), "dependencies": catalog()})
}

/// Open only the installed upstream configuration client; never accept a caller path.
pub(crate) fn open_hidhide() -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let root = std::env::var_os("ProgramW6432").or_else(|| std::env::var_os("ProgramFiles")).ok_or("Windows Program Files directory is unavailable.")?;
        let path = Path::new(&root).join("Nefarius Software Solutions/HidHide/x64/HidHideClient.exe");
        if !path.is_file() { return Err("HidHide configuration client is not installed. Set up HidHide in Settings first.".into()); }
        std::process::Command::new(path).creation_flags(0x08000000).spawn().map_err(|e| format!("Could not open HidHide: {e}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    { Err("HidHide management is available in the Windows desktop app.".into()) }
}

/// Open the installed vJoy layout editor from its fixed upstream location.
pub(crate) fn open_vjoy() -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let root = std::env::var_os("ProgramW6432").or_else(|| std::env::var_os("ProgramFiles")).ok_or("Windows Program Files directory is unavailable.")?;
        let path = Path::new(&root).join("vJoy/x64/vJoyConf.exe");
        if !path.is_file() { return Err("vJoy configuration is not installed. Set up vJoy in Settings first.".into()); }
        std::process::Command::new(path).creation_flags(0x08000000).spawn().map_err(|e| format!("Could not open vJoy configuration: {e}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    { Err("vJoy configuration is available in the Windows desktop app.".into()) }
}

fn dependency(id: &str) -> Result<Value, String> {
    catalog()
        .into_iter()
        .find(|entry| entry["id"] == id)
        .ok_or_else(|| "Unknown optional dependency.".into())
}

fn verify(mut file: File, entry: &Value) -> Result<File, String> {
    let size = entry["size"].as_u64().ok_or("Missing installer size.")?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() != size {
        return Err("Bundled installer size is incorrect. Reinstall Cockpit.".into());
    }
    let mut digest = Sha256::new();
    let mut remaining = (&mut file).take(size + 1);
    let mut buffer = [0u8; 65536];
    loop {
        let count = remaining.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    if format!("{:x}", digest.finalize()) != entry["sha256"].as_str().unwrap_or("") {
        return Err("Bundled installer checksum is incorrect. Reinstall Cockpit.".into());
    }
    Ok(file)
}

fn outcome(code: Option<i32>) -> Result<&'static str, String> {
    match code {
        Some(0) => Ok("Setup finished. Complete any Windows restart requested by the installer, then inspect the dependency in Cockpit. Installation does not enable mapping or visibility controls."),
        Some(3010) | Some(1641) => Ok("Setup requires a Windows restart. Restart Windows before inspecting the dependency in Cockpit."),
        Some(1602) | Some(1223) => Err("Setup was cancelled. You can run it again when ready.".into()),
        Some(code) => Err(format!("Setup did not complete (exit code {code}). Review the installer and retry if needed.")),
        None => Err("Setup ended without an exit code.".into()),
    }
}

pub(crate) fn install(resources: &Path, id: &str) -> Result<&'static str, String> {
    let entry = dependency(id)?;
    if !cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        return Err("These installers require the Windows x64 desktop app.".into());
    }
    let _guard = SETUP
        .try_lock()
        .map_err(|_| "Another dependency installer is already running.")?;
    let path = resources.join("dependencies").join(
        entry["filename"]
            .as_str()
            .ok_or("Missing installer filename.")?,
    );
    #[cfg(windows)]
    {
        use std::os::windows::{fs::OpenOptionsExt, process::CommandExt};
        // Deny replacement/write between hashing and the end of setup.
        let file = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .map_err(|e| format!("Bundled installer is unavailable. Reinstall Cockpit: {e}"))?;
        let _verified = verify(file, &entry)?;
        let system =
            std::env::var_os("SystemRoot").ok_or("Windows system directory is unavailable.")?;
        // Paths are passed as data. The upstream installer owns its interactive
        // wizard, Windows authorization and restart flow; no silent switches.
        let script = r#"$ErrorActionPreference = 'Stop'
try {
    $signature = Get-AuthenticodeSignature -LiteralPath $env:OSG_DEPENDENCY_INSTALLER
    if ($signature.Status -ne 'Valid') { throw 'Installer publisher signature could not be verified.' }
    $process = Start-Process -FilePath $env:OSG_DEPENDENCY_INSTALLER -Verb RunAs -Wait -PassThru
    exit $process.ExitCode
} catch {
    if ($_.Exception.NativeErrorCode -eq 1223) { exit 1223 }
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}"#;
        let result = std::process::Command::new(
            Path::new(&system).join("System32/WindowsPowerShell/v1.0/powershell.exe"),
        )
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .env("OSG_DEPENDENCY_INSTALLER", &path)
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| format!("Could not start dependency setup: {e}"))?;
        if !result.status.success() && !result.stderr.is_empty() {
            return Err(format!(
                "Setup could not complete: {}",
                String::from_utf8_lossy(&result.stderr).trim()
            ));
        }
        outcome(result.status.code())
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("These installers require Windows.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_installers_match_the_catalog() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/dependencies");
        for entry in catalog() {
            let filename = entry["filename"].as_str().unwrap();
            assert_eq!(Path::new(filename).file_name().unwrap(), filename);
            verify(File::open(root.join(filename)).unwrap(), &entry).unwrap();
        }
    }

    #[test]
    fn unknown_dependencies_and_modified_installers_are_rejected() {
        assert!(dependency("../../untrusted.exe").is_err());
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/dependencies");
        let mut entry = dependency("vjoy").unwrap();
        let path = root.join(entry["filename"].as_str().unwrap());
        entry["sha256"] = json!("0".repeat(64));
        assert!(verify(File::open(&path).unwrap(), &entry)
            .unwrap_err()
            .contains("checksum"));
        entry["size"] = json!(1);
        assert!(verify(File::open(path).unwrap(), &entry)
            .unwrap_err()
            .contains("size"));
    }

    #[test]
    fn setup_exit_codes_do_not_claim_driver_readiness() {
        assert!(outcome(Some(0)).unwrap().contains("inspect"));
        assert!(outcome(Some(3010)).unwrap().contains("restart"));
        assert!(outcome(Some(1641)).unwrap().contains("restart"));
        assert!(outcome(Some(1223)).unwrap_err().contains("cancelled"));
        assert!(outcome(Some(1602)).is_err());
        assert!(outcome(Some(5)).is_err());
        assert!(outcome(None).is_err());
    }
}
