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

#[cfg(windows)]
mod journal_security;

#[cfg(windows)]
pub mod windows_refresh;

#[cfg(windows)]
pub mod windows_process;

pub mod output_journal;

#[cfg(windows)]
pub mod windows_coordination;

#[cfg(windows)]
pub mod windows_launcher;

#[cfg(windows)]
pub mod windows_provision_entry;

#[cfg(windows)]
pub mod windows_jobs;

pub mod descriptor;
