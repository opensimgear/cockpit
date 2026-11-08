//! Watch the selected Assetto executable and any supported sibling runtime.
use super::*;
use crate::game_telemetry::Game;
use osg_streams::NativeWake;

#[derive(Default)]
pub(super) struct ProviderWatch {
    source: context_sdk_watch::SourceWatch,
    pending: bool,
}

impl ProviderWatch {
    pub(super) fn retry_at(&self) -> Option<Instant> {
        self.source.retry_at()
    }

    pub(super) fn change_pending(&self) -> bool {
        self.source.change_pending()
    }

    pub(super) fn refresh(
        &mut self, inner: &Inner, provider: Game, game: Option<&Value>, wake: &Arc<NativeWake>,
        current: impl Fn() -> bool,
    ) {
        let paths = game.and_then(|game| game["executable"].as_str())
            .map(|executable| crate::acr_setup::executable_watch_paths(Path::new(executable), provider))
            .unwrap_or_default();
        if self.source.change_pending() && current() {
            if let Some(game) = game { inner.setup.invalidate_source_setup(provider.id(), game); }
        }
        if self.source.refresh(paths, wake) && current() {
            inner.setup.set_setup_watch(provider.id(), self.source.active());
            self.pending = self.source.active();
        }
        if !current() { return; }
        if let Some(game) = game {
            if self.source.retry_at().is_some() {
                inner.setup.set_setup_watch_unavailable(provider.id(), game, true);
            } else if self.pending {
                self.pending = !inner.setup.refresh_watched_setup(provider.id(), game,
                    || current() && !self.source.change_pending());
                if !self.pending && current() {
                    inner.setup.set_setup_watch_unavailable(provider.id(), game, false);
                }
            }
        }
    }
}
