//! Installed aircraft catalogues, independent of simulator process state.
use super::*;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum CatalogueSource {
    Dcs(PathBuf),
    Xplane(PathBuf),
}

impl CatalogueSource {
    pub(crate) fn for_game(game: &Value, dcs_path: Option<&Path>) -> Option<Self> {
        match crate::game_discovery::integration_key(game)? {
            "dcs" => dcs_path.filter(|path| path.is_absolute()).map(|path| Self::Dcs(path.into())),
            "x-plane-11" | "x-plane-12" => game["installPath"].as_str().map(Path::new)
                .filter(|path| path.is_absolute()).map(|path| Self::Xplane(path.into())),
            _ => None,
        }
    }

    pub(crate) fn read(&self) -> Result<Vec<Value>, String> {
        let mut rows = match self {
            Self::Dcs(path) => dcs_catalogue(path)?,
            Self::Xplane(path) => xplane_catalogue(path)?,
        };
        rows.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
        Ok(rows)
    }
}

impl Tracker {
    pub(crate) fn retain_catalogues(&mut self, desired: &std::collections::BTreeSet<CatalogueSource>) {
        let count = self.catalogues.len();
        self.catalogues.retain(|source, _| desired.contains(source));
        if self.catalogues.len() != count { self.changed(); }
    }

    pub(crate) fn publish_catalogue(&mut self, source: CatalogueSource, baseline: Result<Vec<Value>, String>) {
        if self.catalogues.get(&source) == Some(&baseline) { return; }
        self.catalogues.insert(source, baseline);
        self.changed();
    }
}
