//! Workspace hooks share the selected adapter's committed cache, including
//! selection changes. Passive readers never take the CDC selection mutex.
use super::*;
use std::collections::BTreeMap;
impl PicoService {
    pub(crate) fn watch_workspace(&self, wake: Arc<osg_streams::NativeWake>) { self.workspace.watch(wake); }
    pub(crate) fn workspace_snapshot(&self) -> (Arc<Value>, BTreeMap<String, String>) { self.workspace.visible_snapshot() }
    pub(crate) fn set_workspace_owned(&self, owned: bool) { self.workspace.set_owned(owned); }
    pub(crate) fn renew_workspace_client(&self, client: &str) -> Result<Vec<String>, String> {
        if self.closed.load(Ordering::Acquire) { return Err(CLOSED.into()); }
        if !identifier(client, 16, 80) { return Err("Invalid workspace client identity.".into()); }
        let selected = match self.selected.try_lock() {
            Ok(selected) => selected,
            Err(std::sync::TryLockError::Poisoned(poison)) => poison.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return Err("The Pico gateway is busy. Wait for the current operation to finish.".into()),
        };
        match selected.as_ref() {
            Some(service) => service.renew_workspace_client(client),
            None => Ok(vec![]),
        }
    }
}
