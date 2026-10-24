//! Public host setup metadata; protected identity and credentials have separate owners.
use osg_companion_service::server::ServerConfig;
use osg_persistence::{Change,Database,Repository};
use serde_json::{json,Value};
use std::{path::Path,sync::Mutex};
pub(crate) struct Listener {database:Database,state:Mutex<(u64,Option<ServerConfig>)>}
fn database(directory:&Path)->Result<Database,String>{Database::for_namespace(&directory.join("companion-settings"))}
fn valid_listener(config:&ServerConfig)->Result<(),String>{
    let host=config.advertised_host.trim();
    if config.bind.port()<1024 || host.is_empty() || host.len()>253 || host.contains(['/', '@', '?', '#', ' ']) || host.chars().any(char::is_control) {
        return Err("Invalid companion listener settings.".into());
    } Ok(())
}
fn read_listener(database:&Database)->Result<(u64,Option<ServerConfig>),String>{
        let(revision,record)=database.read_with_revision("companion","listener")?;
        let config=record.map(|r|{if r.payload.len()>4096{return Err("Companion listener settings exceed budget.");}serde_json::from_slice::<Option<ServerConfig>>(&r.payload).map_err(|_|"Invalid saved companion listener settings.")}).transpose()?.flatten();
        if let Some(config)=&config{valid_listener(config)?;}
        Ok((revision,config))
}
impl Listener {
    pub(crate) fn open(directory:&Path)->Result<Self,String>{
        let database=database(directory)?;
        let (revision,config)=read_listener(&database)?;
        Ok(Self{database,state:Mutex::new((revision,config))})
    }
    pub(crate) fn refresh(&self)->Result<(u64,Option<ServerConfig>),String>{
        let mut state=self.state.lock().unwrap_or_else(|p|p.into_inner());
        *state=read_listener(&self.database)?;Ok(state.clone())
    }
    pub(crate) fn snapshot(&self)->(u64,Option<ServerConfig>){self.state.lock().unwrap_or_else(|p|p.into_inner()).clone()}
    pub(crate) fn with_config<T>(&self,read:impl FnOnce(Option<ServerConfig>)->T)->T {
        let state=self.state.lock().unwrap_or_else(|p|p.into_inner());read(state.1.clone())
    }
    pub(crate) fn save(&self,config:Option<ServerConfig>,expected:Option<u64>)->Result<(),String>{
        self.save_with_effect(config,expected,||{})
    }
    pub(crate) fn save_with_effect(&self,config:Option<ServerConfig>,expected:Option<u64>,effect:impl FnOnce())->Result<(),String>{
        if let Some(config)=&config{valid_listener(config)?;}
        let bytes=serde_json::to_vec(&config).map_err(|_|"Cannot encode listener settings.")?;
        let mut state=self.state.lock().unwrap_or_else(|p|p.into_inner());
        if expected.is_some_and(|expected|expected!=state.0){return Err("Listener revision conflict. Reload before saving.".into());}
        let revision=self.database.commit(&[Change{domain:"companion",key:"listener",expected_revision:state.0,payload:Some(&bytes)}])?[0];
        *state=(revision,config);effect();Ok(())
    }
}
fn validate_firmware(value:&Value)->Result<(),String>{
    let rows=value.as_object().ok_or("Invalid retained companion firmware metadata.")?;
    if rows.len()>64{return Err("Too many retained companion firmware records.".into());}
    for (id,info) in rows {
        if id.is_empty() || id.len()>64 || !id.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_'){return Err("Invalid firmware companion identity.".into());}
        let info=info.as_object().ok_or("Invalid firmware descriptor.")?;
        if info.keys().any(|k|!["version","profile","connectionMode","renderProfile"].contains(&k.as_str())){return Err("Unexpected firmware descriptor field.".into());}
        for key in ["version","profile"] {if let Some(v)=info.get(key).filter(|v|!v.is_null()) {
            if !v.as_str().is_some_and(|s|!s.is_empty() && s.len()<=128 && !s.chars().any(char::is_control)){return Err("Invalid retained firmware identity.".into());}
        }}
        if info.get("connectionMode").is_some_and(|v|!v.is_null() && v!="wifi" && v!="serial"){return Err("Invalid retained firmware connection mode.".into());}
        if let Some(profile)=info.get("renderProfile") {osg_companion_service::render_profile::validate(profile)?;}
    }
    Ok(())
}
/// Each capture owns its exact revision, so another status read cannot advance
/// an earlier observation's expected revision and let it overwrite newer data.
pub(crate) fn firmware(directory:&Path)->Result<(Repository<Value>,Value),String>{
    let repository=Repository::new(database(directory)?,"companion","firmware",65536,validate_firmware)?;
    let value=repository.load()?.unwrap_or(json!({}));Ok((repository,value))
}
#[cfg(test)]
#[path="host_settings_tests.rs"]
mod tests;
