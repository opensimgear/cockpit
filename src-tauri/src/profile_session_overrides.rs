//! Peripheral publication overrides require an owned runtime capability.
//! Persistent Enable/Disable and direct USB are never substitute capabilities.
#[path = "profile_publication.rs"]
mod publication;
pub(crate) use publication::{advertised, Overrides, Staged, VerifiedPublication, KIND};
#[cfg(test)]
#[path = "profile_session_overrides_tests.rs"]
mod tests;
