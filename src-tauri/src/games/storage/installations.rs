//! Unified, evidence-backed game titles and their machine-local installations.
use serde::{Deserialize, Deserializer, Serialize, de::{self, MapAccess, Visitor}};
use std::{collections::{BTreeMap, BTreeSet}, path::{Path, PathBuf}};
pub(crate) const MAX_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_EXECUTABLE_BYTES: usize = 4096;
pub(crate) const MAX_GAMES: usize = 512;
pub(crate) fn valid_game_id(id: &str) -> bool { osg_profiles::activation::valid_game_id(id) }
pub(crate) fn executable(path: &str) -> bool {
    !path.is_empty() && path.len() <= MAX_EXECUTABLE_BYTES && !path.chars().any(char::is_control)
        && (Path::new(path).is_absolute() || (path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && path.as_bytes().get(1) == Some(&b':') && matches!(path.as_bytes().get(2), Some(b'\\' | b'/'))))
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub(crate) struct LibrarySource {
    pub client:String, pub app_id:String, pub install_path:String,
    #[serde(default, skip_serializing_if="Option::is_none")] pub launcher:Option<String>,
}
impl LibrarySource {
    pub(crate) fn validate(&self)->Result<(),String> {
        if !matches!(self.client.as_str(),"steam"|"epic"|"battlenet"|"ea"|"ubisoft"|"gog"|"xbox"|"disk")
            || self.app_id.is_empty() || self.app_id.len()>256 || self.app_id.chars().any(char::is_control)
            || !executable(&self.install_path) || self.launcher.as_ref().is_some_and(|path| !executable(path)
                || self.client!="steam" || !path.rsplit(['\\','/']).next().is_some_and(|name|name.eq_ignore_ascii_case("steam.exe"))) {
            return Err("Invalid game installation source.".into());
        }
        if self.client=="steam" && self.app_id.parse::<u32>().ok().filter(|id|*id>0).is_none() {
            return Err("Invalid Steam application identity.".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct Installation {
    pub root:String, pub executable:Option<String>, pub executables:Vec<String>, pub sources:Vec<LibrarySource>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct GameRecord {
    pub name:String,
    #[serde(deserialize_with="distinct_map")] pub installations:BTreeMap<String,Installation>,
    pub selected_installation:Option<String>, pub integration:Option<String>,
    pub default_background:Option<crate::game_artwork::Background>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct Settings {
    pub revision:u64, pub discovery_initialized:bool, pub scan_roots:Vec<String>,
    #[serde(deserialize_with="distinct_map")] pub games:BTreeMap<String,GameRecord>,
    #[serde(deserialize_with="distinct_map")] pub tags:BTreeMap<String,String>,
    #[serde(deserialize_with="distinct_map")] pub game_tags:BTreeMap<String,Vec<String>>,
    #[serde(deserialize_with="distinct_map")] pub backgrounds:BTreeMap<String,crate::game_artwork::Background>,
    #[serde(deserialize_with="distinct_map")] pub default_backgrounds:BTreeMap<String,crate::game_artwork::Background>,
}
#[path="../tags/mod.rs"] pub(crate) mod tags;
impl Default for Settings {
    fn default()->Self {
        Self {revision:0,discovery_initialized:false,scan_roots:vec![],games:BTreeMap::new(),
            tags:tags::defaults(),game_tags:BTreeMap::new(),backgrounds:BTreeMap::new(),default_backgrounds:BTreeMap::new()}
    }
}
fn distinct_map<'de,D:Deserializer<'de>,T:Deserialize<'de>>(deserializer:D)->Result<BTreeMap<String,T>,D::Error> {
    struct Entries<T>(std::marker::PhantomData<T>);
    impl<'de,T:Deserialize<'de>> Visitor<'de> for Entries<T> {
        type Value=BTreeMap<String,T>;
        fn expecting(&self,f:&mut std::fmt::Formatter)->std::fmt::Result {f.write_str("bounded distinct game records")}
        fn visit_map<A:MapAccess<'de>>(self,mut map:A)->Result<Self::Value,A::Error> {
            let mut entries=BTreeMap::new();
            while let Some((id,value))=map.next_entry::<String,T>()? {
                if entries.len()>=MAX_GAMES || entries.insert(id,value).is_some() {return Err(de::Error::custom("Duplicate or excessive game record"));}
            }
            Ok(entries)
        }
    }
    deserializer.deserialize_map(Entries(std::marker::PhantomData))
}
impl Settings {
    pub(crate) fn validate(&self)->Result<(),String> {
        tags::validate_definitions(&self.tags)?;
        if self.scan_roots.len()>64 || self.scan_roots.iter().any(|path|!executable(path)) || self.games.len()>MAX_GAMES {
            return Err("Game discovery locations exceed their bounds.".into());
        }
        let mut roots=BTreeSet::new();
        for (id,game) in &self.games {
            if !valid_game_id(id) || game.name.trim()!=game.name || game.name.is_empty() || game.name.chars().count()>80
                || game.name.chars().any(char::is_control) || game.installations.len()>32
                || game.selected_installation.as_ref().is_some_and(|selected|!game.installations.contains_key(selected))
                || game.integration.as_ref().is_some_and(|integration|crate::game_discovery::integration_provider(integration).is_none()) {
                return Err("A discovered game needs a valid identity, name and installation selection.".into());
            }
            for (instance_id,installation) in &game.installations {
                if !valid_game_id(instance_id) || !executable(&installation.root) || installation.sources.is_empty()
                    || installation.sources.len()>16 || installation.executables.len()>32
                    || installation.executable.as_ref().is_some_and(|path|!executable(path))
                    || installation.executables.iter().any(|path|!executable(path))
                    || !roots.insert(crate::game_discovery::normalized_path(&installation.root)) {
                    return Err("Installation locations must be distinct and bounded.".into());
                }
                for source in &installation.sources {source.validate()?;}
            }
            if let Some(background)=&game.default_background {background.validate()?;}
        }
        for (id,ids) in &self.game_tags {
            if !self.games.contains_key(id) {return Err("Choose a discovered game for its tags.".into());}
            tags::validate_assignment(ids, &self.tags)?;
        }
        for (id,background) in self.backgrounds.iter().chain(self.default_backgrounds.iter()) {
            if !self.games.contains_key(id) {return Err("Choose a discovered game for its background.".into());}
            background.validate()?;
        }
        if serde_json::to_vec(self).map_err(|e|e.to_string())?.len()>MAX_BYTES {return Err("Game library exceeds its size bound.".into());}
        Ok(())
    }
}

pub(crate) struct Store {
    path: PathBuf,
    repository: std::sync::OnceLock<osg_persistence::Repository<Settings>>,
}
impl Store {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            repository: std::sync::OnceLock::new(),
        }
    }
    pub(crate) fn root(&self) -> &Path {
        self.path.parent().expect("game namespace has a parent")
    }
    fn repository(&self) -> Result<&osg_persistence::Repository<Settings>, String> {
        if self.repository.get().is_none() {
            let db = osg_persistence::Database::for_namespace(&self.path)?;
            let repository = osg_persistence::Repository::new(
                db,
                "games",
                "library",
                MAX_BYTES,
                Settings::validate,
            )?;
            let _ = self.repository.set(repository);
        }
        Ok(self.repository.get().expect("repository initialized"))
    }
    pub(crate) fn load(&self) -> Result<Option<Settings>, String> {
        self.repository()?
            .load()
            .map_err(|error| format!("Cannot load game installation metadata: {error}"))
    }
    pub(crate) fn save(&self, settings: &Settings) -> Result<(), String> {
        settings.validate()?;
        self.repository()?.save(settings)
    }
}


#[cfg(test)]
#[path="installation_tests.rs"]
mod tests;
