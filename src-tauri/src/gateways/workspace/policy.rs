use super::*;
use std::time::{Duration, Instant};

pub(super) const MAX_JOBS: usize = 4;
pub(super) const CLIENT: &str = "workspace-native-automation";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Action { Connect, Start, Config(String), Actions(String) }
impl Action {
    pub fn request(&self, id: &str, port: &Value) -> Value {
        let mut request = json!({"gatewayId":id,"clientId":CLIENT});
        match self {
            Self::Connect => { request["command"]=json!("connect");request["port"]=port["path"].clone();request["backend"]=port["backend"].clone();request["background"]=json!(true); },
            Self::Start => {request["command"]=json!("start");request["background"]=json!(true);},
            Self::Config(id) => {request["command"]=json!("config_read");request["moduleId"]=json!(id);},
            Self::Actions(id) => {request["command"]=json!("action_describe");request["moduleId"]=json!(id);},
        }
        request
    }
}

pub(super) struct Entry {
    pub id: String, pub port: Value, pub present: bool, pub paused: bool,
    pub session: String, pub adapter: Option<Arc<dyn Adapter>>,
    pub state: Arc<Value>, pub owners: BTreeMap<String,String>,
    pub error: String, pub version: u64, pub job: Option<(u64,String)>,
    pub job_source:Option<Arc<Value>>,
    failures: u8, retry: Option<Instant>, start_failed: bool,
}
impl Entry {
    pub fn new(id: String, port: Value) -> Self {
        Self { id, port, present:true, paused:false, session:uuid::Uuid::new_v4().to_string(),
            adapter:None,state:Arc::new(Value::Null),owners:BTreeMap::new(),error:String::new(),
            version:1,job:None,job_source:None,failures:0,retry:None,start_failed:false }
    }
    pub fn touch(&mut self) -> Result<(),String> {
        self.version=self.version.checked_add(1).ok_or("Workspace revision exhausted. Reopen Cockpit.")?;Ok(())
    }
    pub fn port(&mut self, port: &Value) -> Result<(),String> {
        if self.port != *port {
            if self.port["path"] != port["path"] {self.session=uuid::Uuid::new_v4().to_string();self.reset();}
            self.port=port.clone();self.touch()?;
        }
        if !self.present {self.present=true;self.reset();self.touch()?;}
        Ok(())
    }
    pub fn reset(&mut self) {self.retry=None;self.failures=0;self.start_failed=false;self.error.clear();}
    pub fn accept(&mut self, adapter: Arc<dyn Adapter>, state: Arc<Value>, owners: BTreeMap<String,String>) -> Result<(),String> {
        if self.adapter.as_ref().is_none_or(|old| !Arc::ptr_eq(old,&adapter)) {
            if self.adapter.is_some(){self.session=uuid::Uuid::new_v4().to_string();}
            self.adapter=Some(adapter);self.reset();self.touch()?;
        }
        if !Arc::ptr_eq(&self.state,&state) || self.owners!=owners {
            self.state=state;self.owners=owners;self.touch()?;
        }
        Ok(())
    }
    pub fn plan(&self, now: Instant) -> Option<Action> {
        if self.paused || self.job.is_some() || self.retry.is_some_and(|at|at>now) {return None;}
        let state=&self.state;
        if state["connected"]!=true || state["port"]!=self.port["path"] {
            return self.present.then_some(Action::Connect);
        }
        if state["reconnecting"]==true || state["layoutPending"]==true {return None;}
        let modules=state["modules"].as_array()?;
        let contracts=state["backend"]=="stm32" || state["protocolVersion"]==3 && state["canProtocol"]==3;
        if !contracts && state["backend"]=="pico" && self.owners.is_empty() && !self.start_failed
            && state["running"]!=true && state["needsReconnect"]!=true
            && modules.iter().any(|m|m["online"]==true&&m["registered"]==true) {return Some(Action::Start);}
        if !contracts {return None;}
        if self.owners.is_empty() {
            if let Some(id)=modules.iter().find(|m|m["online"]==true&&m["canConfigure"]==true&&m["directUsb"]!=true
                && m["id"].as_str().is_some_and(|id|state["moduleConfigs"][id].is_null())).and_then(|m|m["id"].as_str()) {
                return Some(Action::Config(id.into()));
            }
        }
        modules.iter().find(|m|m["online"]==true&&m["boot"].as_u64().is_some_and(|b|b>0)
            && m["id"].as_str().is_some_and(|id|state["moduleActions"][id].is_null()))
            .and_then(|m|m["id"].as_str()).map(|id|Action::Actions(id.into()))
    }
    pub fn complete(&mut self, completion: &Completion) -> Result<(),String> {
        if self.job.as_ref().is_none_or(|(token,_)|*token!=completion.token) {return Ok(());}
        let no_progress=self.job_source.take().is_some_and(|state|Arc::ptr_eq(&state,&self.state));
        self.job=None;self.touch()?;
        if self.session!=completion.session {return Ok(());}
        let error=completion.error.as_deref().or_else(||no_progress.then_some("Gateway completed without a current cache update; waiting before retry."));
        match error {
            None => {self.retry=None;self.failures=0;self.error.clear();},
            Some(error) => {
                self.error=error.chars().take(1024).collect();
                self.failures=self.failures.saturating_add(1);
                self.retry=Some(Instant::now()+Duration::from_secs((1u64<<self.failures.saturating_sub(1).min(5)).min(30)));
                if completion.action==Action::Start {self.start_failed=true;}
            },
        }
        Ok(())
    }
    pub fn deadline(&self) -> Option<Instant> {
        self.retry.filter(|at|self.plan(*at).is_some())
    }
    pub fn wire(&self, ports: &[Value]) -> Value {
        let mut state=if self.state["connected"]==true&&self.state["port"]!=self.port["path"]{Value::Null}else{self.state.as_ref().clone()};
        if let Some(object)=state.as_object_mut() {
            object.remove("nativeSnapshotAtMs");object.insert("gatewayId".into(),json!(self.id));
            object.insert("backend".into(),self.port["backend"].clone());
            object.insert("ports".into(),json!(ports));object.insert("claimedModules".into(),json!([]));
        }
        let owners=self.owners.iter().map(|(id,client)|(id.clone(),json!(format!("sha256:{:x}",Sha256::digest(client.as_bytes()))))).collect::<serde_json::Map<_,_>>();
        json!({"id":self.id,"port":self.port,"state":state,"error":self.error,"busy":self.job.is_some(),
            "present":self.present,"autoConnectPaused":self.paused,"session":self.session,"claimOwners":owners})
    }
}
