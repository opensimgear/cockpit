//! Native setup watches and their baselines are owned by the integration.
use super::*;
use crate::game_telemetry::Game;
use crate::game_vehicles::CatalogueSource;
use osg_streams::NativeWake;

enum Source {
    Sdk(context_sdk_watch::SourceWatch),
    Assetto(Game, context_assetto_watch::ProviderWatch),
    Dcs(context_dcs_watch::DcsWatch),
}

impl Source {
    fn new(id: &str) -> Self {
        match id {
            "dcs" => Self::Dcs(Default::default()),
            "x-plane-11" | "x-plane-12" => Self::Sdk(Default::default()),
            _ => Self::Assetto(
                Game::from_id(id).expect("registered setup integration"),
                Default::default(),
            ),
        }
    }

    fn refresh(
        &mut self,
        inner: &Inner,
        id: &str,
        game: Option<&Value>,
        wake: &Arc<NativeWake>,
        current: impl Fn() -> bool,
    ) {
        match self {
            Self::Sdk(source) => {
                context_sdk_watch::refresh_sdk(source, inner, id, game, wake, current)
            }
            Self::Assetto(provider, source) => {
                source.refresh(inner, *provider, game, wake, current)
            }
            Self::Dcs(source) => source.refresh(inner, game, wake, current),
        }
    }

    fn retry_at(&self) -> Option<Instant> {
        match self {
            Self::Sdk(source) => source.retry_at(),
            Self::Assetto(_, source) => source.retry_at(),
            Self::Dcs(source) => source.retry_at(),
        }
    }

    fn change_pending(&self) -> bool {
        match self {
            Self::Sdk(source) => source.change_pending(),
            Self::Assetto(_, source) => source.change_pending(),
            Self::Dcs(source) => source.change_pending(),
        }
    }
}

pub(super) fn start(inner: &Arc<Inner>) -> Vec<thread::JoinHandle<()>> {
    inner
        .changes
        .setup_watch_wakes
        .iter()
        .map(|(id, wake)| {
            let inner = inner.clone();
            let wake = wake.clone();
            thread::spawn(move || run(&inner, id, &wake))
        })
        .collect()
}

fn run(inner: &Inner, id: &str, wake: &Arc<NativeWake>) {
    let _ = wake.bind_thread(thread::current());
    let mut source = Source::new(id);
    let mut catalogues = context_vehicle_catalogues::Catalogues::default();
    let mut last_library = None;
    while !inner.closed.load(Ordering::Acquire) {
        let observed = wake.observed();
        let library = inner.changes.context_wake.observed();
        let revision = inner.setup.provider_setup_revision(id);
        let current = || {
            !inner.closed.load(Ordering::Acquire)
                && inner.changes.context_wake.observed() == library
                && inner.setup.provider_setup_revision(id) == revision
        };
        let games: Vec<_> = {
            let settings = inner.settings.lock().unwrap_or_else(|p| p.into_inner());
            if settings.metadata_error.is_some() {
                Vec::new()
            } else {
                settings
                    .catalogue
                    .iter()
                    .filter(|game| crate::game_discovery::integration_key(game) == Some(id))
                    .cloned()
                    .collect()
            }
        };
        if current() {
            source.refresh(inner, id, games.first(), wake, current);
            if current()
                && last_library != Some(library)
                && inner
                    .setup
                    .refresh_background_scope(&games, || current() && !source.change_pending())
            {
                last_library = Some(library);
            }
            if matches!(source, Source::Dcs(_) | Source::Sdk(_)) {
                catalogues.refresh_scope(inner, Some(id), wake, current, CatalogueSource::read);
            }
        }
        if !current() {
            // A callback may have completed for retired paths. Rebuild this
            // owner's watch state so no publication is lost on the new scope.
            source = Source::new(id);
            catalogues = Default::default();
            last_library = None;
            continue;
        }
        let retry = source
            .retry_at()
            .into_iter()
            .chain(catalogues.retry_at())
            .min()
            .map(|at| at.saturating_duration_since(Instant::now()));
        wake.wait_scheduled(observed, retry);
    }
}
