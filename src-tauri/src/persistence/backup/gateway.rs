//! Typed UI/MCP gateway. All native effects finish before restore is queued.
use super::*;
impl GatewayManager {
    pub(super) fn storage_request(
        &self,
        request: &Value,
        authorization: Option<&McpAuthorization>,
    ) -> Value {
        let result: Result<Value, String> = (|| {
            self.check_mcp_authorization(authorization)?;
            let command = request["command"].as_str().unwrap_or("");
            let fields = match command {
                "storage_status" | "storage_restart" => vec!["command", "clientId"],
                "storage_create" => vec!["command", "clientId", "expectedRevision"],
                "storage_remove" => vec!["command", "clientId", "expectedRevision", "id"],
                "storage_restore" => {
                    vec!["command", "clientId", "expectedRevision", "id", "confirmed"]
                }
                _ => return Err("Unknown storage operation.".into()),
            };
            if request
                .as_object()
                .is_none_or(|object| object.keys().any(|key| !fields.contains(&key.as_str())))
            {
                return Err("Unknown storage request field.".into());
            }
            if command == "storage_status" {
                return self.storage.status();
            }
            if command == "storage_restart" {
                return self.storage.restart();
            }
            if self.closed.load(Ordering::Acquire) {
                return Err("Cockpit is stopped for restore. Restart to continue.".into());
            }
            let expected = request["expectedRevision"]
                .as_u64()
                .ok_or("Provide the exact backup inventory revision.")?;
            match command {
                "storage_create" => self.storage.create(expected),
                "storage_remove" => self.storage.remove(
                    request["id"].as_str().ok_or("Choose a saved backup.")?,
                    expected,
                ),
                "storage_restore" => {
                    if request["confirmed"] != true {
                        return Err("Confirm replacement of saved settings and invalidation of current credentials.".into());
                    }
                    let id = request["id"].as_str().ok_or("Choose a saved backup.")?;
                    self.storage.verify(id, expected)?;
                    {
                        let _profile = self
                            .profile_admission
                            .write()
                            .unwrap_or_else(|p| p.into_inner());
                        let _game = self
                            .game_admission
                            .write()
                            .unwrap_or_else(|p| p.into_inner());
                        self.check_mcp_authorization(authorization)?;
                        self.guard_learning()?;
                        let session = self
                            .profile_session
                            .lock()
                            .unwrap_or_else(|p| p.into_inner());
                        if session.status().state != crate::profile_session::State::Stopped
                            || session.status().owner.is_some()
                            || self.profile_pending.load(Ordering::Acquire) != 0
                            || self.games.active()
                            || self.games.recovery_pending()
                        {
                            return Err("Stop the profile and finish recovery or game setup before restoring a backup.".into());
                        }
                        self.closed.store(true, Ordering::Release);
                        self.profile_closed.store(true, Ordering::Release);
                    }
                    self.shutdown();
                    if self
                        .profile_session
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .status()
                        .state
                        != crate::profile_session::State::Stopped
                    {
                        return Err("Cleanup requires attention. Current data was preserved; restart and recover before restoring.".into());
                    }
                    self.storage.queue(id, expected)
                }
                _ => Err("Unknown storage operation.".into()),
            }
        })();
        match result {
            Ok(value) => json!({"ok":true,"state":{"storage":value}}),
            Err(error) => json!({"ok":false,"error":error}),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restore_requires_confirmation_stopped_owner_and_closed_runtime_before_queue() {
        let directory = crate::test_directory();
        let database =
            osg_persistence::Database::open(directory.path().join("cockpit.sqlite3")).unwrap();
        let mut manager = GatewayManager::mcp_test_manager();
        manager.storage = crate::persistence::backup::Runtime::fixture(
            database.clone(),
            directory.path().to_owned(),
        );
        let request = |command: &str, fields: Value| {
            let mut request = fields;
            request["command"] = json!(command);
            request["clientId"] = json!("storage-native-unit-test");
            manager.request(request)
        };
        assert_eq!(
            request("storage_status", json!({}))["state"]["storage"]["revision"],
            0
        );
        assert_eq!(request("storage_create", json!({}))["ok"], false);
        assert_eq!(
            request(
                "storage_create",
                json!({"expectedRevision":0,"path":"unsafe"})
            )["ok"],
            false
        );
        let created = request("storage_create", json!({"expectedRevision":0}));
        assert_eq!(created["ok"], true, "{created}");
        let id = &created["state"]["storage"]["backup"]["id"];
        assert_eq!(
            request("storage_restore", json!({"id":id,"expectedRevision":1}))["ok"],
            false
        );
        let applied = request(
            "profile_apply",
            json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1}),
        );
        assert_eq!(applied["ok"], true, "{applied}");
        let blocked = request(
            "storage_restore",
            json!({"id":id,"expectedRevision":1,"confirmed":true}),
        );
        assert_eq!(blocked["ok"], false);
        assert!(!manager.closed.load(Ordering::Acquire));
        assert!(database
            .read("storage", "pending-restore")
            .unwrap()
            .is_none());
        assert_eq!(request("profile_stop", json!({}))["ok"], true);
        let queued = request(
            "storage_restore",
            json!({"id":id,"expectedRevision":1,"confirmed":true}),
        );
        assert_eq!(queued["ok"], true, "{queued}");
        assert!(manager.closed.load(Ordering::Acquire));
        assert!(database
            .read("storage", "pending-restore")
            .unwrap()
            .is_some());
        assert_eq!(
            request("storage_remove", json!({"id":id,"expectedRevision":1}))["ok"],
            false
        );
        assert_eq!(
            request("storage_status", json!({}))["state"]["storage"]["restartRequired"],
            true
        );
    }
}
