//! Optional virtual output. No driver is loaded merely by constructing the workspace.
pub mod entry;
pub mod feeder;
pub mod inspection;
pub mod provision;
pub mod session;
pub mod supervisor;
pub mod vjoy;
#[cfg(windows)]
pub mod windows;
pub mod worker;

#[cfg(windows)]
mod registry;

#[cfg(windows)]
pub mod windows_provision;
