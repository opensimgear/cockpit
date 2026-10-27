use super::*;
impl GatewayManager {
    pub(crate) fn record_external_activity(&self, command: &str, success: bool) {
        self.record_activity(command,None,None,(!success).then_some("Native settings change was rejected; inspect the related settings for details."),if command.starts_with("mcp_"){"mcp"}else{"cockpit"});
    }
    /// Command owners that do not use Gateway requests call this with operation
    /// metadata only. Settings values, credentials and raw payloads are excluded.
    pub(crate) fn record_activity(&self, command: &str, id: Option<&str>, name: Option<&str>, error: Option<&str>, source: &str) {
        let client=if source=="mcp" {"cockpit-mcp-native"} else {"cockpit-native-settings"};
        self.activity.audit(&json!({"command":command,"id":id,"name":name,"clientId":client}), &json!({"ok":error.is_none(),"error":error}));
    }
    pub(super) fn start_activity(&self) {
        let games = self.games.clone();
        let profiles = self.profiles.clone();
        let observer = self.activity.session_observer();
        let gateways = self.sessions.clone();
        let controllers = self.controllers.clone();
        let companions = self.companion_host.clone();
        let modules = self.module_usb.clone();
        let provider = self.provider.clone();
        self.activity.observe(move || {
            let mut snapshot = crate::activity::Snapshot::default();
            let game_state=games.activity_snapshot();
            if let Some(catalogue)=game_state["catalogue"].as_array(){snapshot.games(catalogue);}
            snapshot.discovery(&game_state["discovery"]);
            let profiles = profiles.lock().unwrap_or_else(|p| p.into_inner());
            observer.lock().unwrap_or_else(|p|p.into_inner()).names(profiles.saved_documents());
            snapshot.profiles(profiles.saved_documents());
            drop(profiles);
            let mut present=std::collections::BTreeSet::new();
            match provider.ports() {
                Ok(ports)=>for port in ports {if let Some((_,id))=candidate(&port) {let name=port.friendly_name.as_deref().unwrap_or(&id);snapshot.insert("devices","gateway",&id,name,json!({"present":true,"connected":false}));present.insert(id);}},
                Err(_)=>snapshot.unavailable("gateway"),
            }
            let adapters = gateways.lock().unwrap_or_else(|p| p.into_inner()).iter().map(|(id, adapter)| (id.clone(), adapter.clone())).collect::<Vec<_>>();
            for (id, adapter) in adapters {
                let state = adapter.cached_state();
                snapshot.insert("devices", "gateway", &id, &id, json!({"present":present.contains(&id),"connected":state["connected"],"running":state["running"]}));
                for module in state["modules"].as_array().into_iter().flatten() {
                    let module_id = module["id"].as_str().or_else(|| module["moduleId"].as_str()).unwrap_or("module");
                    let mut module = module.clone();
                    module["id"] = json!(format!("{id}:{module_id}"));
                    snapshot.devices("module", &[module]);
                }
            }
            for offset in (0..=256).step_by(32) {
                let Ok(state)=controllers.request(&json!({"command":"controller_list","offset":offset})) else {snapshot.unavailable("controller");break};
                let Some(rows)=state["sources"].as_array() else {snapshot.unavailable("controller");break};
                snapshot.devices("controller", rows);
                if offset+rows.len()>=state["total"].as_u64().unwrap_or(0) as usize || rows.is_empty() {break;}
            }
            for module in modules.snapshots() {
                snapshot.insert("devices", "module", &module.key, &module.key, json!({"connected":module.snapshot.connected,"present":module.present,"identity":module.snapshot.identity,"route":module.snapshot.route}));
            }
            match companions.resource_companions() {
                Ok(companions)=>for companion in companions { snapshot.insert("companions", "companion", &companion.companion_id, &companion.name, json!({"connected":companion.connected,"instanceId":companion.instance_id,"name":companion.name})); },
                Err(_)=>snapshot.unavailable("companion"),
            }
            snapshot
        });
    }
    pub(super) fn activity_request(&self, request: &Value, authorization: Option<&McpAuthorization>) -> Value {
        if self.closed.load(Ordering::Acquire) || request.to_string().len() > 16384 || !request["clientId"].as_str().is_some_and(|id| crate::protocol::identifier(id, 16, 80)) {
            return self.error("Invalid activity request or closed cockpit.", None);
        }
        let _admission = self.profile_admission.read().unwrap_or_else(|p| p.into_inner());
        if let Err(error) = self.check_mcp_authorization(authorization) { return self.error(&error, None); }
        match self.activity.request(request) { Ok(state) => json!({"ok":true,"state":state}), Err(error) => self.error(&error, None) }
    }
}
#[cfg(test)]
#[path="gateway_tests.rs"]
mod activity_tests;
