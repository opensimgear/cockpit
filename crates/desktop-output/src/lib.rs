//! Independent desktop output. Construction and recording never inject input.
pub mod audit;
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

/// Windows output still requires local enablement and an owned native session.
pub const QUALIFIED: bool = cfg!(windows);
pub const QUALIFICATION_REASON: &str =
    "Native keyboard and mouse output is available on Windows only.";
pub fn qualification_gate() -> bool {
    QUALIFIED
}
#[cfg(test)]
mod tests;

#[cfg(test)]
#[test]
fn windows_output_availability_does_not_depend_on_development_build_flags() {
    assert_eq!(qualification_gate(), cfg!(windows));
}
