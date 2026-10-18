//! Scope is explicit. Unsupported visibility never falls back to broader blocking.
use serde::Serialize;

/// Explicit acceptance runs use the normal Windows debug app, never release builds.
pub const DEVELOPMENT_NATIVE_PROFILES: bool = development_native_profiles(
    cfg!(windows),
    cfg!(debug_assertions),
    option_env!("OSG_DEV_NATIVE_PROFILES"),
);
const fn development_native_profiles(windows: bool, debug: bool, value: Option<&str>) -> bool {
    if !windows || !debug {
        return false;
    }
    match value {
        Some(value) => value.len() == 1 && value.as_bytes()[0] == b'1',
        None => false,
    }
}
pub const DEVELOPMENT_REASON: &str = "Development acceptance mode: Windows vJoy output and HidHide are enabled for explicit profile Apply. Release qualification remains unchanged.";
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
            qualified: windows && DEVELOPMENT_NATIVE_PROFILES,
            reason: if windows && DEVELOPMENT_NATIVE_PROFILES {
                DEVELOPMENT_REASON
            } else if windows {
                "HidHide global policy and capture exception require native qualification."
            } else {
                "A capture grab/seizure does not establish enumeration hiding or block every alternative input path."
            },
        },
        Mode {
            mode: "hide-from-process",
            label: "Hide from a specific process",
            supported: windows,
            qualified: windows && DEVELOPMENT_NATIVE_PROFILES,
            reason: if windows && DEVELOPMENT_NATIVE_PROFILES {
                "Development acceptance mode: the saved executable path applies to every matching instance; HidHide shares one global policy. Release qualification remains unchanged."
            } else if windows {
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
    if platform == Platform::Windows && DEVELOPMENT_NATIVE_PROFILES {
        return DEVELOPMENT_REASON;
    }
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
    fn native_development_acceptance_requires_exact_opt_in_and_windows_debug() {
        for windows in [false, true] {
            for debug in [false, true] {
                for value in [None, Some(""), Some("0"), Some("true"), Some("01"), Some("1 "), Some("1")] {
                    assert_eq!(development_native_profiles(windows, debug, value),
                        windows && debug && value == Some("1"));
                }
            }
        }
        #[cfg(windows)]
        assert_eq!(super::super::windows::QUALIFIED, DEVELOPMENT_NATIVE_PROFILES);
    }
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
                assert_eq!(m.qualified, m.mode == "visible" ||
                    (p == Platform::Windows && DEVELOPMENT_NATIVE_PROFILES &&
                        matches!(m.mode, "hide-all" | "hide-from-process")));
            }
        }
        assert!(validate(Platform::Linux, "hide-all").is_err());
        assert!(validate(Platform::Linux, "hide-from-process").is_err());
        assert!(validate(Platform::Macos, "hide-from-process").is_err());
        assert!(validate(Platform::Windows, "block-original-input").is_err());
    }
}
