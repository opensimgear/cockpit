//! Watch the selected Assetto executable and any supported sibling runtime.
use super::*;
use crate::game_telemetry::Game;

const PROVIDERS: [Game; 4] = [Game::Corsa, Game::Competizione, Game::Evo, Game::Rally];

#[derive(Default)]
pub(super) struct AssettoWatches([ProviderWatch; 4]);

#[derive(Default)]
struct ProviderWatch {
    source: context_sdk_watch::SourceWatch,
    pending: bool,
}

impl AssettoWatches {
    pub(super) fn retry_at(&self) -> Option<Instant> {
        self.0.iter().filter_map(|watch| watch.source.retry_at()).min()
    }

    pub(super) fn refresh(&mut self, inner: &Inner) {
        let games = {
            let settings = inner.settings.lock().unwrap_or_else(|error| error.into_inner());
            PROVIDERS.map(|provider| settings.catalogue.iter()
                .find(|game| crate::game_discovery::integration_key(game) == Some(provider.id()))
                .cloned())
        };
        for ((watch, game), provider) in self.0.iter_mut().zip(games).zip(PROVIDERS) {
            let paths = game.as_ref().and_then(|game| game["executable"].as_str())
                .map(|executable| crate::acr_setup::executable_watch_paths(Path::new(executable), provider))
                .unwrap_or_default();
            if watch.source.refresh(paths, &inner.changes.context_worker_wake) {
                inner.setup.set_setup_watch(provider.id(), watch.source.active());
                watch.pending = watch.source.active();
            }
            if watch.pending {
                if let Some(game) = &game {
                    watch.pending = !inner.setup.refresh_watched_setup(provider.id(), game);
                }
            }
        }
    }
}
