#[cfg(not(windows))]
mod native {
    use super::*;
    pub(super) fn roots() -> Roots {
        let mut roots = Roots::default();
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            for relative in [
                "Library/Application Support/Steam",
                ".steam/steam",
                ".local/share/Steam",
                ".var/app/com.valvesoftware.Steam/.local/share/Steam",
            ] {
                add_path(&mut roots.steam, home.join(relative));
            }
        }
        roots
    }
    pub(super) fn spawn(_: &LaunchRequest) -> Result<(), String> {
        Err("Game launch requires Windows.".into())
    }
    pub(super) fn find(_: &[String]) -> Result<Option<ProcessIdentity>, String> {
        Err("Game process monitoring requires Windows.".into())
    }
    pub(super) fn inventory(_: &[String]) -> Result<ProcessInventory, String> {
        Err("Game process monitoring requires Windows.".into())
    }
    pub(super) fn alive(_: &ProcessIdentity) -> Result<bool, String> {
        Err("Game process monitoring requires Windows.".into())
    }
}
