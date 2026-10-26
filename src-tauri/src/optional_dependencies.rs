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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConfigurationApp {
    HidHide,
    VJoy,
}

impl ConfigurationApp {
    fn relative_path(self) -> &'static str {
        match self {
            Self::HidHide => "Nefarius Software Solutions/HidHide/x64/HidHideClient.exe",
            Self::VJoy => "vJoy/x64/vJoyConf.exe",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::HidHide => "HidHide",
            Self::VJoy => "vJoy configuration",
        }
    }

    fn missing_message(self) -> &'static str {
        match self {
            Self::HidHide => {
                "HidHide configuration client is not installed. Set up HidHide in Settings first."
            }
            Self::VJoy => "vJoy configuration is not installed. Set up vJoy in Settings first.",
        }
    }
}

fn configuration_path(root: &Path, app: ConfigurationApp) -> std::path::PathBuf {
    root.join(app.relative_path())
}

fn configuration_outcome(
    app: ConfigurationApp,
    success: bool,
    code: Option<i32>,
    stderr: &str,
) -> Result<(), String> {
    if success {
        return Ok(());
    }
    if code == Some(1223) {
        return Err(format!(
            "Windows elevation for {} was cancelled. You can try again when ready.",
            app.label()
        ));
    }
    let detail = stderr.trim();
    if detail.is_empty() {
        Err(format!(
            "Could not open {} (PowerShell exit code {}).",
            app.label(),
            code.map_or_else(|| "unavailable".to_owned(), |value| value.to_string())
        ))
    } else {
        Err(format!("Could not open {}: {detail}", app.label()))
    }
}

#[cfg(windows)]
const CONFIG_LAUNCH_SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
try {
    Start-Process -FilePath $env:OSG_CONFIG_APP -Verb RunAs | Out-Null
    exit 0
} catch {
    if ($_.Exception.NativeErrorCode -eq 1223 -or $_.Exception.HResult -eq -2147023673) { exit 1223 }
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}"#;

/// Ask Windows to elevate only one of the installed, fixed-path configuration clients.
/// The executable path is passed to PowerShell as environment data, not interpolated
/// into its command text. This lets tools that require administrator rights show UAC.
#[cfg(windows)]
fn launch_configuration(app: ConfigurationApp) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    let root = std::env::var_os("ProgramW6432")
        .or_else(|| std::env::var_os("ProgramFiles"))
        .ok_or("Windows Program Files directory is unavailable.")?;
    let path = configuration_path(Path::new(&root), app);
    if !path.is_file() {
        return Err(app.missing_message().into());
    }
    let system =
        std::env::var_os("SystemRoot").ok_or("Windows system directory is unavailable.")?;
    let result = std::process::Command::new(
        Path::new(&system).join("System32/WindowsPowerShell/v1.0/powershell.exe"),
    )
    .args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        CONFIG_LAUNCH_SCRIPT,
    ])
    .env("OSG_CONFIG_APP", &path)
    .creation_flags(0x08000000)
    .output()
    .map_err(|error| format!("Could not request elevation for {}: {error}", app.label()))?;

    configuration_outcome(
        app,
        result.status.success(),
        result.status.code(),
        &String::from_utf8_lossy(&result.stderr),
    )
}

#[cfg(not(windows))]
fn launch_configuration(_app: ConfigurationApp) -> Result<(), String> {
    Err("Configuration tools are available in the Windows desktop app.".into())
}

/// Open only the installed upstream configuration client; never accept a caller path.
pub(crate) fn open_hidhide() -> Result<(), String> {
    launch_configuration(ConfigurationApp::HidHide)
}

/// Open the installed vJoy layout editor from its fixed upstream location.
pub(crate) fn open_vjoy() -> Result<(), String> {
    launch_configuration(ConfigurationApp::VJoy)
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

    #[test]
    fn configuration_tools_resolve_only_the_expected_installed_paths() {
        let root = Path::new("C:/Program Files");
        assert_eq!(
            configuration_path(root, ConfigurationApp::VJoy),
            root.join("vJoy/x64/vJoyConf.exe")
        );
        assert_eq!(
            configuration_path(root, ConfigurationApp::HidHide),
            root.join("Nefarius Software Solutions/HidHide/x64/HidHideClient.exe")
        );
        assert!(ConfigurationApp::VJoy
            .missing_message()
            .contains("Set up vJoy"));
        assert!(ConfigurationApp::HidHide
            .missing_message()
            .contains("Set up HidHide"));
    }

    #[test]
    fn configuration_launch_reports_success_cancellation_and_failures() {
        assert!(configuration_outcome(ConfigurationApp::VJoy, true, Some(0), "").is_ok());
        assert!(
            configuration_outcome(ConfigurationApp::VJoy, false, Some(1223), "")
                .unwrap_err()
                .contains("cancelled")
        );
        assert!(
            configuration_outcome(ConfigurationApp::VJoy, false, Some(1), "access denied")
                .unwrap_err()
                .contains("access denied")
        );
        assert!(
            configuration_outcome(ConfigurationApp::HidHide, false, None, "")
                .unwrap_err()
                .contains("exit code unavailable")
        );
    }

    #[cfg(windows)]
    #[test]
    fn configuration_launch_hands_off_path_and_reports_mocked_uac_cancellation() {
        use std::process::Command;

        let system = std::env::var_os("SystemRoot").expect("Windows system directory");
        let log =
            std::env::temp_dir().join(format!("osg-config-launch-{}.log", std::process::id()));
        let prelude = r#"function Start-Process {
    param([string]$FilePath, [string]$Verb)
    Set-Content -LiteralPath $env:OSG_TEST_LOG -Value ($FilePath + '|' + $Verb)
    if ($env:OSG_TEST_CANCEL -eq '1') { throw [System.Runtime.InteropServices.Marshal]::GetExceptionForHR(-2147023673) }
}"#;
        let script = format!("{prelude}\n{CONFIG_LAUNCH_SCRIPT}");
        let run = |cancel: bool| {
            Command::new(Path::new(&system).join("System32/WindowsPowerShell/v1.0/powershell.exe"))
                .args([
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    &script,
                ])
                .env("OSG_CONFIG_APP", r"C:\Program Files\Fake Config\tool.exe")
                .env("OSG_TEST_LOG", &log)
                .env("OSG_TEST_CANCEL", if cancel { "1" } else { "0" })
                .output()
                .expect("run mocked PowerShell launcher")
        };

        let started = run(false);
        assert!(
            started.status.success(),
            "{}",
            String::from_utf8_lossy(&started.stderr)
        );
        assert_eq!(
            std::fs::read_to_string(&log).unwrap().trim(),
            r"C:\Program Files\Fake Config\tool.exe|RunAs"
        );

        let cancelled = run(true);
        assert_eq!(cancelled.status.code(), Some(1223));
        assert!(configuration_outcome(
            ConfigurationApp::VJoy,
            cancelled.status.success(),
            cancelled.status.code(),
            &String::from_utf8_lossy(&cancelled.stderr)
        )
        .unwrap_err()
        .contains("cancelled"));
        std::fs::remove_file(log).unwrap();
    }
}
