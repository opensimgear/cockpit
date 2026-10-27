use serde_json::Value;

/// Subjects an operation can change. This defers observations while the native
/// operation is in flight; only its acknowledged terminal states are consumed.
#[derive(Default)]
pub(super) struct Scope(Vec<(&'static str,Option<String>)>);
impl Scope {
    pub fn request(request:&Value)->Self {
        let command=request["command"].as_str().unwrap_or("");
        let id=|fields:&[&str]|fields.iter().find_map(|field|request[*field].as_str().map(str::to_owned));
        let mut scope=Self::default();
        let mut add=|kind,target|scope.0.push((kind,target));
        match command {
            "profile_create"|"profile_duplicate"|"profile_import"=>add("profile",None),
            "profile_save"|"profile_editor_save"|"profile_draft_save"|"profile_assignments_save"|"profile_rename"|"profile_delete"=>
                add("profile",id(&["id","profileId"]).or_else(||request["document"]["id"].as_str().map(str::to_owned))),
            "profile_apply"|"profile_stop"|"profile_recover"=>add("profile_session",Some("current".into())),
            "profile_game_launch"=>{add("profile_session",Some("current".into()));add("game_process",id(&["gameId"]));},
            "game_add"|"game_tag_definitions_save"=>add("game",None),
            "game_discovery_apply"=>{add("game",None);add("discovery",Some("current".into()));},
            "game_save"|"game_remove"|"game_installation_select"|"game_tags_save"|"game_background_save"=>add("game",id(&["gameId","id"])),
            "game_discovery_start"=>add("discovery",Some("current".into())),
            "game_rescan"=>{add("game",None);add("discovery",Some("current".into()));},
            _ if command.starts_with("game_integration_")||command.starts_with("integration_")||command.starts_with("dcs_")=>add("integration",id(&["gameId","id"])),
            _ if command.starts_with("controller_")=>{
                if let Some(ids)=request["sourceIds"].as_array(){for id in ids.iter().filter_map(Value::as_str){add("controller",Some(id.into()));}}
                else{add("controller",id(&["sourceId","id"]));}
            }
            _ if command.starts_with("module_usb_")=>add("module",id(&["key","moduleId","id"])),
            _ if command.starts_with("companion_")=>add("companion",id(&["companionId","id"]).or_else(||request["companion"]["companionId"].as_str().map(str::to_owned))),
            "connect"|"disconnect"|"reconnect"|"start"|"stop"|"apply"|"save"|"update-profile"|"claim"|"release"|"forget"|"register"|"unregister"|"enable"|"disable"|"configure"|"config_apply"|"config_apply_save"|"config_save"|"config_discard"|"config_reset"|"firmware_flash"|"flash"=>{
                let gateway=id(&["gatewayId"]);
                let module=id(&["moduleId"]);
                if let Some(module)=module {add("module",gateway.as_ref().map(|gateway|format!("{gateway}:{module}")));}
                else {add("gateway",gateway);}
            }
            _=>(),
        }
        scope
    }
    pub fn covers(&self,subject:&str)->bool {
        self.0.iter().any(|(kind,id)|subject.strip_prefix(&format!("{kind}:")).is_some_and(|actual|id.as_deref().is_none_or(|id|id==actual)))
    }
    pub fn watches(&self,kind:&str)->bool {self.0.iter().any(|(watched,_)|*watched==kind)}
}

#[cfg(test)]
#[path="scopes_tests.rs"]
mod tests;
