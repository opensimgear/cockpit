use super::*;
impl GatewayManager {
    pub(super) fn activity_profile_mutation(profiles:&mut osg_profiles::service::Service,request:&Value)->Result<Value,String> {
        let command=request["command"].as_str().unwrap_or("");
        let is_edit=Self::activity_profile_document_command(command);
        let assignments=matches!(command,"profile_editor_save"|"profile_draft_save"|"profile_assignments_save");
        let before=is_edit.then(||profiles.compile_source(request["id"].as_str().or_else(||request["document"]["id"].as_str()).unwrap_or("")).ok()).flatten().and_then(|(document,local,_)| {
            let mut value=serde_json::to_value(document).ok()?;if assignments{value["assignments"]=json!(local);}Some(crate::activity::changes::profile_snapshot(&value))
        }).unwrap_or(Value::Null);
        let mut response=profiles.request(request)?;
        if is_edit {
            let mut after=response["document"].clone();if assignments {after["assignments"]=response["assignments"].clone();}
            response["activityChange"]=json!({"before":before,"after":crate::activity::changes::profile_snapshot(&after)});
        }
        Ok(response)
    }
    fn activity_profile_document_command(command:&str)->bool {
        matches!(command,"profile_create"|"profile_duplicate"|"profile_import"|"profile_save"|"profile_editor_save"|"profile_draft_save"|"profile_assignments_save"|"profile_rename"|"profile_delete")
    }
    pub(super) fn audit_activity_response(&self,request:&Value,response:&mut Value,context:Option<crate::activity::context::Context>)->bool {
        let change=response.get_mut("state").and_then(|state|state.get_mut("profiles")).and_then(Value::as_object_mut).and_then(|profiles|profiles.remove("activityChange"));
        if let Some(change)=change {
            let context=crate::activity::context::Context{kind:"profile",before:change["before"].clone()};
            let mut audit=response.clone();audit["state"]["activityChangeAfter"]=change["after"].clone();
            return self.audit_activity(request,&audit,Some(&context));
        }
        self.audit_activity(request,response,context.as_ref())
    }
    pub(super) fn observe_activity_session(&self) {
        let observer=self.activity.session_observer();
        // Cancellation and cleanup progress must not wait for takeover locks.
        // The coordinator worker also observes every native lifecycle change.
        if let Ok(profiles)=self.profiles.try_lock() {if let Ok(mut observer)=observer.try_lock() {observer.names(profiles.saved_documents());}}
        if let Ok(coordinator)=self.profile_session.try_lock() {if let Ok(mut observer)=observer.try_lock() {observer.observe(coordinator.status());}}
    }
    fn activity_automatic_settings(&self,request:&Value)->Value {
        let saved=self.profile_automatic.settings_snapshot()["settings"].clone();
        match request["command"].as_str().unwrap_or("") {
            "profile_selection_mode"=>json!({"mode":saved["mode"]}),
            "profile_game_rule_save"|"profile_game_rule_remove"=>json!({"rule":saved["rules"].as_array().into_iter().flatten().find(|rule|rule["identity"]["gameId"]==request["gameId"])}),
            _=>json!({"profileId":saved["preferredProfiles"][request["gameId"].as_str().unwrap_or("")]}),
        }
    }
    pub(crate) fn activity_mutation(&self)->std::sync::MutexGuard<'_,()> {self.activity.mutation_guard()}
    pub(super) fn activity_context_command(request:&Value)->bool {
        if Self::activity_profile_document_command(request["command"].as_str().unwrap_or("")) {return false;}
        matches!(request["command"].as_str(),Some("profile_create"|"profile_duplicate"|"profile_import"|"profile_save"|"profile_editor_save"|"profile_draft_save"|"profile_assignments_save"|"profile_rename"|"profile_delete"|"game_add"|"game_rescan"|"game_discovery_apply"|"game_tag_definitions_save"|"game_save"|"game_remove"|"game_installation_select"|"game_tags_save"|"game_background_save"|"ui_preferences_save"|"activity_settings_save"|"game_integration_settings_save"|"profile_local_settings_save"|"profile_selection_mode"|"profile_game_rule_save"|"profile_game_rule_remove"|"profile_game_preference_save"|"profile_game_preference_remove"|"companion_publish_commit"))
    }
    /// Capture metadata before dispatch. Emergency and read-only controls never
    /// take additional coordinator or inventory locks for activity collection.
    pub(super) fn activity_before(&self, request: &Value) -> Option<crate::activity::context::Context> {
        use crate::activity::{changes::profile_snapshot, context::Context};
        let command=request["command"].as_str()?;
        if Self::activity_profile_document_command(command) {return None;}
        let id=request["id"].as_str().or_else(||request["profileId"].as_str()).or_else(||request["document"]["id"].as_str());
        let (kind,before)=match command {
            "profile_save"|"profile_editor_save"|"profile_draft_save"|"profile_assignments_save"|"profile_rename"|"profile_delete" => {
                let profiles=self.profiles.lock().unwrap_or_else(|p|p.into_inner());
                let (document,assignments,_)=profiles.compile_source(id?).ok()?;
                let mut value=serde_json::to_value(document).ok()?;
                if matches!(command,"profile_editor_save"|"profile_draft_save"|"profile_assignments_save") {value["assignments"]=json!(assignments);}
                ("profile",profile_snapshot(&value))
            }
            "profile_create"|"profile_duplicate"|"profile_import" => ("profile",Value::Null),
            "game_save"|"game_remove"|"game_installation_select"|"game_tags_save"|"game_background_save" =>
                ("game",crate::activity::changes::game_snapshot(&self.games.activity_game(request["gameId"].as_str().or(id).unwrap_or("")))),
            "ui_preferences_save" if request["key"]!="remembered" =>
                ("ui_preferences",self.ui_preferences.read(request["key"].as_str()?).ok()?.value),
            "activity_settings_save" => ("activity_settings",self.activity.request(&json!({"command":"activity_settings_get"})).ok()?["activitySettings"].clone()),
            "game_integration_settings_save" => ("integration",self.games.local_provider_settings(request["gameId"].as_str()? ).0),
            "profile_local_settings_save" => ("configuration",self.local_settings_snapshot(request).ok()?["settings"].clone()),
            "profile_selection_mode"|"profile_game_rule_save"|"profile_game_rule_remove"|"profile_game_preference_save"|"profile_game_preference_remove" =>
                ("configuration",self.activity_automatic_settings(request)),
            _=>return None,
        };
        Some(Context{kind,before})
    }
    pub(super) fn audit_activity(&self,request:&Value,response:&Value,context:Option<&crate::activity::context::Context>)->bool {
        if context.is_none_or(|context|!matches!(context.kind,"configuration"|"integration")) {return self.activity.audit_with_context(request,response,context);}
        let mut captured=response.clone();
        if response["ok"]==true {if let Some(context)=context {
            let after=match context.kind {
                "configuration" if request["command"]=="profile_local_settings_save" => response["state"]["localSettings"].get("settings").cloned(),
                "configuration"=>Some(self.activity_automatic_settings(request)),
                "integration"=>request["gameId"].as_str().map(|id|self.games.local_provider_settings(id).0),
                _=>None,
            };
            if let Some(after)=after {captured["state"]["activityChangeAfter"]=after;}
        }}
        self.activity.audit_with_context(request,&captured,context)
    }
    pub(crate) fn record_external_change(&self,command:&str,id:Option<&str>,name:Option<&str>,before:&Value,after:&Value,error:Option<&str>,source:&str) {
        let client=if source=="mcp" {"cockpit-mcp-native"} else {"cockpit-native-settings"};
        let context=crate::activity::context::Context{kind:"configuration",before:before.clone()};
        self.activity.audit_with_context(&json!({"command":command,"id":id,"name":name,"clientId":client}),
            &json!({"ok":error.is_none(),"error":error,"state":{"activityChangeAfter":after}}),Some(&context));
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
