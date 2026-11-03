use super::super::{Registry, Source, MAX_SOURCES};
use serde::{Serialize, Serializer};
use std::sync::Arc;

const MAX_DIAGNOSTIC_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SelfAccess {
    status: &'static str,
    ready: bool,
    changed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    executable: Option<String>,
    message: String,
}
impl From<crate::visibility::self_access::Status> for SelfAccess {
    fn from(value: crate::visibility::self_access::Status) -> Self {
        Self { status: value.status, ready: value.ready, changed: value.changed,
            executable: value.executable, message: value.message }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Summary {
    pub id: String,
    name: String,
    role: String,
    origin: &'static str,
    backend: String,
    vendor_id: u16,
    product_id: u16,
    osg_hid: bool,
    online: bool,
    #[serde(serialize_with = "decimal")]
    generation: u64,
    sequence: &'static str,
    control_count: usize,
    baseline_ready: bool,
    identity_confidence: &'static str,
}
impl Summary {
    pub fn capture(source: &Source) -> Self {
        let saved = &source.saved;
        Self { id: saved.id.clone(), name: saved.name.clone(), role: saved.role.clone(),
            origin: "host-controller", backend: saved.identity.backend.clone(),
            vendor_id: saved.identity.vendor_id, product_id: saved.identity.product_id,
            osg_hid: saved.identity.vendor_id == 0xcafe && saved.identity.product_id == 0x4010,
            online: source.online, generation: source.generation, sequence: "0",
            control_count: source.controls.len(), baseline_ready: source.ready(),
            identity_confidence: if saved.identity.serial.is_some() { "serial" } else { "local-path" } }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct View {
    pub session: String,
    backend_status: String,
    diagnostic: String,
    sources: Vec<Summary>,
    total: usize,
    offset: usize,
    limit: usize,
    #[serde(serialize_with = "self_access")]
    self_access: Arc<SelfAccess>,
}
impl View {
    pub fn capture(registry: &Registry) -> Result<Self, String> {
        #[cfg(test)]
        super::test_support::full_capture(&registry.session);
        let access = &registry.inventory_observation.self_access;
        if registry.sources.len() > MAX_SOURCES || registry.backend_status.len() > 128
            || registry.diagnostic.len() > MAX_DIAGNOSTIC_BYTES
            || access.message.len() > MAX_DIAGNOSTIC_BYTES
            || access.executable.as_ref().is_some_and(|path| path.len() > 8192)
        { return Err("Controller inventory snapshot limit reached.".into()); }
        Ok(Self { session: registry.session.clone(), backend_status: registry.backend_status.clone(),
            diagnostic: registry.diagnostic.clone(), sources: registry.sources.values().map(Summary::capture).collect(),
            total: registry.sources.len(), offset: 0, limit: MAX_SOURCES, self_access: access.clone() })
    }
    pub fn apply_row(&mut self, row: Summary) -> Result<(), String> {
        let target = self.sources.iter_mut().find(|known| known.id == row.id)
            .ok_or("Controller inventory row has no current metadata baseline.")?;
        *target = row;
        Ok(())
    }
}
fn decimal<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&value.to_string())
}
fn self_access<S: Serializer>(value: &Arc<SelfAccess>, serializer: S) -> Result<S::Ok, S::Error> {
    value.as_ref().serialize(serializer)
}
