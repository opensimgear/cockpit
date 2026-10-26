//! Fixed UI owners share SQLite with native commands, MCP and desktop windows.
use osg_persistence::{Change,Database};
use serde::Serialize;
use serde_json::{json,Value};
use std::{collections::BTreeMap,path::Path,sync::{Arc,Mutex}};
mod validation;
pub(crate) mod commands;
pub(crate) mod window_writer;
pub(crate) use validation::{validate,WindowGeometry};
pub(crate) const FRONTEND_KEYS:[&str;7]=["theme","devices-mode","canvas-navigation","canvas-styling","remembered","tray","progress"];
pub(crate) const KEYS:[&str;8]=["theme","devices-mode","canvas-navigation","canvas-styling","remembered","tray","progress","windows"];
type Observer=Arc<dyn Fn(&str,u64)+Send+Sync>;
#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct Entry {pub revision:u64,pub value:Value}
#[derive(Default)]
struct State {entries:BTreeMap<String,Entry>,errors:BTreeMap<String,String>,observer:Option<Observer>,tray_available:bool}
#[derive(Clone)]
pub(crate) struct Preferences {database:Result<Database,String>,state:Arc<Mutex<State>>}
fn location(key:&str)->Result<(&str,&str),String>{
    if !KEYS.contains(&key){return Err("Unknown UI preference owner.".into());}
    Ok(if key=="remembered"{("inventory","remembered")}else{("ui",key)})
}
fn limit(key:&str)->usize{if key=="remembered"{1024*1024}else{8192}}
fn defaults(key:&str)->Value{match key {
    "theme"=>json!(false),"devices-mode"=>json!("list"),"canvas-navigation"=>json!({"wheelAction":"pan"}),
    "canvas-styling"=>json!({"arrowStyle":"smooth","cornerRadius":24}),
    "tray"=>json!({"closeToTray":false,"minimizeToTray":false}),"progress"=>json!({"delivery":"system","position":"bottom-right"}),_=>Value::Null,
}}
impl Preferences {
    pub(crate) fn new(anchor:&Path)->Self{Self::from_database(Database::for_namespace(anchor))}
    pub(crate) fn from_database(database:Result<Database,String>)->Self{Self{database,state:Arc::new(Mutex::new(State::default()))}}
    pub(crate) fn observe(&self,observer:Observer){self.state.lock().unwrap_or_else(|p|p.into_inner()).observer=Some(observer);}
    pub(crate) fn set_tray_available(&self,available:bool){self.state.lock().unwrap_or_else(|p|p.into_inner()).tray_available=available;}
    fn publish(&self,key:&str,entry:Entry){
        let observer={let mut state=self.state.lock().unwrap_or_else(|p|p.into_inner());
            if state.entries.get(key).is_some_and(|old|old.revision>entry.revision){return;}
            let changed=state.entries.get(key).is_none_or(|old|old.revision!=entry.revision);
            state.errors.remove(key);state.entries.insert(key.into(),entry.clone());if changed{state.observer.clone()}else{None}};
        if let Some(observer)=observer{observer(key,entry.revision);}
    }
    fn decoded(&self,key:&str,revision:u64,bytes:Option<&[u8]>)->Result<Entry,String>{
        let parsed:Result<Entry,String>=(||{let mut value=if let Some(bytes)=bytes{
            if bytes.len()>limit(key){return Err("Saved UI preferences exceed their byte budget.".into());}
            let value:Value=serde_json::from_slice(bytes).map_err(|_|"Saved UI preferences are invalid. Preserve their SQLite record.")?;validate(key,&value)?;value
        }else{defaults(key)};validation::stale_observations(key,&mut value);Ok(Entry{revision,value})})();
        match parsed{Ok(entry)=>{self.publish(key,entry.clone());Ok(entry)},Err(error)=>{self.state.lock().unwrap_or_else(|p|p.into_inner()).errors.insert(key.into(),error.clone());Err(error)}}
    }
    pub(crate) fn read(&self,key:&str)->Result<Entry,String>{
        let(domain,key)=location(key)?;let database=self.database.as_ref().map_err(Clone::clone)?;
        let(revision,record)=database.read_with_revision(domain,key)?;self.decoded(key,revision,record.as_ref().map(|record|record.payload.as_slice()))
    }
    pub(crate) fn snapshot(&self,keys:&[&str])->Result<Value,String>{
        let locations=keys.iter().map(|key|location(key)).collect::<Result<Vec<_>,_>>()?;
        let rows=self.database.as_ref().map_err(Clone::clone)?.read_many(&locations)?;let mut entries=serde_json::Map::new();
        for (key,(revision,record)) in keys.iter().zip(rows){entries.insert((*key).into(),serde_json::to_value(self.decoded(key,revision,record.as_ref().map(|r|r.payload.as_slice()))?).map_err(|_|"Cannot encode UI settings.")?);}
        Ok(json!({"entries":entries}))
    }
    pub(crate) fn cached(&self,key:&str)->Value{
        let state=self.state.lock().unwrap_or_else(|p|p.into_inner());if state.errors.contains_key(key){return defaults(key);}
        state.entries.get(key).map(|entry|entry.value.clone()).unwrap_or_else(||defaults(key))
    }
    pub(crate) fn cached_entry(&self,key:&str)->Result<Entry,String>{
        location(key)?;let state=self.state.lock().unwrap_or_else(|p|p.into_inner());
        if let Some(error)=state.errors.get(key){return Err(error.clone());}
        Ok(state.entries.get(key).cloned().unwrap_or_else(||Entry{revision:0,value:defaults(key)}))
    }
    pub(crate) fn write(&self,key:&str,expected:u64,mut value:Value)->Result<Entry,String>{
        let(domain,row)=location(key)?;if expected>9_007_199_254_740_990{return Err("Invalid UI settings revision.".into());}
        validate(key,&value)?;
        validation::stale_observations(key,&mut value);
        if key=="tray"&&(value["closeToTray"]==true||value["minimizeToTray"]==true)&&!self.state.lock().unwrap_or_else(|p|p.into_inner()).tray_available{return Err("The system tray is unavailable on this desktop.".into());}
        let bytes=serde_json::to_vec(&value).map_err(|_|"Cannot encode UI preferences.")?;
        if bytes.len()>limit(key){return Err("UI preferences exceed their byte budget.".into());}
        let database=self.database.as_ref().map_err(Clone::clone)?;
        database.commit(&[Change{domain,key:row,expected_revision:expected,payload:Some(&bytes)}])?;
        let entry=Entry{revision:expected+1,value};self.publish(key,entry.clone());Ok(entry)
    }
    pub(crate) fn request(&self,request:&Value)->Result<Value,String>{
        if request.to_string().len()>2*1024*1024+8192{return Err("UI settings request exceeds its byte budget.".into());}
        let command=request["command"].as_str().ok_or("Missing UI settings command.")?;
        let allowed=if command=="ui_preferences_get"{vec!["command","clientId","key"]}else{vec!["command","clientId","key","expectedRevision","value"]};
        if request.as_object().is_none_or(|object|object.keys().any(|key|!allowed.contains(&key.as_str()))){return Err("Unexpected UI settings fields.".into());}
        match command {
            "ui_preferences_get"=>match request.get("key"){Some(key)=>serde_json::to_value(self.read(key.as_str().ok_or("Invalid UI preference owner.")?)?).map_err(|_|"Cannot encode UI settings.".into()),None=>self.snapshot(&FRONTEND_KEYS)},
            "ui_preferences_save"=>{let key=request["key"].as_str().ok_or("Missing UI preference owner.")?;let expected=request["expectedRevision"].as_u64().ok_or("Missing UI settings revision.")?;let value=request.get("value").ok_or("Missing UI settings value.")?.clone();serde_json::to_value(self.write(key,expected,value)?).map_err(|_|"Cannot encode UI settings.".into())},
            _=>Err("Unknown UI settings operation.".into()),
        }
    }
}
#[cfg(test)]
mod tests;
