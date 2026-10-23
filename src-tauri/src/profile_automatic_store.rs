//! Machine-local activation rules, separate from portable profile graphs and
//! transient process proofs. Missing configuration is Manual/Default; malformed
//! configuration is preserved and must never enable background activation.
use osg_profiles::{
    activation::{validate_rules, Rule},
    session::SelectionMode,
    DEFAULT_PROFILE_ID,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::PathBuf,
};
const MAX_BYTES: usize = 2 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Settings {
    pub schema_version: u32,
    pub mode: SelectionMode,
    pub manual_profile_id: String,
    pub rules: Vec<Rule>,
    /// User choice among profiles sharing a saved game context. Contexts are
    /// revalidated against profile storage on every read and launch.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub preferred_profiles: BTreeMap<String, String>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            mode: SelectionMode::Manual,
            manual_profile_id: DEFAULT_PROFILE_ID.into(),
            rules: vec![],
            preferred_profiles: BTreeMap::new(),
        }
    }
}
impl Settings {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || !uuid::Uuid::parse_str(&self.manual_profile_id)
                .is_ok_and(|id| id.to_string() == self.manual_profile_id)
        {
            return Err("Invalid native automatic profile settings identity/version.".into());
        }
        validate_rules(&self.rules)?;
        if self.preferred_profiles.len() > osg_profiles::activation::MAX_RULES
            || self.preferred_profiles.iter().any(|(game, profile)| {
                !osg_profiles::activation::valid_game_id(game)
                    || !uuid::Uuid::parse_str(profile).is_ok_and(|id| id.to_string() == *profile)
            })
        {
            return Err(
                "Remembered launch choices need valid games and saved profile identities."
                    .into(),
            );
        }
        if serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > MAX_BYTES {
            return Err("Automatic profile settings exceed their native storage bound.".into());
        }
        Ok(())
    }
}
pub(crate) struct Store {
    path:Option<PathBuf>,
    repository:std::sync::OnceLock<osg_persistence::Repository<Settings>>,
}
impl Store {
    pub(crate) fn new(path:Option<PathBuf>)->Self {Self {path,repository:std::sync::OnceLock::new()}}
    fn repository(&self)->Result<Option<&osg_persistence::Repository<Settings>>,String> {
        let Some(path)=&self.path else {return Ok(None);};
        if self.repository.get().is_none() {
            let db=osg_persistence::Database::for_source(path)?;
            let repository=osg_persistence::Repository::new(db,"profiles","selection",1,Some(path.clone()),MAX_BYTES,Settings::validate)?;
            let _=self.repository.set(repository);
        }
        Ok(self.repository.get())
    }
    pub(crate) fn load(&self)->Result<Option<Settings>,String> {self.repository()?.map_or(Ok(None),|repository|repository.load())}
    pub(crate) fn save(&self,settings:&Settings)->Result<(),String> {
        settings.validate()?;self.repository()?.map_or(Ok(()),|repository|repository.save(settings))
    }
}
#[cfg(test)]
#[path = "profile_automatic_store_tests.rs"]
mod tests;
