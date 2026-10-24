//! Typed listener setup uses the same acknowledged native boundary as the portal.
use super::*;
impl Tools {
    pub(super) fn listener_tool(&self,name:&str,args:&Value,client:&str)->Result<Value,Value>{
        let state=if name=="companion_listener_get" {
            self.native("companion_status",json!({}),client,Some("companions"))?
        } else {
            let enabled=args["enabled"]==true;
            let mut settings=select(args,&["expectedSettingsRevision","address","port","advertisedHost","discovery"]);
            if enabled && ["address","port","advertisedHost","discovery"].iter().any(|key|settings.get(*key).is_none()) {
                return Err(failure("invalid_arguments","Enabling requires address, port, advertisedHost and discovery.",false));
            }
            if !enabled {settings=select(args,&["expectedSettingsRevision"]);}
            self.native(if enabled {"companion_enable"}else{"companion_disable"},json!({"companion":settings}),client,Some("companions"))?
        };
        Ok(state["listener"].clone())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn listener_task_shapes_are_typed_and_revisions_reach_the_native_boundary(){
        struct BackendFixture;
        impl Backend for BackendFixture {
            fn request(&self,request:Value)->Value {
                assert!(matches!(request["command"].as_str(),Some("companion_status"|"companion_enable"|"companion_disable")));
                if request["command"]!="companion_status" {assert_eq!(request["companion"]["expectedSettingsRevision"],7);assert!(request["companion"].get("operationId").is_none());}
                json!({"ok":true,"state":{"companions":{"listener":{"settingsRevision":7,"enabled":request["command"]=="companion_enable"}}}})
            }
            fn guarded(&self,_:Value,_:Value)->Value{panic!("Listener setup never enters output ownership")}
        }
        let tools=Tools{backend:Arc::new(BackendFixture),ledger:Mutex::new(Default::default()),drafts:Mutex::new(vec![]),workspace_uploads:Mutex::new(vec![]),integration:OnceLock::new()};
        let client="listener-shape-test";
        assert_eq!(tools.call("companion_listener_get",json!({}),client)["data"]["settingsRevision"],7);
        let incomplete=tools.call("companion_listener_save",json!({"operationId":uuid::Uuid::new_v4().to_string(),"enabled":true,"expectedSettingsRevision":7}),client);assert_eq!(incomplete["error"]["code"],"invalid_arguments");
        for enabled in [true,false] {
            let saved=tools.call("companion_listener_save",json!({"operationId":uuid::Uuid::new_v4().to_string(),"enabled":enabled,"expectedSettingsRevision":7,"address":"127.0.0.1","advertisedHost":"localhost","port":19443,"discovery":false}),client);assert_eq!(saved["data"]["enabled"],enabled,"{saved}");
        }
    }
    #[cfg(windows)]
    #[test]
    fn mcp_listener_settings_use_exact_revisions_and_disable_the_real_listener(){
        let tools=Tools::new(Arc::new(GatewayManager::mcp_test_manager()));let client="listener-mcp-test";
        let initial=tools.call("companion_listener_get",json!({}),client);assert_eq!(initial["ok"],true,"{initial}");
        let revision=initial["data"]["settingsRevision"].clone();
        let reservation=std::net::TcpListener::bind("127.0.0.1:0").unwrap();let port=reservation.local_addr().unwrap().port();drop(reservation);
        let settings=json!({"operationId":uuid::Uuid::new_v4().to_string(),"enabled":true,"address":"127.0.0.1","port":port,"advertisedHost":"127.0.0.1","discovery":false,"expectedSettingsRevision":revision});
        let enabled=tools.call("companion_listener_save",settings.clone(),client);assert_eq!(enabled["ok"],true,"{enabled}");assert_eq!(enabled["data"]["enabled"],true);assert_eq!(tools.call("companion_listener_save",settings,client),enabled);
        let stale=tools.call("companion_listener_save",json!({"operationId":uuid::Uuid::new_v4().to_string(),"enabled":false,"expectedSettingsRevision":revision}),client);assert_eq!(stale["error"]["code"],"revision_conflict");
        let disabled=tools.call("companion_listener_save",json!({"operationId":uuid::Uuid::new_v4().to_string(),"enabled":false,"expectedSettingsRevision":enabled["data"]["settingsRevision"]}),client);assert_eq!(disabled["data"]["configuredEnabled"],false);assert_eq!(disabled["data"]["enabled"],false);
    }
}
