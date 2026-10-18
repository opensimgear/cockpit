//! Independent desktop output. Construction and recording never inject input.
pub mod coordinator;
pub mod entry;
pub mod keys;
pub mod protocol;
pub mod reducer;
pub mod transport;
#[cfg(windows)]
pub mod windows;
pub mod worker;
pub use coordinator::{Coordinator, Snapshot};
pub use protocol::{ProcessIdentity, Scope};

/// Production output remains closed until the native qualification matrix passes.
pub const QUALIFIED: bool = false;
pub const QUALIFICATION_REASON: &str =
    "Windows keyboard and mouse output has not completed native/game qualification.";
pub fn qualification_gate() -> bool {
    QUALIFIED
        || (cfg!(all(windows, debug_assertions))
            && option_env!("OSG_DEV_DESKTOP_INPUT") == Some("1"))
}
#[cfg(test)]
mod tests;
