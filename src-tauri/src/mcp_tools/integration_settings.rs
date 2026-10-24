//! Typed controls for the same running server shown in desktop settings.
use super::*;
impl Tools {
    pub(super) fn integration_tool(&self,name:&str,args:&Value)->Result<Value,Value>{
        let server=self.integration.get().and_then(std::sync::Weak::upgrade).ok_or_else(||failure("unavailable","The running MCP server is unavailable.",false))?;
        let revision=||args["expectedSettingsRevision"].as_u64().ok_or_else(||failure("invalid_arguments","Provide the exact settings revision.",false));
        let result=match name {
            "mcp_settings_get"=>Ok(server.status()),
            "mcp_settings_update"=>server.configure(crate::mcp::SettingsUpdate{expected_settings_revision:revision()?,enabled:args["enabled"]==true,port:args["port"].as_u64().unwrap() as u16}),
            "mcp_credential_create"=>server.create_credential(args["name"].as_str().unwrap().into(),args["scope"].as_str().unwrap().into(),revision()?),
            "mcp_credential_rotate"=>server.rotate_credential(args["id"].as_str().unwrap(),revision()?),
            "mcp_credential_revoke"=>server.revoke_credential(args["id"].as_str().unwrap(),revision()?),
            _=>return Err(failure("unknown_tool","Unknown integration settings tool.",false)),
        };
        match result {Ok(value)=>Ok(value),Err(error)=>native_result(json!({"ok":false,"error":error}),None)}
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn integration_controls_share_native_revisions_and_atomic_credential_replacement(){
        let server=crate::mcp::Server::open(Arc::new(GatewayManager::mcp_test_manager()),None);
        let tools=Tools::new(Arc::new(GatewayManager::mcp_test_manager()));tools.bind_integration(Arc::downgrade(&server));let client="integration-settings-test";
        let state=tools.call("mcp_settings_get",json!({}),client);assert_eq!(state["data"]["settingsRevision"],0);
        let args=json!({"operationId":uuid::Uuid::new_v4().to_string(),"name":"Test assistant","scope":"read","expectedSettingsRevision":0});
        let created=tools.call("mcp_credential_create",args.clone(),client);assert_eq!(created["ok"],true,"{created}");assert_eq!(tools.call("mcp_credential_create",args,client),created);
        let id=created["data"]["credential"]["id"].as_str().unwrap();let rotated=tools.call("mcp_credential_rotate",json!({"operationId":uuid::Uuid::new_v4().to_string(),"id":id,"expectedSettingsRevision":1}),client);
        assert_eq!(rotated["data"]["status"]["credentials"].as_array().unwrap().len(),1);assert_ne!(rotated["data"]["credential"]["id"],id);
        assert_eq!(tools.call("mcp_credential_revoke",json!({"operationId":uuid::Uuid::new_v4().to_string(),"id":rotated["data"]["credential"]["id"],"expectedSettingsRevision":1}),client)["error"]["code"],"revision_conflict");
        let deleted=tools.call("mcp_credential_revoke",json!({"operationId":uuid::Uuid::new_v4().to_string(),"id":rotated["data"]["credential"]["id"],"expectedSettingsRevision":2}),client);assert_eq!(deleted["data"]["credentials"],json!([]));
        assert!(!server.status().to_string().contains(created["data"]["token"].as_str().unwrap()));server.shutdown();
    }
}
