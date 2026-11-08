use super::{Observe, Source};
use crate::{registry::Registry, stream::ReportWatch};
use osg_streams::NativeWake;
use std::sync::{Arc, Mutex};

type Watch = Arc<
    dyn Fn(&[String], &Arc<NativeWake>, &Arc<NativeWake>) -> Result<ReportWatch, String>
        + Send
        + Sync,
>;

pub(crate) struct Capture {
    pub(super) observe: Observe,
    pub(super) watch: Watch,
}

impl Capture {
    pub(crate) fn registry(registry: Arc<Mutex<Registry>>) -> Self {
        let snapshots = registry.clone();
        Self {
            observe: Arc::new(move |ids| {
                snapshots.lock().unwrap_or_else(|p| p.into_inner()).visibility_sources(ids)
            }),
            watch: Arc::new(move |ids, reports, inventory| {
                let mut registry = registry.lock().unwrap_or_else(|p| p.into_inner());
                let reports = reports.clone();
                let lease = registry.watch_reports(ids, Arc::new(move || reports.notify()))?;
                registry.watch_visibility_inventory(inventory)?;
                Ok(lease)
            }),
        }
    }

    pub(super) fn sources(&self, ids: &[String]) -> Result<Vec<Source>, String> {
        (self.observe)(ids)
    }
}
