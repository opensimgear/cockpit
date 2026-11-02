use super::*;
impl GatewayManager {
    pub(super) fn process_monitor_cancel_request(
        &self,
        request: &Value,
        authorization: Option<&McpAuthorization>,
    ) -> Value {
        if self.closed.load(Ordering::Acquire)
            || request.to_string().len() > 65_536
            || !request["clientId"]
                .as_str()
                .is_some_and(|id| crate::protocol::identifier(id, 16, 80))
        {
            return json!({"ok":false,"error":"Invalid process monitoring client or closed Cockpit."});
        }
        if let Err(error) = self.check_mcp_authorization(authorization) {
            return json!({"ok":false,"error":error});
        }
        // Cancel never waits for profile takeover, game metadata admission or
        // the controller coordinator. The permission service revokes first.
        match self.games.request(request) {
            Ok(()) => json!({"ok":true,"state":{"games":self.games.status()}}),
            Err(error) => json!({"ok":false,"error":error.chars().take(512).collect::<String>()}),
        }
    }
}
