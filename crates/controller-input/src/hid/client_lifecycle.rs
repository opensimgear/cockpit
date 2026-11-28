//! Credential retirement shares gateway admission with native HID operations.
use super::*;

impl Service {
    pub(crate) fn release_client(&self, client: &str) {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let owned: Vec<_> = sessions
            .iter()
            .filter(|(_, active)| active.owner.as_deref() == Some(client))
            .map(|(id, _)| id.clone())
            .collect();
        let removed: Vec<_> = owned.iter().filter_map(|id| sessions.remove(id)).collect();
        drop(sessions);
        for session in removed.into_iter().filter_map(|active| active.session) {
            session.revoke(json!({"kind":"authorization-revoked","message":"Capture owner's MCP authorization ended"}));
        }
        self.recordings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .release_client(client);
        self.replays
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .release_client(client);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revocation_releases_only_the_credentials_receivers_and_uploads() {
        let service = Service::new(false, None);
        for (id, client) in [("mcp-owned", "credential"), ("portal-owned", "portal")] {
            service.sessions.lock().unwrap().insert(
                id.into(),
                Active {
                    generation: id.into(),
                    fingerprint: "contract".into(),
                    owner: Some(client.into()),
                    session: None,
                },
            );
        }
        let upload = |client| {
            service
                .request(
                    "controller_hid_replay_upload_start",
                    &json!({"totalBytes":42,"clientId":client}),
                )
                .unwrap()["uploadId"]
                .clone()
        };
        let removed = upload("credential");
        let retained = upload("portal");
        service.release_client("credential");
        assert!(!service.sessions.lock().unwrap().contains_key("mcp-owned"));
        assert!(service
            .sessions
            .lock()
            .unwrap()
            .contains_key("portal-owned"));
        let cancel = |client, id| {
            service.request(
                "controller_hid_replay_upload_cancel",
                &json!({"uploadId":id,"clientId":client}),
            )
        };
        assert!(cancel("credential", removed).is_err());
        assert!(cancel("portal", retained).is_ok());
    }
}
