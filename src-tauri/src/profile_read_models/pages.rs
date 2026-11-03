//! Immutable, process-local rule blobs. Hashes are issued only by the producer;
//! readers have no path, filesystem, inventory, or command authority.
use super::*;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};

const MAX_BLOBS: usize = 4;
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_BLOB_BYTES: usize = 3 * 1024 * 1024;
const CHUNK_BYTES: usize = 8192;
const INLINE_BYTES: usize = 32 * 1024;

#[derive(Default)]
struct State {
    blobs: BTreeMap<String, Arc<str>>,
    order: VecDeque<String>,
    bytes: usize,
    pinned: Option<String>,
}
#[derive(Default)]
pub(super) struct Pages { state: Mutex<State> }
impl Pages {
    fn install(&self, bytes: String, count: usize) -> Result<Value, String> {
        if bytes.len() > MAX_BLOB_BYTES { return Err("Profile rule read model exceeds its blob bound.".into()); }
        let id = format!("sha256:{:x}", Sha256::digest(bytes.as_bytes()));
        let total = bytes.len();
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if !state.blobs.contains_key(&id) {
            while state.blobs.len() >= MAX_BLOBS || state.bytes + total > MAX_BYTES {
                let Some(position) = state.order.iter().position(|old| Some(old) != state.pinned.as_ref()) else { return Err("Profile rule page cache is full.".into()); };
                let oldest = state.order.remove(position).expect("selected cache entry");
                if let Some(blob) = state.blobs.remove(&oldest) { state.bytes -= blob.len(); }
            }
            state.bytes += total;
            state.order.push_back(id.clone());
            state.blobs.insert(id.clone(), Arc::from(bytes));
        }
        Ok(json!({"pages":{"id":id,"bytes":total,"count":count}}))
    }
    fn pin(&self, id: Option<&str>) { self.state.lock().unwrap_or_else(|p| p.into_inner()).pinned = id.map(str::to_owned); }
    pub(super) fn get(&self, id: &str, offset: u64) -> Result<Value, String> {
        if id.len() != 71 || !id.starts_with("sha256:")
            || !id[7..].bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
            return Err("Read-model page identity must be an issued sha256 digest.".into());
        }
        // Pin only an Arc while locked. UTF-8 slicing and response construction
        // cannot hold the cache map or any authoritative source lock.
        let blob = self.state.lock().unwrap_or_else(|p| p.into_inner()).blobs.get(id).cloned()
            .ok_or("Profile read-model pages expired. Reopen the current observation baseline.")?;
        let offset = usize::try_from(offset).map_err(|_| "Invalid read-model page offset.")?;
        if offset > blob.len() || !blob.is_char_boundary(offset) { return Err("Read-model page offset is not a valid UTF-8 boundary.".into()); }
        let mut next = (offset + CHUNK_BYTES).min(blob.len());
        while !blob.is_char_boundary(next) { next -= 1; }
        Ok(json!({"id":id,"offset":offset,"nextOffset":next,
            "total":blob.len(),"chunk":&blob[offset..next]}))
    }
    pub(super) fn clear(&self) { *self.state.lock().unwrap_or_else(|p| p.into_inner()) = State::default(); }
}

/// Retain the exact source vector for equality checks, then reuse its prepared
/// wire shape on health/emergency events without encoding or hashing rules.
#[derive(Default)]
pub(super) struct PreparedRules { previous: Option<Vec<Rule>>, wire: Value }
impl PreparedRules {
    pub(super) fn prepare(&mut self, rules: &[Rule], pages: &Pages) -> Result<(), String> {
        if self.previous.as_deref() == Some(rules) { return Ok(()); }
        let bytes = serde_json::to_string(rules).map_err(|error| error.to_string())?;
        self.wire = if bytes.len() <= INLINE_BYTES {
            serde_json::from_str(&bytes).map_err(|error| error.to_string())?
        } else { pages.install(bytes, rules.len())? };
        self.previous = Some(rules.to_vec());
        Ok(())
    }
    pub(super) fn wire(&self) -> &Value { &self.wire }
    pub(super) fn pin(&self, pages: &Pages) { pages.pin(self.wire["pages"]["id"].as_str()); }
}

#[cfg(test)]
mod tests;
