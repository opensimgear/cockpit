use super::*;
use osg_streams::{Delivery, LossReason};
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) struct Source {
    owner: Arc<Owner>,
    lease: Arc<Lease>,
    reader: Reader,
    last: Option<NativeSnapshot>,
    error: Option<String>,
    outputs: Vec<Value>,
    inspection: Vec<String>,
}
impl Source {
    pub(crate) fn new(connection: &Value, outputs: Vec<Value>) -> Result<Self, String> {
        validate_connection(connection)?;
        if !cfg!(windows) {
            return Err("X-Plane native process monitoring currently requires Windows.".into());
        }
        let identity =
            crate::game_discovery::ProcessIdentity::parse(&connection["processIdentity"])?;
        Self::open_with(connection, outputs, || {
            use crate::game_discovery::Platform;
            crate::game_discovery::NativePlatform.validate(
                &format!("x-plane-{}", connection["version"].as_u64().unwrap()),
                &identity.path,
            )?;
            let monitor = permissions::acquire(
                &format!("x-plane-{}", connection["version"].as_u64().unwrap()),
                &identity,
            )?;
            let catalogue = Driver::load_catalogue(connection)?;
            if let Some(error) = monitor.error() {
                return Err(error);
            }
            Owner::start(connection, Some(monitor), catalogue)
        })
    }

    fn open_with(
        connection: &Value,
        outputs: Vec<Value>,
        create_owner: impl FnOnce() -> Result<Owner, String>,
    ) -> Result<Self, String> {
        validate_connection(connection)?;
        crate::game_discovery::ProcessIdentity::parse(&connection["processIdentity"])?;
        let key = connection.to_string();
        let scope = connection["processIdentity"].to_string();
        let mut registry = OWNERS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        registry.retain(|_, owner| owner.strong_count() > 0);
        for (_, owner) in registry.iter().filter(|(other_key, _)| *other_key != &key) {
            if owner
                .upgrade()
                .is_some_and(|owner| owner.process_scope == scope && owner.shared.error().is_none())
            {
                return Err("X-Plane acquisition belongs to a different captured setup. Retire that reader before activating the replacement.".into());
            }
        }
        let existing = registry.get(&key).and_then(Weak::upgrade);
        let owner = match existing {
            Some(owner) if owner.shared.error().is_none() => owner,
            other => {
                if let Some(owner) = other {
                    owner.shutdown();
                }
                let owner = Arc::new(create_owner()?);
                registry.insert(key, Arc::downgrade(&owner));
                owner
            }
        };
        drop(registry);
        Self::attach(owner, outputs)
    }
    fn attach(owner: Arc<Owner>, outputs: Vec<Value>) -> Result<Self, String> {
        if outputs.len() > 1024
            || outputs.iter().any(|output| {
                output["id"]
                    .as_str()
                    .is_none_or(|id| id.is_empty() || id.len() > 256)
            })
        {
            return Err("Choose at most 1024 bounded X-Plane output identifiers.".into());
        }
        let ids = Self::selected_ids(&owner.catalogue, &outputs, &[])?;
        owner
            .shared
            .active_leases
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 128).then_some(count + 1)
            })
            .map_err(|_| "X-Plane pending reader limit reached.")?;
        let lease = Arc::new(Lease {
            id: uuid::Uuid::new_v4().to_string(),
            release: owner.shared.release.clone(),
            active: AtomicBool::new(true),
            count: owner.shared.active_leases.clone(),
        });
        let reader = owner
            .update(&lease, ids, true)?
            .ok_or("X-Plane initial subscription missing.")?;
        Ok(Self {
            owner,
            lease,
            reader,
            last: None,
            error: None,
            outputs,
            inspection: vec![],
        })
    }
    pub(crate) fn set_inspection_channels(&mut self, ids: &[String]) -> Result<(), String> {
        if ids.len() > 512 {
            return Err("Inspect at most 512 X-Plane components per page.".into());
        }
        let mut inspection = ids.to_vec();
        inspection.sort();
        inspection.dedup();
        if inspection == self.inspection {
            return Ok(());
        }
        let ids = Self::selected_ids(&self.owner.catalogue, &self.outputs, &inspection)?;
        if let Err(error) = self.owner.update(&self.lease, ids, false) {
            if !self.lease.active.load(Ordering::Acquire) || self.owner.shared.error().is_some() {
                return Err(self.retire(error));
            }
            return Err(error);
        }
        self.inspection = inspection;
        Ok(())
    }
    fn selected_ids(
        catalogue: &Catalogue,
        outputs: &[Value],
        inspection: &[String],
    ) -> Result<Vec<String>, String> {
        let mut ids: BTreeSet<_> = outputs
            .iter()
            .map(|output| {
                output["channel"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or("Choose a numeric X-Plane source channel.")
            })
            .collect::<Result<_, _>>()?;
        ids.extend(inspection.iter().cloned());
        if ids.len() > 1024 {
            return Err("Select at most 1024 X-Plane components per reader.".into());
        }
        for id in &ids {
            if !catalogue
                .get(id)
                .is_some_and(|channel| channel.metadata["supported"] == true)
            {
                return Err(format!(
                    "Selected X-Plane dataref is unavailable in this installation: {id}."
                ));
            }
        }
        Ok(ids.into_iter().collect())
    }
    pub(crate) fn native_snapshot(&mut self) -> Result<NativeSnapshot, String> {
        if !self.lease.active.load(Ordering::Acquire) {
            return Err(self.retire("X-Plane reader lease retired.".into()));
        }
        if let Some(error) = self.owner.shared.error().or_else(|| self.error.clone()) {
            return Err(self.retire(error));
        }
        loop {
            match self.reader.try_recv() {
                Ok(Some(Delivery::Snapshot(snapshot))) => self.last = Some(snapshot.value),
                Ok(Some(Delivery::Event(event))) => self.last = Some(event.value),
                Ok(Some(Delivery::Loss(loss))) if loss.reason == LossReason::GenerationReplaced => {
                    self.last = None
                }
                Ok(None) => {
                    return self
                        .last
                        .clone()
                        .ok_or_else(|| "X-Plane committed baseline unavailable.".into())
                }
                result => {
                    let error = format!("X-Plane subscriber lost continuity: {result:?}");
                    return Err(self.retire(error));
                }
            }
        }
    }
    fn retire(&mut self, error: String) -> String {
        self.reader.cancellation_handle().cancel();
        self.lease.deactivate();
        self.last = None;
        self.error = Some(error.clone());
        error
    }
    pub(crate) fn snapshot(&mut self) -> Result<(ExportSnapshot, Value), String> {
        let snapshot = self.native_snapshot()?;
        Ok(snapshot.project_selected(Instant::now(), &self.outputs, &self.inspection))
    }
    #[cfg(test)]
    pub(super) fn open_test(
        connection: &Value,
        catalogue: BTreeMap<String, datarefs::Channel>,
        outputs: Vec<Value>,
    ) -> Result<Self, String> {
        Self::attach(
            Arc::new(Owner::start(connection, None, catalogue)?),
            outputs,
        )
    }
    #[cfg(test)]
    pub(super) fn share_test(&self, outputs: Vec<Value>) -> Result<Self, String> {
        Self::attach(self.owner.clone(), outputs)
    }
    #[cfg(test)]
    pub(super) fn address_test(&self) -> std::net::SocketAddr {
        self.owner.address
    }
    #[cfg(test)]
    pub(super) fn retire_test(&self) {
        self.lease.deactivate();
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
