use super::*;
fn issue(name:String,scope:String)->Result<(Credential,String),String>{
    let name=name.trim();
    if name.is_empty() || name.len()>80 || name.chars().any(char::is_control) || !matches!(scope.as_str(),"read"|"write") {
        return Err("Use a name of 1–80 bytes and read or write access.".into());
    }
    let id=uuid::Uuid::new_v4().to_string();
    let token=format!("osg_mcp_{}.{}{}",id,uuid::Uuid::new_v4().simple(),uuid::Uuid::new_v4().simple());
    let credential=Credential{id,name:name.into(),scope,created_at:SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64,hash:crate::mcp_credentials::digest(&token)};
    Ok((credential,token))
}
impl Server {
    pub fn create_credential(&self,name:String,scope:String,expected_revision:u64)->Result<Value,String>{
        let _control=self.controls.lock().unwrap_or_else(|p|p.into_inner());
        let mut state=self.state.lock().unwrap_or_else(|p|p.into_inner());
        if state.closed{return Err("Cockpit is shutting down.".into());}
        self.refresh_credentials(&mut state)?;state.store.check_revision(expected_revision)?;
        if state.store.settings.credentials.len()>=32{return Err("Remove an unused credential before creating another (maximum 32).".into());}
        let(credential,token)=issue(name,scope)?;let metadata=credential.metadata();
        let mut next=state.store.settings.clone();next.credentials.push(credential);state.store.save(next)?;
        let enabled=state.store.settings.enabled && state.listener.is_some();drop(state);
        self.native_authorization(metadata["id"].as_str().unwrap(),enabled);
        Ok(json!({"credential":metadata,"token":token,"status":self.status()}))
    }
    pub fn revoke_credential(&self,id:&str,expected_revision:u64)->Result<Value,String>{
        let _control=self.controls.lock().unwrap_or_else(|p|p.into_inner());
        let mut state=self.state.lock().unwrap_or_else(|p|p.into_inner());
        if state.closed{return Err("Cockpit is shutting down.".into());}
        self.refresh_credentials(&mut state)?;state.store.check_revision(expected_revision)?;
        let mut next=state.store.settings.clone();next.credentials.retain(|credential|credential.id!=id);
        if next.credentials.len()==state.store.settings.credentials.len(){return Err("That MCP credential no longer exists. Refresh the list.".into());}
        state.store.save(next)?;drop(state);self.native_authorization(id,false);Ok(self.status())
    }
    pub fn rotate_credential(&self,id:&str,expected_revision:u64)->Result<Value,String>{
        let _control=self.controls.lock().unwrap_or_else(|p|p.into_inner());
        let mut state=self.state.lock().unwrap_or_else(|p|p.into_inner());
        if state.closed{return Err("Cockpit is shutting down.".into());}
        self.refresh_credentials(&mut state)?;state.store.check_revision(expected_revision)?;
        let index=state.store.settings.credentials.iter().position(|credential|credential.id==id).ok_or("That MCP credential no longer exists. Refresh the list.")?;
        let original=&state.store.settings.credentials[index];let(replacement,token)=issue(original.name.clone(),original.scope.clone())?;let metadata=replacement.metadata();
        let mut next=state.store.settings.clone();next.credentials[index]=replacement;
        // New hash and old-credential removal become durable together.
        state.store.save(next)?;let enabled=state.store.settings.enabled && state.listener.is_some();drop(state);
        self.native_authorization(id,false);self.native_authorization(metadata["id"].as_str().unwrap(),enabled);
        Ok(json!({"credential":metadata,"token":token,"status":self.status()}))
    }
}
