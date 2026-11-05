//! Activity reads prepared caches only. Each closure runs on its writer after
//! a committed source notification, never on a producer or input callback.
use super::*;
use crate::activity::{metadata::Group, Snapshot};

impl GatewayManager {
    pub(in crate::gateways) fn start_activity(&self) {
        let games = self.games.clone();
        self.activity.observe_group(Group::Games, move || {
            let mut snapshot = Snapshot::default();
            let state = games.activity_snapshot();
            if let Some(catalogue) = state["catalogue"].as_array() { snapshot.games(catalogue); }
            else { for kind in ["game", "integration", "game_process", "game_session", "game_vehicle"] { snapshot.unavailable(kind); } }
            snapshot.discovery(&state["discovery"]);
            snapshot
        });
        let profiles = self.profiles.clone();
        let observer = self.activity.session_observer();
        self.activity.observe_group(Group::Profiles, move || {
            let mut snapshot = Snapshot::default();
            let profiles = profiles.lock().unwrap_or_else(|p| p.into_inner());
            observer.lock().unwrap_or_else(|p|p.into_inner()).names(profiles.saved_documents());
            snapshot.profiles(profiles.saved_documents());
            snapshot
        });
        let topology = self.serial_topology.clone();
        let gateways = self.sessions.clone();
        let controllers = self.controllers.clone();
        let modules = self.module_usb.clone();
        self.activity.observe_group(Group::Devices, move || {
            let mut snapshot = Snapshot::default();
            let mut present = std::collections::BTreeSet::new();
            let known_ports = match topology.cached() {
                Ok(ports) => {
                    snapshot.mark_kind("gateway");
                    for port in ports { if let Some((_, id)) = candidate(&port) {
                        snapshot.insert("devices", "gateway", &id, port.friendly_name.as_deref().unwrap_or(&id), json!({"present":true,"connected":false}));
                        present.insert(id);
                    }}
                    true
                }
                Err(_) => { snapshot.unavailable("gateway"); false }
            };
            snapshot.mark_kind("module");
            let adapters = gateways.lock().unwrap_or_else(|p| p.into_inner()).iter()
                .map(|(id, adapter)| (id.clone(), adapter.clone())).collect::<Vec<_>>();
            for (id, adapter) in adapters {
                let state = adapter.cached_state();
                if known_ports {
                    snapshot.insert("devices", "gateway", &id, &id, json!({"present":present.contains(&id),"connected":state["connected"],"running":state["running"]}));
                }
                for module in state["modules"].as_array().into_iter().flatten() {
                    let module_id = module["id"].as_str().or_else(|| module["moduleId"].as_str()).unwrap_or("module");
                    let mut module = module.clone();
                    module["id"] = json!(format!("{id}:{module_id}"));
                    snapshot.devices("module", &[module]);
                }
            }
            let mut rows = vec![];
            let mut complete = false;
            for offset in (0..=256).step_by(32) {
                let state = controllers.cached_sources_page(offset);
                let Some(page) = state["sources"].as_array() else { break; };
                rows.extend(page.iter().cloned());
                if offset + page.len() >= state["total"].as_u64().unwrap_or(0) as usize {
                    complete = true; break;
                }
                if page.is_empty() { break; }
            }
            if complete { snapshot.devices("controller", &rows); }
            else { snapshot.unavailable("controller"); }
            for module in modules.snapshots() {
                snapshot.insert("devices", "module", &module.key, &module.key,
                    json!({"connected":module.snapshot.connected,"present":module.present,"identity":module.snapshot.identity,"route":module.snapshot.route}));
            }
            snapshot
        });
        let companions = self.companion_host.clone();
        self.activity.observe_group(Group::Companions, move || {
            let mut snapshot = Snapshot::default();
            match companions.cached_resource_companions() {
                Some(companions) => {
                    snapshot.mark_kind("companion");
                    for companion in companions {
                        snapshot.insert("companions", "companion", &companion.companion_id, &companion.name,
                            json!({"connected":companion.connected,"instanceId":companion.instance_id,"name":companion.name}));
                    }
                }
                None => snapshot.unavailable("companion"),
            }
            snapshot
        });
    }
}
