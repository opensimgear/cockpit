use super::*;

impl Server {
    pub(super) fn refresh_credentials(&self,state:&mut State)->Result<(),String>{
        let previous=state.store.settings.clone();let was_error=state.store.error.is_some();
        let active_before=!was_error && !state.closed && previous.enabled && state.listener.as_ref().is_some_and(|listener|listener.port==previous.port);
        if let Err(error)=state.store.refresh(){
            if !was_error {for credential in &previous.credentials {self.native_authorization(&credential.id,false);}}
            state.error=Some(error.clone());return Err(error);
        }
        let settings=&state.store.settings;
        let active=!state.closed && settings.enabled && state.listener.as_ref().is_some_and(|listener|listener.port==settings.port);
        for credential in &previous.credentials {
            let unchanged=settings.credentials.iter().any(|next|next.id==credential.id && next.hash==credential.hash && next.scope==credential.scope);
            if active_before && (!active || !unchanged) {self.native_authorization(&credential.id,false);}
        }
        for credential in &settings.credentials {
            let unchanged=previous.credentials.iter().any(|old|old.id==credential.id && old.hash==credential.hash && old.scope==credential.scope);
            if active && (!active_before || !unchanged) {self.native_authorization(&credential.id,true);}
        }
        if !settings.enabled || state.listener.as_ref().is_some_and(|listener|listener.port!=settings.port) {
            if let Some(listener)=state.listener.take(){listener.cancellation.cancel();state.generation=state.generation.wrapping_add(1);}
        }
        if was_error {state.error=None;}
        Ok(())
    }
    pub(super) fn native_authorization(&self, id: &str, authorized: bool) {
        self.manager
            .mcp_authorize_client(&mcp_tools::native_client(&format!("mcp-{id}")), authorized);
    }
    pub(super) fn authenticate(&self, token: &str, generation: u64) -> Option<Principal> {
        if token.len() > 256 {
            return None;
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if self.refresh_credentials(&mut state).is_err(){return None;}
        if state.closed
            || !state.store.settings.enabled
            || !state
                .listener
                .as_ref()
                .is_some_and(|l| l.generation == generation)
        {
            return None;
        }
        state
            .store
            .settings
            .credentials
            .iter()
            .find(|c| crate::mcp_credentials::matches(token, &c.hash))
            .map(|c| Principal {
                id: c.id.clone(),
                scope: c.scope.clone(),
                generation,
                credential_hash:c.hash.clone(),
            })
    }
    pub(super) fn authorized(&self, principal: &Principal, mutate: bool) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if self.refresh_credentials(&mut state).is_err(){return false;}
        !state.closed
            && state.store.settings.enabled
            && state
                .listener
                .as_ref()
                .is_some_and(|l| l.generation == principal.generation)
            && state.store.settings.credentials.iter().any(|c| {
                c.id == principal.id
                    && c.scope == principal.scope
                    && c.hash == principal.credential_hash
                    && (!mutate || c.scope == "write")
            })
    }
}
