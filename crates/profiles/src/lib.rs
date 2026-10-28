//! Portable unified profile documents. Parsing and validation never acquire a device.
pub mod activation;
pub mod compiler;
pub mod debug_controller;
pub mod compiler_models;
pub mod model;
pub mod resources;
pub mod resources_ports;
pub mod resources_store;
pub mod service;
pub mod session;
pub mod store;
pub mod validation;

pub use model::*;
pub use validation::{semantic_hash, validate_document};

pub const PROFILE_SCHEMA_VERSION: u32 = 2;
pub const DEFAULT_PROFILE_ID: &str = "00000000-0000-4000-8000-000000000001";
/// A single editor graph plus machine-local exact port expectations.
pub const MAX_EDITOR_REQUEST_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PROFILE_BYTES: usize = 512 * 1024;
pub const MAX_PROFILES: usize = 32;
pub const MAX_LIBRARY_BYTES: usize = 17 * 1024 * 1024;
pub const MAX_REVISION: u64 = 9_007_199_254_740_991;

pub fn default_document() -> ProfileDocument {
    ProfileDocument::empty(DEFAULT_PROFILE_ID.to_owned(), "Default".to_owned())
}

pub fn new_document(name: &str) -> Result<ProfileDocument, String> {
    let document = ProfileDocument::empty(uuid::Uuid::new_v4().to_string(), name.trim().to_owned());
    validate_document(&document)?;
    Ok(document)
}

// macOS commonly exposes its temporary directory through /var -> /private/var.
// Fixtures use a canonical temporary parent so valid-store tests do not ask the
// production store to follow that alias. Store itself must keep rejecting links.
#[cfg(test)]
fn test_directory() -> tempfile::TempDir {
    test_directory_in(&std::env::temp_dir())
}

#[cfg(test)]
fn test_directory_in(parent: &std::path::Path) -> tempfile::TempDir {
    #[cfg(unix)]
    let parent = parent
        .canonicalize()
        .expect("canonical test temporary parent");
    #[cfg(not(unix))]
    let parent = parent.to_owned();
    tempfile::tempdir_in(parent).expect("test temporary directory")
}

#[cfg(test)]
mod desktop_tests;

#[cfg(test)]
fn fixture_record(path:&std::path::Path,domain:&str,key:&str,payload:&[u8]) {
    let db=osg_persistence::Database::for_namespace(path).unwrap();
    db.commit(&[osg_persistence::Change{domain,key,expected_revision:db.revision(domain,key).unwrap(),payload:Some(payload)}]).unwrap();
}
