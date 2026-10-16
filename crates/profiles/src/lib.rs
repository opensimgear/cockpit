//! Portable unified profile documents. Parsing and validation never acquire a device.
pub mod model;
pub mod service;
pub mod session;
pub mod store;
pub mod validation;

pub use model::*;
pub use validation::{semantic_hash, validate_document};

pub const DEFAULT_PROFILE_ID: &str = "00000000-0000-4000-8000-000000000001";
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
