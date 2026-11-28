//! Durable metadata only. Cached descriptors never authorize native access.
use osg_persistence::{Database, Repository};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

mod metadata;
const MAX_SNAPSHOTS: usize = 256;
const MAX_DETAIL_BYTES: usize = 2 * 1024 * 1024;
const MAX_CATALOG_BYTES: usize = 16 * 1024 * 1024;
const DOMAIN: &str = "controllers/hid";
const KEY: &str = "last-observed";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Snapshot {
    snapshot_id: String,
    interface_id: String,
    contract_fingerprint: String,
    provenance: Value,
    observed_at: String,
    detail: Value,
}
type Saved = BTreeMap<String, Snapshot>;

pub(super) struct Catalog {
    saved: Saved,
    store: Option<Repository<Saved>>,
}

fn digest(value: &Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control)
}
fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn validate(saved: &Saved) -> Result<(), String> {
    if saved.len() > MAX_SNAPSHOTS {
        return Err("HID metadata catalog capacity reached".into());
    }
    for (identity, snapshot) in saved {
        if !valid_hash(identity)
            || snapshot.snapshot_id != *identity
            || !valid_id(&snapshot.interface_id)
            || !valid_hash(&snapshot.contract_fingerprint)
            || snapshot.observed_at.parse::<u128>().is_err()
            || !snapshot.detail.is_object()
            || snapshot.detail.to_string().len() > MAX_DETAIL_BYTES
            || metadata::sanitize(&snapshot.detail) != snapshot.detail
            || snapshot.detail["interfaceId"].as_str() != Some(&snapshot.interface_id)
            || metadata::identity(&snapshot.interface_id, &snapshot.detail) != *identity
            || metadata::provenance(&snapshot.detail) != snapshot.provenance
            || metadata::fingerprint(&snapshot.detail, &snapshot.provenance)
                != snapshot.contract_fingerprint
        {
            return Err("Invalid retained HID metadata; preserve the database for recovery".into());
        }
    }
    Ok(())
}

impl Catalog {
    pub(super) fn new(path: Option<&Path>) -> Result<Self, String> {
        let store = path
            .map(|path| {
                Repository::new(
                    Database::for_namespace(path)?,
                    DOMAIN,
                    KEY,
                    MAX_CATALOG_BYTES,
                    validate,
                )
            })
            .transpose()?;
        let saved = store
            .as_ref()
            .map(|store| store.load())
            .transpose()?
            .flatten()
            .unwrap_or_default();
        Ok(Self { saved, store })
    }

    /// Publish memory only after the complete replacement commits successfully.
    pub(super) fn observe(&mut self, id: &str, detail: &Value) -> Result<Value, String> {
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "HID observation clock unavailable")?
            .as_millis()
            .to_string();
        self.observe_at(id, detail, at)
    }
    fn observe_at(
        &mut self,
        id: &str,
        detail: &Value,
        observed_at: String,
    ) -> Result<Value, String> {
        if !valid_id(id)
            || !detail.is_object()
            || detail
                .get("interfaceId")
                .is_some_and(|v| v.as_str() != Some(id))
        {
            return Err("Invalid HID metadata identity".into());
        }
        let mut detail = metadata::sanitize(detail);
        detail["interfaceId"] = json!(id);
        if detail.to_string().len() > MAX_DETAIL_BYTES {
            return Err("HID metadata snapshot exceeds its size bound".into());
        }
        let identity = metadata::identity(id, &detail);
        let previous = self
            .saved
            .get(&identity)
            .map(|s| s.interface_id.clone())
            .filter(|old| old != id);
        let provenance = metadata::provenance(&detail);
        let snapshot = Snapshot {
            snapshot_id: identity.clone(),
            interface_id: id.into(),
            contract_fingerprint: metadata::fingerprint(&detail, &provenance),
            provenance,
            observed_at,
            detail,
        };
        let mut next = self.saved.clone();
        next.insert(identity, snapshot.clone());
        validate(&next)?;
        if serde_json::to_vec(&next)
            .map_err(|_| "Cannot encode HID metadata catalog")?
            .len()
            > MAX_CATALOG_BYTES
        {
            return Err("HID metadata catalog exceeds its size bound".into());
        }
        if let Some(store) = &self.store {
            store.save(&next)?;
        }
        self.saved = next;
        let mut result = snapshot.value(true);
        if let Some(previous) = previous {
            result["previousInterfaceId"] = json!(previous);
        }
        Ok(result)
    }

    pub(super) fn snapshot(&self, id: &str) -> Option<Value> {
        self.saved
            .get(id)
            .or_else(|| self.saved.values().find(|s| s.interface_id == id))
            .map(|s| s.value(true))
    }
    pub(super) fn snapshots(&self) -> Vec<Value> {
        self.saved.values().map(|s| s.value(false)).collect()
    }
    pub(super) fn needs_refresh(&self, id: &str, summary: &Value) -> bool {
        self.saved
            .get(&metadata::identity(id, summary))
            .is_some_and(|s| s.interface_id != id)
    }
}

impl Snapshot {
    fn value(&self, full: bool) -> Value {
        let mut value = json!({"snapshotId":self.snapshot_id,"interfaceId":self.interface_id,
            "contractFingerprint":self.contract_fingerprint,"provenance":self.provenance,
            "observedAt":self.observed_at,"availability":"offline","status":"last-observed"});
        for name in [
            "product",
            "name",
            "manufacturer",
            "attributes",
            "vendorId",
            "productId",
            "usagePage",
            "usage",
            "backend",
        ] {
            if let Some(field) = self.detail.get(name) {
                value[name] = field.clone();
            }
        }
        if full {
            value["detail"] = self.detail.clone();
        }
        value
    }
}

#[cfg(test)]
mod tests;
