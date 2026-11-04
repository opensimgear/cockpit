//! Passive native workspace boundary. No reads, commands or lease renewal.
use super::*;
use std::{collections::BTreeMap, sync::Weak};

const MAX_WATCHERS: usize = 8;
struct Snapshot {
    state: Arc<Value>,
    visible: Arc<Value>,
    owners: BTreeMap<String, String>,
    wakes: Vec<Weak<osg_streams::NativeWake>>,
    inventory_wake: Weak<osg_streams::NativeWake>,
}
pub(crate) struct Cache { snapshot: Mutex<Snapshot>, owned: AtomicBool }
impl Cache {
    pub(crate) fn new(state: Value) -> Self {
        let state = public_state(state);
        let visible = visible_state(&state);
        Self { snapshot: Mutex::new(Snapshot { state: Arc::new(state), visible: Arc::new(visible),
            owners: BTreeMap::new(), wakes: vec![], inventory_wake: Weak::new() }), owned: AtomicBool::new(false) }
    }
    pub(crate) fn watch(&self, wake: Arc<osg_streams::NativeWake>) {
        let mut snapshot = self.snapshot.lock().unwrap_or_else(|p| p.into_inner());
        snapshot.wakes.retain(|wake| wake.strong_count() > 0);
        let weak = Arc::downgrade(&wake);
        if snapshot.wakes.len() < MAX_WATCHERS && !snapshot.wakes.iter().any(|old| old.ptr_eq(&weak)) {
            snapshot.wakes.push(weak);
        }
    }
    pub(crate) fn snapshot(&self) -> (Arc<Value>, BTreeMap<String, String>) {
        let snapshot = self.snapshot.lock().unwrap_or_else(|p| p.into_inner());
        (snapshot.state.clone(), snapshot.owners.clone())
    }
    fn watch_inventory(&self, wake: &Arc<osg_streams::NativeWake>) {
        self.snapshot.lock().unwrap_or_else(|p| p.into_inner()).inventory_wake = Arc::downgrade(wake);
    }
    pub(crate) fn visible_snapshot(&self) -> (Arc<Value>, BTreeMap<String, String>) {
        let snapshot = self.snapshot.lock().unwrap_or_else(|p| p.into_inner());
        (snapshot.visible.clone(), snapshot.owners.clone())
    }
    pub(crate) fn set_owned(&self, owned: bool) { self.owned.store(owned, Ordering::Release); }
    pub(crate) fn owned(&self) -> bool { self.owned.load(Ordering::Acquire) }
    pub(crate) fn commit(&self, state: Value, owners: BTreeMap<String, String>) {
        let state = public_state(state);
        let visible = visible_state(&state);
        let (wakes, inventory) = {
            let mut snapshot = self.snapshot.lock().unwrap_or_else(|p| p.into_inner());
            let visible_changed = snapshot.visible.as_ref() != &visible;
            let changed = visible_changed || snapshot.owners != owners;
            let inventory = snapshot.inventory_wake.upgrade().filter(|_| {
                visible_changed && inventory_changed(&snapshot.state, &state)
                    || state["connected"] == true && crate::profile_resource_sources::hid_publication::freshness_restored(
                        snapshot.state["nativeSnapshotAtMs"].as_u64(),
                        state["nativeSnapshotAtMs"].as_u64(),
                        crate::profile_resource_sources::hid_publication::now_ms(),
                    )
            });
            if snapshot.state.as_ref() != &state { snapshot.state = Arc::new(state); }
            if visible_changed { snapshot.visible = Arc::new(visible); }
            snapshot.owners = owners;
            snapshot.wakes.retain(|wake| wake.strong_count() > 0);
            let wakes = if changed { snapshot.wakes.iter().filter_map(Weak::upgrade).collect::<Vec<_>>() } else { vec![] };
            (wakes, inventory)
        };
        for wake in wakes { wake.notify(); }
        if let Some(wake) = inventory { wake.notify(); }
    }
}

#[path = "workspace/inventory.rs"]
mod inventory;
use inventory::inventory_changed;
fn public_state(mut state: Value) -> Value {
    fn scrub(value: &mut Value) {
        match value {
            Value::Object(object) => {
                // Configuration owner integers and client UUIDs are authority;
                // output receipts/capability metadata are already public values.
                for key in ["clientId", "ownerClient", "owner", "token", "ownershipToken"] { object.remove(key); }
                for value in object.values_mut() { scrub(value); }
            }
            Value::Array(values) => for value in values { scrub(value); },
            _ => {}
        }
    }
    scrub(&mut state);
    state
}
fn visible_state(state: &Value) -> Value {
    let mut visible = state.clone();
    if let Some(object) = visible.as_object_mut() { object.remove("nativeSnapshotAtMs"); }
    visible
}

impl Stm32Service {
    pub(crate) fn watch_resource_inventory(&self, wake: &Arc<osg_streams::NativeWake>) { self.workspace.watch_inventory(wake); }
    pub(crate) fn watch_workspace(&self, wake: Arc<osg_streams::NativeWake>) { self.workspace.watch(wake); }
    pub(crate) fn workspace_snapshot(&self) -> (Arc<Value>, BTreeMap<String, String>) { self.workspace.visible_snapshot() }
    pub(crate) fn set_workspace_owned(&self, owned: bool) { self.workspace.set_owned(owned); }
    pub(crate) fn renew_workspace_client(&self, client: &str) -> Result<Vec<String>, String> {
        if self.closed.load(Ordering::Acquire) { return Err(CLOSED.into()); }
        if !crate::protocol::identifier(client, 16, 80) { return Err("Invalid workspace client identity.".into()); }
        let (reply, received) = mpsc::sync_channel(1);
        self.sender.try_send(Message::RenewWorkspace(client.into(), reply)).map_err(|error| match error {
            mpsc::TrySendError::Full(_) => "CAN gateway is busy; wait for the current request.".to_string(),
            mpsc::TrySendError::Disconnected(_) => CLOSED.into(),
        })?;
        received.recv_timeout(Duration::from_secs(15)).map_err(|_| "Workspace claim renewal did not complete before its deadline.".to_string())?
    }
}
impl Gateway {
    pub(super) fn renew_workspace(&mut self, client: &str) -> Result<Vec<String>, String> {
        if self.cancel.load(Ordering::Acquire) { return Err(CLOSED.into()); }
        if !crate::protocol::identifier(client, 16, 80) { return Err("Invalid workspace client identity.".into()); }
        self.expire_claims(Some(client));
        let mut ids = self.claims.iter().filter(|(_, claim)| claim.client == client).map(|(id, _)| id.clone()).collect::<Vec<_>>();
        ids.sort();Ok(ids)
    }
    pub(super) fn cache_workspace(&self, latest: &Mutex<Value>, claims: &Mutex<Vec<(Instant, crate::module_configuration_routes::WriterPin)>>, workspace: &Cache) {
        let state = self.decorated(None);
        let owners = self.claims.iter().filter(|(_, claim)| claim.seen.elapsed() <= self.timing.claim_timeout)
            .map(|(id, claim)| (id.clone(), claim.client.clone())).collect();
        *claims.lock().unwrap_or_else(|p| p.into_inner()) = self.configuration_claims();
        *latest.lock().unwrap_or_else(|p| p.into_inner()) = state.clone();
        workspace.commit(state, owners);
    }
}

#[cfg(test)]
mod tests;
