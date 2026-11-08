//! Native invalidation for the selected DCS setup files.
use super::*;
use crate::game_integration_setup::DcsCatalogueWatch;
use notify::{event::ModifyKind, EventKind};
use osg_streams::NativeWake;

#[derive(Default)]
pub(super) struct DcsWatch {
    source: context_sdk_watch::SourceWatch,
    catalogue: context_sdk_watch::SourceWatch,
    scope: Option<(PathBuf, Option<PathBuf>)>,
    paths: Vec<PathBuf>,
    pending: bool,
    installed: bool,
    saved_games: Option<PathBuf>,
    saved_games_retry: Option<Instant>,
    install: Option<PathBuf>,
}

impl DcsWatch {
    pub(super) fn retry_at(&self) -> Option<Instant> {
        [self.source.retry_at(), self.catalogue.retry_at(), self.saved_games_retry]
            .into_iter().flatten().min()
    }

    pub(super) fn change_pending(&self) -> bool {
        self.source.change_pending()
    }

    pub(super) fn refresh(
        &mut self, inner: &Inner, game: Option<&Value>, wake: &Arc<NativeWake>,
        current: impl Fn() -> bool,
    ) {
        let catalogue = inner.setup.dcs_catalogue_watch_path();
        let children = catalogue.as_deref().map(|root| {
            (root, catalogue_file as prepared_sources::watch::ChildFilter)
        });
        if self.catalogue.refresh_with_children(catalogue.iter().cloned().collect(), children, wake) && current() {
            let watch = catalogue.zip(self.catalogue.registration())
                .map(|(path, (changes, observed))| DcsCatalogueWatch { path, changes, observed });
            inner.setup.update_dcs_catalogue_watch(watch);
            inner.changes.mark(crate::game_read_models::signals::Domain::Setup);
            inner.changes.mark_resources();
            inner.changes.context_update_wake.notify();
        }
        if !current() { return; }
        let installed = game.as_ref().is_some_and(|game| game["installed"] == true);
        let install = game.as_ref().and_then(|game| game["installPath"].as_str())
            .map(PathBuf::from).filter(|path| path.is_absolute());
        let scope = game.as_ref().and_then(|game| inner.setup.dcs_setup_watch_scope(game));
        let installation_changed = self.installed != installed || self.install != install;
        let resolve_root = installation_changed || self.scope != scope
            || self.saved_games_retry.is_some_and(|at| at <= Instant::now());
        let mut root_changed = false;
        if resolve_root {
            let root = installed.then(crate::dcs_setup::saved_games_root).and_then(Result::ok);
            root_changed = self.saved_games != root;
            self.saved_games = root;
            self.saved_games_retry = (cfg!(windows) && installed && self.saved_games.is_none())
                .then(|| Instant::now() + context_sdk_watch::WATCH_RETRY);
        }
        if installation_changed || self.scope != scope || root_changed {
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
        if self.source.change_pending() && current() {
            if let Some(game) = game { inner.setup.invalidate_source_setup("dcs", game); }
        }
        let source_changed = self.source.refresh_with_children(self.paths.clone(), children, wake);
        if !current() { return; }
        if source_changed {
            self.pending = self.source.active();
            inner.setup.set_setup_watch("dcs", self.pending && self.scope.is_some());
            if !self.pending { inner.changes.mark_resources(); }
        }
        if let Some(game) = game {
            let unavailable = self.source.retry_at().is_some()
                || (self.scope.is_none() && self.saved_games_retry.is_some());
            if unavailable {
                inner.setup.set_setup_watch_unavailable("dcs", game, true);
            }
            if self.pending {
                self.pending = !inner.setup.refresh_watched_setup("dcs", game, || current() && !self.source.change_pending());
                if !self.pending && current() {
                    if !unavailable { inner.setup.set_setup_watch_unavailable("dcs", game, false); }
                    inner.changes.mark_resources();
                }
            }
        }
    }
}

fn dcs_profile_directory(kind: EventKind, name: &std::ffi::OsStr) -> bool {
    matches!(kind, EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_)))
        && crate::dcs_setup::is_profile_folder_name(name)
}

fn catalogue_file(_: EventKind, name: &std::ffi::OsStr) -> bool {
    Path::new(name).extension().and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
}

#[cfg(test)]
#[path = "context_dcs_watch_tests.rs"]
mod tests;
