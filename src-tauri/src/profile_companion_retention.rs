//! Native saved-profile references fence operator publication compaction.
use super::*;
use std::collections::BTreeSet;

fn digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn with_saved_references(
    profiles: &Mutex<osg_profiles::service::Service>,
    request: &Value,
    prune: impl FnOnce(&Value) -> Result<Value, String>,
) -> Result<Value, String> {
    let supplied = request["companion"]["hashes"].as_array().filter(|hashes| hashes.len() <= 100)
        .ok_or("Companion pruning requires at most 100 explicit retention hashes.")?;
    let mut hashes = BTreeSet::new();
    for value in supplied {
        let hash = value.as_str().filter(|hash| digest(hash)).ok_or("Invalid companion retention hash.")?;
        hashes.insert(hash.to_owned());
    }
    // Profile Save/Delete cannot change the reference set between capture and
    // the native Hub's prune. The Hub separately retains assignments, active
    // sessions and profile ownership baselines.
    let profiles = profiles.lock().unwrap_or_else(|poison| poison.into_inner());
    let references = profiles.companion_references()?;
    hashes.extend(references["hashes"].as_array().ok_or("Native saved companion references are unavailable.")?
        .iter().map(|hash| hash.as_str().expect("validated native publication hash").to_owned()));
    if hashes.len() > 100 {
        return Err("Saved profiles retain more publications than the bounded prune operation can carry. Preserve them; remove unused saved references before compacting.".into());
    }
    let mut protected = request.clone();
    protected["companion"]["hashes"] = json!(hashes);
    let result = prune(&protected);
    drop(profiles);
    result
}

impl GatewayManager {
    pub(super) fn companion_request(&self, request: &Value) -> Result<Value, String> {
        if request["command"] != "companion_prune" {
            return self.companion_host.request(request);
        }
        with_saved_references(&self.profiles, request, |protected| self.companion_host.request(protected))
    }
}

#[cfg(test)]
#[path = "profile_companion_retention_tests.rs"]
mod tests;
