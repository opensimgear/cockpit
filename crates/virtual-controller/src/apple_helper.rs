//! Optional separately installed Apple helper. No approved signed release exists yet.
use crate::{
    session::{Channel, Factory},
    supervisor::Worker,
};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
pub const QUALIFIED: bool = false;
pub const REASON:&str="macOS virtual output is disabled until a signed, notarized helper with Apple's virtual-HID entitlement and native game/lifetime qualification is approved. Recording preview remains available.";
pub const ARGUMENT: &str = "--osg-macos-hid-feeder";
pub const PATH: &str = "/Library/Application Support/OpenSimGear/Cockpit/HID/osg-macos-hid-helper";
pub const ENTITLEMENT: &str = "com.apple.developer.hid.virtual.device";
/// Compile-time pin of the exact externally qualified signed/notarized artifact.
/// Never accept a pin or qualification claim from IPC, settings or environment.
const RELEASE_SHA256: Option<&str> = None;
const MAX_HELPER: u64 = 32 * 1024 * 1024;
pub fn readiness() -> serde_json::Value {
    serde_json::json!({"backend":"macos-hid","state":"unavailable","qualified":false,"helperPresent":Path::new(PATH).is_file(),"approvedRelease":RELEASE_SHA256.is_some(),"reason":REASON,"limits":{"axes":8,"buttons":32,"hats":4,"targets":8}})
}
fn verify_file(path: &Path, hash: &str) -> Result<(), String> {
    if hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("Invalid approved helper fingerprint.".into());
    }
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|e| format!("Install the approved Apple helper: {e}"))?;
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > MAX_HELPER {
        return Err("Apple helper must be a bounded regular executable, not a symlink.".into());
    }
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = vec![];
    Read::by_ref(&mut file)
        .take(MAX_HELPER + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 != metadata.len() || format!("{:x}", Sha256::digest(&bytes)) != hash {
        return Err(
            "Apple helper differs from the approved signed release; reinstall it explicitly."
                .into(),
        );
    }
    Ok(())
}
#[cfg(target_os = "macos")]
fn validate_installation(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    for component in path.ancestors() {
        let m = std::fs::symlink_metadata(component).map_err(|e| e.to_string())?;
        if m.file_type().is_symlink() || m.uid() != 0 || m.mode() & 0o022 != 0 {
            return Err("The approved helper installation must be owned by root and not writable by other users.".into());
        }
    }
    Ok(())
}
fn executable() -> Result<PathBuf, String> {
    if !QUALIFIED {
        return Err(REASON.into());
    }
    let hash = RELEASE_SHA256.ok_or("No signed Apple helper release has been approved.")?;
    let path = PathBuf::from(PATH);
    verify_file(&path, hash)?;
    #[cfg(target_os = "macos")]
    validate_installation(&path)?;
    if !cfg!(target_os = "macos") {
        return Err("The Apple HID helper requires macOS.".into());
    }
    Ok(path)
}
pub fn factory() -> Factory {
    Arc::new(|| {
        let path = executable()?;
        let mut worker = Worker::spawn(
            &path,
            &[ARGUMENT.into()],
            "macos-hid",
            Duration::from_secs(3),
        )?;
        worker.response_timeout(Duration::from_millis(100));
        Ok(Box::new(worker) as Box<dyn Channel>)
    })
}
fn failed(mut output: impl Write, error: &str) -> i32 {
    let _ = writeln!(
        output,
        "{}",
        serde_json::json!({"ok":false,"backend":"macos-hid","protocol":1,"error":error.chars().take(512).collect::<String>()})
    );
    1
}
pub fn run(args: &[String]) -> i32 {
    if args == ["--inspect"] {
        let _ = writeln!(std::io::stdout().lock(), "{}", readiness());
        return 0;
    }
    if args != [ARGUMENT] {
        return failed(
            std::io::stdout().lock(),
            "Use the private feeder or --inspect entry.",
        );
    }
    #[cfg(target_os = "macos")]
    {
        match crate::macos_hid::Native::open() {
            Ok(backend) => match crate::worker::serve(
                crate::uinput::Output::new(backend),
                std::io::BufReader::new(std::io::stdin()),
                std::io::stdout(),
                "macos-hid",
            ) {
                Ok(()) => 0,
                Err(_) => 1,
            },
            Err(error) => failed(std::io::stdout().lock(), &error),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        failed(
            std::io::stdout().lock(),
            "The Apple HID helper requires macOS.",
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_unsigned_helper_or_environment_claim_can_enable_creation() {
        assert_eq!(readiness()["qualified"], false);
        assert_eq!(readiness()["approvedRelease"], false);
        assert!(factory()().is_err());
        #[cfg(target_os = "macos")]
        {
            validate_installation(Path::new("/")).unwrap();
            let dir = tempfile::tempdir().unwrap();
            assert!(validate_installation(dir.path()).is_err());
        }
        assert_eq!(run(&[]), 1);
        assert_eq!(run(&[ARGUMENT.into()]), 1);
        assert_eq!(run(&["--inspect".into()]), 0);
        let mut bytes = vec![];
        assert_eq!(failed(&mut bytes, &"x".repeat(800)), 1);
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"].as_str().unwrap().len(), 512);
    }
    #[test]
    fn approved_artifact_checks_preserve_missing_changed_oversized_and_symlinked_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("helper");
        let hash = format!("{:x}", Sha256::digest(b"signed release fixture"));
        assert!(verify_file(&path, "bad").is_err());
        assert!(verify_file(&path, &hash).is_err());
        std::fs::write(&path, []).unwrap();
        assert!(verify_file(&path, &hash).is_err());
        std::fs::write(&path, b"signed release fixture").unwrap();
        verify_file(&path, &hash).unwrap();
        assert!(verify_file(&path, &"0".repeat(64)).is_err());
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(MAX_HELPER + 1)
            .unwrap();
        assert!(verify_file(&path, &hash).is_err());
        assert!(verify_file(dir.path(), &hash).is_err());
        #[cfg(unix)]
        {
            let link = dir.path().join("link");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(verify_file(&link, &hash).is_err());
        }
    }
}
