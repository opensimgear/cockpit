//! Scope is explicit. Unsupported visibility never falls back to broader blocking.
use serde::Serialize;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Linux,
    Macos,
    Unsupported,
}
pub fn current() -> Platform {
    if cfg!(windows) {
        Platform::Windows
    } else if cfg!(target_os = "linux") {
        Platform::Linux
    } else if cfg!(target_os = "macos") {
        Platform::Macos
    } else {
        Platform::Unsupported
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mode {
    pub mode: &'static str,
    pub label: &'static str,
    pub supported: bool,
    pub qualified: bool,
    pub reason: &'static str,
}
pub fn modes(platform: Platform) -> Vec<Mode> {
    let windows = platform == Platform::Windows;
    let broad = matches!(platform, Platform::Linux | Platform::Macos);
    vec![
        Mode {
            mode: "visible",
            label: "Visible",
            supported: true,
            qualified: true,
            reason:
                "No owned suppression is requested. Existing external policies remain unchanged.",
        },
        Mode {
            mode: "hide-all",
            label: "Hide from all applications",
            supported: windows,
            qualified: false,
            reason: if windows {
                "HidHide global policy and capture exception require native qualification."
            } else {
                "A capture grab/seizure does not establish enumeration hiding or block every alternative input path."
            },
        },
        Mode {
            mode: "hide-from-process",
            label: "Hide from a specific process",
            supported: windows,
            qualified: false,
            reason: if windows {
                "The saved executable path applies to every matching instance; HidHide shares one global policy."
            } else if platform == Platform::Linux {
                "Only a separately qualified controlled-launch boundary can isolate selected device nodes. Already-running applications cannot be retrofitted."
            } else {
                "Per-process controller hiding is unavailable on this platform."
            },
        },
        Mode {
            mode: "block-original-input",
            label: "Block original input",
            supported: broad,
            qualified: false,
            reason: if platform == Platform::Linux {
                "An evdev grab suppresses event delivery through that handle's device. It does not hide enumeration, hidraw or raw USB access. Native scope and cleanup remain unqualified."
            } else if platform == Platform::Macos {
                "IOHID seizure is broad input suppression, not per-process hiding. Supported devices, alternative paths and cleanup remain unqualified."
            } else {
                "Use this platform's qualified visibility adapter; broader suppression is not substituted."
            },
        },
    ]
}
pub fn validate(platform: Platform, mode: &str) -> Result<(), String> {
    let m = modes(platform)
        .into_iter()
        .find(|m| m.mode == mode)
        .ok_or("Unknown visibility scope.")?;
    if !m.supported {
        return Err(m.reason.into());
    }
    Ok(())
}
pub fn reason(platform: Platform) -> &'static str {
    match platform{
 Platform::Windows=>"Device hiding is unavailable until native visibility and recovery qualification is complete. Saving a selection does not hide anything.",
 Platform::Linux=>"Linux input suppression and controlled-launch isolation remain unqualified. Event blocking does not imply device hiding. Saving a selection does not apply it.",
 Platform::Macos=>"macOS broad input suppression remains unqualified. Per-process hiding is unsupported. Saving a selection does not apply it.",
 Platform::Unsupported=>"No visibility adapter is available for this platform."}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policies_never_broaden_scope_and_unqualified_actions_stay_disabled() {
        let _ = current();
        for p in [
            Platform::Windows,
            Platform::Linux,
            Platform::Macos,
            Platform::Unsupported,
        ] {
            assert!(!reason(p).is_empty());
            assert!(validate(p, "visible").is_ok());
            assert!(validate(p, "missing").is_err());
            for m in modes(p) {
                assert_eq!(validate(p, m.mode).is_ok(), m.supported);
                assert_eq!(m.qualified, m.mode == "visible");
            }
        }
        assert!(validate(Platform::Linux, "hide-all").is_err());
        assert!(validate(Platform::Linux, "hide-from-process").is_err());
        assert!(validate(Platform::Macos, "hide-from-process").is_err());
        assert!(validate(Platform::Windows, "block-original-input").is_err());
    }
}
