//! Native invalidation and baseline ownership for installed aircraft catalogues.
use super::*;
use crate::game_vehicles::CatalogueSource;
use notify::{event::ModifyKind, EventKind};
use osg_streams::NativeWake;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct Catalogues {
    watches: BTreeMap<CatalogueSource, context_sdk_watch::SourceWatch>,
}

impl Catalogues {
    pub(super) fn retry_at(&self) -> Option<Instant> {
        self.watches.values().filter_map(context_sdk_watch::SourceWatch::retry_at).min()
    }

    #[cfg(test)]
    pub(super) fn refresh(&mut self, inner: &Inner) {
        retain_current(inner);
        self.refresh_scope(inner, None, &inner.changes.context_worker_wake, || true, CatalogueSource::read);
    }

    pub(super) fn refresh_scope(
        &mut self, inner: &Inner, integration: Option<&str>, wake: &Arc<NativeWake>,
        current: impl Fn() -> bool, read: impl Fn(&CatalogueSource) -> Result<Vec<Value>, String>,
    ) {
        let desired = desired_sources(inner, integration);
        self.watches.retain(|source, _| desired.contains(source));
        for source in desired {
            if !current() { return; }
            let watch = self.watches.entry(source.clone()).or_default();
            let changed = match &source {
                CatalogueSource::Dcs(directory) => watch.refresh(
                    vec![directory.clone(), directory.join("AircraftAliases.json")], wake,
                ),
                CatalogueSource::Xplane(installation) => {
                    let aircraft = installation.join("Aircraft");
                    watch.refresh_tree(vec![installation.clone(), aircraft.clone()], &aircraft, aircraft_topology, wake)
                }
            };
            if !changed { continue; }
            let baseline = if watch.active() {
                read(&source)
            } else {
                Err("Aircraft catalogue file notifications are unavailable. Cockpit will retry the file watch.".into())
            };
            // A changed source is read on the pending wake. A different scope
            // cannot publish a result captured for the retired installation.
            if current() && !watch.change_pending() && desired_sources(inner, integration).contains(&source) {
                inner.vehicles.lock().unwrap_or_else(|p| p.into_inner()).publish_catalogue(source, baseline);
            }
        }
    }
}

pub(super) fn retain_current(inner: &Inner) {
    let desired = desired_sources(inner, None);
    inner.vehicles.lock().unwrap_or_else(|p| p.into_inner()).retain_catalogues(&desired);
}

fn desired_sources(inner: &Inner, integration: Option<&str>) -> BTreeSet<CatalogueSource> {
    let dcs = inner.setup.dcs_catalogue_watch_path();
    let settings = inner.settings.lock().unwrap_or_else(|p| p.into_inner());
    if settings.metadata_error.is_some() { return BTreeSet::new(); }
    settings.catalogue.iter()
        .filter(|game| integration.is_none_or(|id| crate::game_discovery::integration_key(game) == Some(id)))
        .filter_map(|game| CatalogueSource::for_game(game, dcs.as_deref())).collect()
}

// Only names and topology determine the aircraft list. Directory moves can
// add or remove aircraft below the moved path even when it has no .acf suffix.
fn aircraft_topology(kind: EventKind, _: &Path) -> bool {
    matches!(kind, EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_)))
}
