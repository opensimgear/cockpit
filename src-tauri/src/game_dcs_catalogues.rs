//! Passive aircraft catalogue registrations shared by authoring and native stage.
use super::*;
impl GamesRuntime {
    pub(crate) fn resource_catalogue_generation(&self, integration: &str) -> Option<u64> {
        self.inner.setup.catalogue_generation(integration)
    }
    pub(crate) fn resource_catalogue_watched(&self, integration: &str) -> bool {
        self.inner.setup.catalogue_watched(integration)
    }
    pub(crate) fn resource_dcs_vehicle_catalogue(&self, vehicle: &str) -> Result<Value, String> {
        self.inner.setup.resource_integration_catalogue_for_vehicle("dcs", Some(vehicle))
    }
    pub(crate) fn register_dcs_vehicle_catalogue(&self, vehicle: &str) -> Result<String, String> {
        let catalogue = self.resource_dcs_vehicle_catalogue(vehicle)?;
        let module = catalogue["aircraftModule"].as_str().ok_or("The aircraft catalogue has no canonical module.")?.to_owned();
        let mut registered = self.inner.dcs_vehicle_catalogues.lock().unwrap_or_else(|p|p.into_inner());
        if registered.len() >= 1000 && !registered.contains(&module) { return Err("The registered DCS aircraft catalogue limit was reached.".into()); }
        let added = registered.insert(module.clone());
        drop(registered);
        if added { self.inner.changes.mark_resources(); }
        Ok(module)
    }
    pub(crate) fn dcs_vehicle_catalogues(&self) -> Vec<String> {
        self.inner.dcs_vehicle_catalogues.lock().unwrap_or_else(|p|p.into_inner()).iter().cloned().collect()
    }
}
