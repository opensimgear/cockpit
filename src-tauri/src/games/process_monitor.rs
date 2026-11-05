use super::*;
impl GamesRuntime {
    pub(super) fn process_monitor_request(&self, request: &Value) -> Result<(), String> {
        if !known(request, &["command", "clientId", "gameId"]) {
            return Err("Unknown process monitoring field.".into());
        }
        let id = game_id(&request["gameId"])?;
        let cancel = request["command"] == "game_process_monitor_cancel";
        if !cancel {
            self.inner.guard_installation_metadata()?;
        }
        let game = self
            .inner
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .catalogue
            .iter()
            .find(|game| game["id"] == id)
            .cloned()
            .ok_or("Choose a registered game.")?;
        let integration = crate::game_discovery::integration_binding(&game)
            .filter(|integration| crate::xplane::permissions::supports(integration))
            .ok_or("Choose a supported game for process notification permission.")?;
        if cancel {
            crate::xplane::permissions::cancel(integration);
            return Ok(());
        }
        let expected = match self.try_observed_desktop_process(id)? {
            crate::game_activation::CurrentProcessObservation::Observed(Some(process)) => process,
            _ => return Err("Start the selected game and wait for its process to be observed before allowing monitoring.".into()),
        };
        // The helper validates this exact handle/image/creation epoch again
        // before Windows can show consent. No caller-supplied PID or path.
        crate::xplane::permissions::enable(integration, expected)
    }
}

#[cfg(test)]
#[path = "process_monitor_tests.rs"]
mod tests;
