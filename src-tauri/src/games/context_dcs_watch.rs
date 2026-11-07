//! Native invalidation for the selected DCS setup files.
use super::*;
use notify::{event::ModifyKind, EventKind};

#[derive(Default)]
pub(super) struct DcsWatch {
    source: context_sdk_watch::SourceWatch,
    catalogue: context_sdk_watch::SourceWatch,
    scope: Option<(PathBuf, Option<PathBuf>)>,
    paths: Vec<PathBuf>,
    pending: bool,
    installed: bool,
    saved_games: Option<PathBuf>,
    install: Option<PathBuf>,
}

impl DcsWatch {
    pub(super) fn retry_at(&self) -> Option<Instant> {
        self.source.retry_at().into_iter().chain(self.catalogue.retry_at()).min()
    }

    pub(super) fn refresh(&mut self, inner: &Inner) {
        let wake = &inner.changes.context_worker_wake;
        let catalogue = inner.setup.dcs_catalogue_watch_path();
        let children = catalogue.as_deref().map(|root| {
            (root, catalogue_file as prepared_sources::watch::ChildFilter)
        });
        if self.catalogue.refresh_with_children(catalogue.iter().cloned().collect(), children, wake) {
            inner.setup.invalidate_dcs_catalogue();
            inner.changes.mark(crate::game_read_models::signals::Domain::Setup);
            inner.changes.mark_resources();
        }
        let game = inner.settings.lock().unwrap_or_else(|error| error.into_inner())
            .catalogue.iter().find(|game| crate::game_discovery::integration_key(game) == Some("dcs")).cloned();
        let installed = game.as_ref().is_some_and(|game| game["installed"] == true);
        let install = game.as_ref().and_then(|game| game["installPath"].as_str())
            .map(PathBuf::from).filter(|path| path.is_absolute());
        let scope = game.as_ref().and_then(|game| inner.setup.dcs_setup_watch_scope(game));
        if self.installed != installed || self.scope != scope || self.install != install {
            self.saved_games = installed.then(crate::dcs_setup::saved_games_root).and_then(Result::ok);
            self.paths = scope.as_ref().map_or_else(Vec::new, |(target, install)| {
                crate::dcs_setup::setup_watch_paths(target, install.as_deref())
            });
            self.paths.extend(crate::dcs_setup::install_discovery_watch_paths(install.as_deref()));
            self.paths.extend(self.saved_games.iter().cloned());
            self.scope = scope;
            self.installed = installed;
            self.install = install;
        }
        let children = self.saved_games.as_deref().map(|root| {
            (root, dcs_profile_directory as prepared_sources::watch::ChildFilter)
        });
        if self.source.refresh_with_children(self.paths.clone(), children, wake) {
            self.pending = self.source.active();
            inner.setup.set_setup_watch("dcs", self.pending && self.scope.is_some());
            inner.setup.set_dcs_discovery_watch(self.pending && children.is_some());
            if !self.pending { inner.changes.mark_resources(); }
        }
        if self.pending {
            if let Some(game) = &game {
                self.pending = !inner.setup.refresh_watched_setup("dcs", game);
                if !self.pending { inner.changes.mark_resources(); }
            }
        }
    }
}

fn dcs_profile_directory(kind: EventKind, name: &std::ffi::OsStr) -> bool {
    matches!(kind, EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_)))
        && name.to_str().is_some_and(|name| name.eq_ignore_ascii_case("dcs")
            || name.get(..4).is_some_and(|prefix| prefix.eq_ignore_ascii_case("dcs.")))
}

fn catalogue_file(_: EventKind, name: &std::ffi::OsStr) -> bool {
    Path::new(name).extension().and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
}

#[cfg(test)]
#[path = "context_dcs_watch_tests.rs"]
mod tests;
