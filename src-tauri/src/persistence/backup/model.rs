use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
pub(super) const MAX_BACKUPS: usize = 8;
pub(super) const MAX_ASSETS: usize = 1216;
pub(super) const MAX_ASSET_BYTES: u64 = 8 * 1024 * 1024;
pub(super) const MAX_TOTAL_ASSETS: u64 = 64 * 1024 * 1024;
pub(super) fn id(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
pub(super) fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Index {
    pub entries: Vec<Entry>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Entry {
    pub id: String,
    pub created_at: u64,
    pub manifest_hash: String,
}
impl Index {
    pub fn validate(&self) -> Result<(), String> {
        let mut ids = BTreeSet::new();
        if self.entries.len() > MAX_BACKUPS
            || self.entries.iter().any(|entry| {
                !id(&entry.id) || !hash(&entry.manifest_hash) || !ids.insert(&entry.id)
            })
        {
            return Err("Invalid saved backup inventory. Preserve the database.".into());
        }
        Ok(())
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Asset {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Manifest {
    pub id: String,
    pub created_at: u64,
    pub database_hash: String,
    pub assets: Vec<Asset>,
}
impl Manifest {
    pub fn validate(&self) -> Result<(), String> {
        if !id(&self.id) || !hash(&self.database_hash) || self.assets.len() > MAX_ASSETS {
            return Err("Invalid backup manifest.".into());
        }
        let mut paths = BTreeSet::new();
        let mut total = 0u64;
        for asset in &self.assets {
            let parts = asset.path.split('/').collect::<Vec<_>>();
            let allowed = match parts.as_slice() {
                ["game-artwork", name] => name
                    .strip_suffix(".jpg")
                    .is_some_and(|name| hash(name) && name == asset.sha256),
                ["games.integration-setup.setup-work", operation, name] => {
                    id(operation)
                        && [
                            "Export.lua.before",
                            "DCSAircraft.lua.before",
                            "DCSPhysics.lua.before",
                        ]
                        .contains(name)
                }
                _ => false,
            };
            if !allowed
                || !hash(&asset.sha256)
                || asset.size > MAX_ASSET_BYTES
                || !paths.insert(&asset.path)
            {
                return Err("Invalid backup asset reference.".into());
            }
            total = total
                .checked_add(asset.size)
                .ok_or("Backup assets exceed their byte budget.")?;
        }
        if total > MAX_TOTAL_ASSETS {
            return Err("Backup assets exceed their byte budget.".into());
        }
        Ok(())
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Pending {
    pub id: String,
    pub epoch: String,
    pub rollback: Option<String>,
    pub recovery_fingerprint: String,
}
impl Pending {
    pub fn validate(&self) -> Result<(), String> {
        if !id(&self.id)
            || !hash(&self.recovery_fingerprint)
            || self.epoch.len() != 32
            || !self
                .epoch
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || self.rollback.as_ref().is_some_and(|v| !id(v))
        {
            return Err("Invalid pending restore. Preserve the database.".into());
        }
        Ok(())
    }
}
