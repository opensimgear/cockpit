//! Native invalidation for the selected DCS setup files.
use super::*;

#[derive(Default)]
pub(super) struct DcsWatch {
    source: context_sdk_watch::SourceWatch,
    scope: Option<(PathBuf, Option<PathBuf>)>,
    paths: Vec<PathBuf>,
    pending: bool,
}

impl DcsWatch {
    pub(super) fn refresh(&mut self, inner: &Inner) {
        let game = inner.settings.lock().unwrap_or_else(|error| error.into_inner())
            .catalogue.iter().find(|game| crate::game_discovery::integration_key(game) == Some("dcs")).cloned();
        let scope = game.as_ref().and_then(|game| inner.setup.dcs_setup_watch_scope(game));
        if self.scope != scope {
            self.paths = scope.as_ref().map_or_else(Vec::new, |(target, install)| {
                let mut paths = crate::dcs_setup::setup_watch_paths(target, install.as_deref());
                paths.extend(crate::dcs_setup::install_discovery_watch_paths(install.as_deref()));
                paths
            });
            self.scope = scope;
        }
        if self.source.refresh(self.paths.clone()) {
            self.pending = self.source.active();
            inner.setup.set_setup_watch("dcs", self.pending);
        }
        if self.pending {
            if let Some(game) = &game {
                self.pending = !inner.setup.refresh_watched_setup("dcs", game);
            }
        }
    }
}
