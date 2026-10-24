//! Fresh installation metadata only. No profile, binding or runtime owner is
//! accepted here; callers supply this store's new explicit path.
use serde::{
    de::{self, MapAccess, Visitor},
    Deserialize, Deserializer, Serialize,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(crate) const MAX_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_EXECUTABLE_BYTES: usize = 4096;
pub(crate) const GAMES: [&str; 7] = [
    "dcs",
    "assetto-corsa-rally",
    "x-plane-11",
    "x-plane-12",
    "assetto-corsa",
    "assetto-corsa-competizione",
    "assetto-corsa-evo",
];
pub(crate) const MAX_GAMES: usize = 256;

pub(crate) fn is_custom_id(id: &str) -> bool {
    id.strip_prefix("custom-").is_some_and(|value| {
        uuid::Uuid::parse_str(value).is_ok_and(|parsed| parsed.to_string() == value)
    })
}
pub(crate) fn valid_game_id(id: &str) -> bool {
    GAMES.contains(&id) || is_custom_id(id)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CustomGame {
    pub name: String,
    pub executable: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<LibrarySource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_background: Option<crate::game_artwork::Background>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LibrarySource {
    pub client: String,
    pub app_id: String,
    pub install_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launcher: Option<String>,
}
impl LibrarySource {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !matches!(self.client.as_str(), "steam" | "epic")
            || self.app_id.is_empty()
            || self.app_id.len() > 128
            || !self
                .app_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
            || (self.client == "steam"
                && self
                    .app_id
                    .parse::<u32>()
                    .ok()
                    .filter(|id| *id > 0)
                    .is_none())
            || !executable(&self.install_path)
            || self.launcher.as_ref().is_some_and(|path| {
                !executable(path)
                    || self.client != "steam"
                    || !path
                        .rsplit(['\\', '/'])
                        .next()
                        .is_some_and(|name| name.eq_ignore_ascii_case("steam.exe"))
            })
        {
            return Err("Invalid local game library source.".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Settings {
    pub schema_version: u32,
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "distinct_map"
    )]
    pub categories: BTreeMap<String, String>,
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "distinct_map"
    )]
    pub game_categories: BTreeMap<String, String>,
    #[serde(deserialize_with = "distinct_installations")]
    pub manual_installations: BTreeMap<String, String>,
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "distinct_map"
    )]
    pub custom_games: BTreeMap<String, CustomGame>,
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "distinct_map"
    )]
    pub backgrounds: BTreeMap<String, crate::game_artwork::Background>,
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "distinct_map"
    )]
    pub default_backgrounds: BTreeMap<String, crate::game_artwork::Background>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            categories: BTreeMap::new(),
            game_categories: BTreeMap::new(),
            manual_installations: BTreeMap::new(),
            custom_games: BTreeMap::new(),
            backgrounds: BTreeMap::new(),
            default_backgrounds: BTreeMap::new(),
        }
    }
}
fn distinct_map<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, T>, D::Error> {
    struct Entries<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> Visitor<'de> for Entries<T> {
        type Value = BTreeMap<String, T>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("bounded distinct game records")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut entries = BTreeMap::new();
            while let Some((id, value)) = map.next_entry::<String, T>()? {
                if entries.len() >= MAX_GAMES || entries.insert(id, value).is_some() {
                    return Err(de::Error::custom("Duplicate or excessive game record"));
                }
            }
            Ok(entries)
        }
    }
    deserializer.deserialize_map(Entries(std::marker::PhantomData))
}
fn distinct_installations<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, String>, D::Error> {
    struct Installations;
    impl<'de> Visitor<'de> for Installations {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("at most seven distinct native game installations")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut entries = BTreeMap::new();
            while let Some((id, path)) = map.next_entry::<String, String>()? {
                if entries.len() == GAMES.len() || entries.insert(id, path).is_some() {
                    return Err(de::Error::custom(
                        "Duplicate or excessive native game installation",
                    ));
                }
            }
            Ok(entries)
        }
    }
    deserializer.deserialize_map(Installations)
}
pub(crate) fn executable(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_EXECUTABLE_BYTES
        && !path.chars().any(char::is_control)
        && (Path::new(path).is_absolute()
            || (path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                && path.as_bytes().get(1) == Some(&b':')
                && matches!(path.as_bytes().get(2), Some(b'\\' | b'/'))))
}
impl Settings {
    pub(crate) fn validate(&self) -> Result<(), String> {
        let mut names = std::collections::BTreeSet::new();
        if self.categories.len() > 64
            || self.categories.iter().any(|(id, name)| {
                !uuid::Uuid::parse_str(id).is_ok_and(|parsed| parsed.to_string() == *id)
                    || name.is_empty()
                    || name.trim() != name
                    || name.chars().count() > 48
                    || name.chars().any(char::is_control)
                    || !names.insert(name.to_lowercase())
            })
        {
            return Err(
                "Use at most 64 categories with distinct names between 1 and 48 characters.".into(),
            );
        }
        if self.game_categories.len() > MAX_GAMES
            || self.game_categories.iter().any(|(id, category)| {
                (!GAMES.contains(&id.as_str()) && !self.custom_games.contains_key(id))
                    || !self.categories.contains_key(category)
            })
        {
            return Err("Choose an existing game and category.".into());
        }
        if self.schema_version != 1
            || self.manual_installations.len() > GAMES.len()
            || self
                .manual_installations
                .iter()
                .any(|(id, path)| !GAMES.contains(&id.as_str()) || !executable(path))
        {
            return Err("Installation metadata requires supported games and bounded absolute executable paths.".into());
        }
        if self.custom_games.len() + GAMES.len() > MAX_GAMES || self.backgrounds.len() > MAX_GAMES {
            return Err("The game library exceeds its native game count bound.".into());
        }
        let mut sources = std::collections::BTreeSet::new();
        for (id, game) in &self.custom_games {
            if !is_custom_id(id)
                || game.name.is_empty()
                || game.name.trim() != game.name
                || game.name.chars().count() > 80
                || game.name.chars().any(char::is_control)
                || game
                    .executable
                    .as_ref()
                    .is_some_and(|path| !executable(path))
            {
                return Err("Custom games need stable identities, bounded names and absolute executable paths.".into());
            }
            if let Some(source) = &game.library {
                source.validate()?;
                if !sources.insert((&source.client, &source.app_id)) {
                    return Err("This library game was already imported.".into());
                }
            }
            if let Some(background) = &game.default_background {
                background.validate()?;
            }
        }
        for (id, background) in self
            .backgrounds
            .iter()
            .chain(self.default_backgrounds.iter())
        {
            if !GAMES.contains(&id.as_str()) && !self.custom_games.contains_key(id) {
                return Err("Choose an existing game for its background.".into());
            }
            background.validate()?;
        }
        if serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > MAX_BYTES {
            return Err("Installation metadata exceeds its native size bound.".into());
        }
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
                "installations",
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
#[path = "installation_tests.rs"]
mod tests;
