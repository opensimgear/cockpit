use crate::ControllerService;
use serde_json::Value;

impl ControllerService {
    /// Read one current registry page without persistence, acquisition or authority checks.
    pub fn cached_sources_page(&self, offset: usize) -> Value {
        self.registry.lock().unwrap_or_else(|p| p.into_inner()).list(offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_pages_preserve_dirty_metadata_without_persistence_or_worker_access() {
        let mut service = ControllerService::new(None, false);
        let ids = {
            let mut registry = service.registry.lock().unwrap();
            (0..35).map(|index| {
                registry.connect(crate::tests::fixture(&format!("cached-{index}"), None)).unwrap()
            }).collect::<Vec<_>>()
        };
        service.registry.lock().unwrap().update(&ids[0], "Cached wheel", "wheelbase").unwrap();
        // An unavailable authority and held persistence/worker locks must not
        // affect a passive page read or cause it to try saving dirty metadata.
        service.storage_valid = false;
        let persistence = service.persistence.lock().unwrap();
        let worker = service.worker.lock().unwrap();
        let page = service.cached_sources_page(32);
        assert_eq!(page["offset"], 32);
        assert_eq!(page["total"], 35);
        assert_eq!(page["sources"].as_array().unwrap().len(), 3);
        assert!(page.get("selfAccess").is_none());
        let registry = service.registry.lock().unwrap();
        assert!(registry.dirty);
        assert_eq!(registry.inspect(&ids[0], 0).unwrap()["source"]["name"], "Cached wheel");
        assert_eq!(page["diagnostic"], registry.list(32)["diagnostic"]);
        drop(registry);
        assert!(service.cached_sources_page(usize::MAX)["sources"].as_array().unwrap().is_empty());
        drop(worker);
        drop(persistence);
    }
}
