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
        let _activity_guard=self.manager.activity_mutation();
        let before=Value::Null;
        let result=self.create_credential_inner(name,scope,expected_revision);
        let after=result.as_ref().ok().and_then(|value|safe_credential_metadata(&value["credential"])).unwrap_or(Value::Null);
        let id=after["id"].as_str();let name=after["name"].as_str();
        self.manager.record_external_change("mcp_credential_create",id,name,&before,&after,result.as_ref().err().map(String::as_str),"mcp");
        result
    }
    fn create_credential_inner(&self,name:String,scope:String,expected_revision:u64)->Result<Value,String>{
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
        let _activity_guard=self.manager.activity_mutation();
        let before=self.credential_metadata(id).unwrap_or(Value::Null);
        let credential_id=before["id"].as_str();
        let name=before["name"].as_str();
        let result=self.revoke_credential_inner(id,expected_revision);
        let after=if result.is_ok(){Value::Null}else{before.clone()};
        self.manager.record_external_change("mcp_credential_revoke",credential_id,name,&before,&after,result.as_ref().err().map(String::as_str),"mcp");
        result
    }
    fn revoke_credential_inner(&self,id:&str,expected_revision:u64)->Result<Value,String>{
        let _control=self.controls.lock().unwrap_or_else(|p|p.into_inner());
        let mut state=self.state.lock().unwrap_or_else(|p|p.into_inner());
        if state.closed{return Err("Cockpit is shutting down.".into());}
        self.refresh_credentials(&mut state)?;state.store.check_revision(expected_revision)?;
        let mut next=state.store.settings.clone();next.credentials.retain(|credential|credential.id!=id);
        if next.credentials.len()==state.store.settings.credentials.len(){return Err("That MCP credential no longer exists. Refresh the list.".into());}
        state.store.save(next)?;drop(state);self.native_authorization(id,false);Ok(self.status())
    }
    pub fn rotate_credential(&self,id:&str,expected_revision:u64)->Result<Value,String>{
        let _activity_guard=self.manager.activity_mutation();
        let before=self.credential_metadata(id).unwrap_or(Value::Null);
        let credential_id=before["id"].as_str();
        let name=before["name"].as_str();
        let result=self.rotate_credential_inner(id,expected_revision);
        let after=result.as_ref().ok().and_then(|value|safe_credential_metadata(&value["credential"])).unwrap_or_else(||before.clone());
        self.manager.record_external_change("mcp_credential_rotate",credential_id,name,&before,&after,result.as_ref().err().map(String::as_str),"mcp");
        result
    }
    fn credential_metadata(&self,id:&str)->Option<Value>{
        let mut state=self.state.lock().unwrap_or_else(|p|p.into_inner());
        self.refresh_credentials(&mut state).ok()?;
        state.store.settings.credentials.iter().find(|credential|credential.id==id).map(Credential::metadata).and_then(|value|safe_credential_metadata(&value))
    }
    fn rotate_credential_inner(&self,id:&str,expected_revision:u64)->Result<Value,String>{
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

fn safe_credential_metadata(value:&Value)->Option<Value>{
    Some(json!({"id":value.get("id")?.as_str()? ,"name":value.get("name")?.as_str()? ,"scope":value.get("scope")?.as_str()?}))
}
