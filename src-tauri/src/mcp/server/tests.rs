    use super::*;
    pub(super) fn request(
        endpoint: &str,
        token: Option<&str>,
        payload: Value,
    ) -> Result<ureq::Response, ureq::Error> {
        let mut request = ureq::post(endpoint)
            .timeout(Duration::from_secs(10))
            .set("Accept", "application/json, text/event-stream")
            .set("MCP-Protocol-Version", "2025-11-25");
        if let Some(token) = token {
            request = request.set("Authorization", &format!("Bearer {token}"));
        }
        request.send_json(payload)
    }
    #[test]
    fn listener_authentication_scope_revocation_and_lifecycle() {
        let manager = Arc::new(GatewayManager::mcp_test_manager());
        let server = Server::open(manager, None);
        assert_eq!(server.status()["enabled"], false);
        let read = server
            .create_credential("Reader".into(), "read".into(),server.status()["settingsRevision"].as_u64().unwrap())
            .unwrap();
        let write = server
            .create_credential("Writer".into(), "write".into(),server.status()["settingsRevision"].as_u64().unwrap())
            .unwrap();
        let token = read["token"].as_str().unwrap();
        assert!(!server.status().to_string().contains(token));
        let socket = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = socket.local_addr().unwrap().port();
        drop(socket);
        server
            .configure(SettingsUpdate {
                expected_settings_revision:server.status()["settingsRevision"].as_u64().unwrap(),
                enabled: true,
                port,
            })
            .unwrap();
        let endpoint = format!("http://127.0.0.1:{port}/mcp");
        let payload = json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}});
        assert!(matches!(
            request(&endpoint, None, payload.clone()),
            Err(ureq::Error::Status(401, _))
        ));
        let read_list: Value = request(&endpoint, Some(token), payload.clone())
            .unwrap()
            .into_json()
            .unwrap();
        assert!(
            read_list["result"]["tools"].as_array().is_some(),
            "{read_list}"
        );
        assert!(!read_list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "profile_create"));
        let write_list: Value = request(&endpoint, write["token"].as_str(), payload)
            .unwrap()
            .into_json()
            .unwrap();
        assert!(write_list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "profile_create"));
        let denied:Value=request(&endpoint,Some(token),json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"profile_create","arguments":{"name":"Forbidden","operationId":uuid::Uuid::new_v4().to_string()}}})).unwrap().into_json().unwrap();
        assert!(denied.get("error").is_some(), "{denied}");
        let invalid_origin = ureq::post(&endpoint)
            .set("Authorization", &format!("Bearer {token}"))
            .set("Origin", "https://example.invalid")
            .set("Accept", "application/json, text/event-stream")
            .send_json(json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}));
        assert!(matches!(invalid_origin, Err(ureq::Error::Status(403, _))));
        let invalid_host = ureq::post(&endpoint)
            .set("Authorization", &format!("Bearer {token}"))
            .set("Host", "attacker.invalid")
            .set("Accept", "application/json, text/event-stream")
            .send_json(json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}));
        assert!(matches!(invalid_host, Err(ureq::Error::Status(403, _))));
        server
            .revoke_credential(read["credential"]["id"].as_str().unwrap(),server.status()["settingsRevision"].as_u64().unwrap())
            .unwrap();
        assert!(matches!(
            request(
                &endpoint,
                Some(token),
                json!({"jsonrpc":"2.0","id":4,"method":"tools/list"})
            ),
            Err(ureq::Error::Status(401, _))
        ));
        server
            .configure(SettingsUpdate {
                expected_settings_revision:server.status()["settingsRevision"].as_u64().unwrap(),
                enabled: false,
                port,
            })
            .unwrap();
        assert_eq!(server.status()["running"], false);
        server.shutdown();
    }
    #[test]
    fn occupied_port_does_not_destroy_existing_configuration() {
        let server = Server::open(Arc::new(GatewayManager::mcp_test_manager()), None);
        let occupied = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = occupied.local_addr().unwrap().port();
        assert!(server
            .configure(SettingsUpdate {
                expected_settings_revision:server.status()["settingsRevision"].as_u64().unwrap(),
                enabled: true,
                port
            })
            .is_err());
        assert_eq!(server.status()["enabled"], false);
        assert_eq!(server.status()["running"], false);
        assert!(server.status()["error"].is_string());
        assert!(server
            .configure(SettingsUpdate {
                expected_settings_revision:server.status()["settingsRevision"].as_u64().unwrap(),
                enabled: true,
                port: 80
            })
            .is_err());
        server.shutdown();
    }

#[test]
fn atomic_rotation_and_external_scope_changes_are_enforced_from_sqlite(){
 let root=crate::test_directory();let path=root.path().join("mcp-v1.json");let server=Server::open(Arc::new(GatewayManager::mcp_test_manager()),Some(path.clone()));
 let created=server.create_credential("Persisted writer".into(),"write".into(),0).unwrap();let id=created["credential"]["id"].as_str().unwrap();let old_token=created["token"].as_str().unwrap();
 let reservation=std::net::TcpListener::bind("127.0.0.1:0").unwrap();let port=reservation.local_addr().unwrap().port();drop(reservation);
 server.configure(SettingsUpdate{expected_settings_revision:1,enabled:true,port}).unwrap();let generation=server.state.lock().unwrap().listener.as_ref().unwrap().generation;
 assert!(server.authenticate(old_token,generation).is_some());let db=osg_persistence::Database::for_namespace(&path).unwrap();
 let lock=osg_persistence::test_support::hold_writer(&db).unwrap();assert!(server.rotate_credential(id,2).is_err());drop(lock);
 assert!(server.authenticate(old_token,generation).is_some());assert_eq!(server.status()["credentials"].as_array().unwrap().len(),1);
 let rotated=server.rotate_credential(id,2).unwrap();let new_id=rotated["credential"]["id"].as_str().unwrap();let token=rotated["token"].as_str().unwrap();
 assert!(server.authenticate(old_token,generation).is_none());let principal=server.authenticate(token,generation).unwrap();assert!(server.authorized(&principal,true));assert_eq!(rotated["status"]["settingsRevision"],3);
 assert!(server.create_credential("Stale".into(),"read".into(),2).is_err());
 let mut external=Store::open(Some(path.clone()));let mut next=external.settings.clone();next.credentials[0].scope="read".into();external.save(next).unwrap();
 assert!(!server.authorized(&principal,true));let reader=server.authenticate(token,generation).unwrap();assert!(!server.authorized(&reader,true));assert!(server.authorized(&reader,false));
 let mut next=external.settings.clone();next.credentials.retain(|c|c.id!=new_id);external.save(next).unwrap();assert!(server.authenticate(token,generation).is_none());
 let payload=db.read("security","mcp").unwrap().unwrap().payload;assert!(!String::from_utf8(payload).unwrap().contains("osg_mcp_"));
 let bytes=std::fs::read(db.path()).unwrap();assert!(!bytes.windows(old_token.len()).any(|b|b==old_token.as_bytes()));assert!(!bytes.windows(token.len()).any(|b|b==token.as_bytes()));
 server.shutdown();assert!(server.create_credential("Late".into(),"write".into(),external.revision).is_err());
}

#[test]
fn corrupt_sqlite_authority_fails_closed_and_recovery_requires_a_fresh_read(){
 let root=crate::test_directory();let path=root.path().join("mcp-v1.json");let manager=Arc::new(GatewayManager::mcp_test_manager());let server=Server::open(manager.clone(),Some(path.clone()));
 let created=server.create_credential("Recovery reader".into(),"read".into(),0).unwrap();let token=created["token"].as_str().unwrap();let id=created["credential"]["id"].as_str().unwrap();
 let reservation=std::net::TcpListener::bind("127.0.0.1:0").unwrap();let port=reservation.local_addr().unwrap().port();drop(reservation);server.configure(SettingsUpdate{enabled:true,port,expected_settings_revision:1}).unwrap();
 let generation=server.state.lock().unwrap().listener.as_ref().unwrap().generation;assert!(server.authenticate(token,generation).is_some());let db=osg_persistence::Database::for_namespace(&path).unwrap();let previous=db.read("security","mcp").unwrap().unwrap().payload;
 crate::persistence::test_support::record(&path,"security","mcp",b"invalid");assert_eq!(server.status()["readOnly"],true);assert!(server.authenticate(token,generation).is_none());assert!(server.create_credential("Blocked".into(),"write".into(),2).is_err());
 let client=mcp_tools::native_client(&format!("mcp-{id}"));let denied=manager.mcp_request(json!({"command":"widgets_list","clientId":client}));assert_eq!(denied["ok"],false);
 crate::persistence::test_support::record(&path,"security","mcp",&previous);assert_eq!(server.status()["readOnly"],false);assert!(server.authenticate(token,generation).is_some());
 server.shutdown();
}
