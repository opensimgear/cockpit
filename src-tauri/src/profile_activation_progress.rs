//! Volatile operation identities for manual activation. Progress never acquires
//! the profile admission or coordinator locks; cancellation revokes only the
//! exact captured epoch and cannot deactivate a subsequently admitted owner.
use super::*;
use std::time::{Duration, Instant};

const RETAIN: Duration = Duration::from_secs(600);
const MAX_RETAINED: usize = 16;

fn operation_id(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

#[derive(Clone)]
struct CompanionTarget {
    instance: String,
    publication: String,
    name: String,
    previous: Option<Value>,
}

#[derive(Clone)]
struct Activation {
    id: String,
    profile: String,
    epoch: u64,
    phase: &'static str,
    cancel_requested: bool,
    message: String,
    error: Option<String>,
    outcome: Option<&'static str>,
    previous_owner: Option<crate::profile_session::RunOwner>,
    companions: Vec<CompanionTarget>,
    completed: Option<Instant>,
}
impl Activation {
    fn pending(&self) -> bool { self.completed.is_none() }
    fn view(&self, epoch: u64, companions: Vec<Value>) -> Value {
        json!({"activationId":self.id,"profileId":self.profile,"phase":self.phase,
            "cancelRequested":self.cancel_requested,"canCancel":self.pending()&&!self.cancel_requested&&self.epoch==epoch,
            "message":self.message,"error":self.error,"outcome":self.outcome,"companions":companions})
    }
}

#[derive(Default)]
pub(super) struct Tracker { entries: Mutex<BTreeMap<String, Activation>> }
impl Tracker {
    fn begin(&self, id: &str, profile: &str, begin_epoch: impl FnOnce() -> u64) -> Result<u64,String> {
        if !operation_id(id) || !operation_id(profile) { return Err("Activation requires a fresh operation identity and saved profile.".into()); }
        let mut entries=self.entries.lock().unwrap_or_else(|p|p.into_inner());
        entries.retain(|_,entry|entry.completed.is_none_or(|at|at.elapsed()<RETAIN));
        if entries.contains_key(id) { return Err("This activation identity was already used. Activate again with a fresh operation.".into()); }
        while entries.len() >= MAX_RETAINED {
            let oldest=entries.iter().filter_map(|(id,entry)|entry.completed.map(|at|(id.clone(),at))).min_by_key(|(_,at)|*at).map(|(id,_)|id);
            if let Some(oldest)=oldest { entries.remove(&oldest); }
            else { return Err("Wait for pending profile activations to finish before starting another.".into()); }
        }
        let epoch=begin_epoch();
        entries.insert(id.into(),Activation {id:id.into(),profile:profile.into(),epoch,phase:"preparing",cancel_requested:false,
            message:"Checking devices and preparing the profile…".into(),error:None,outcome:None,previous_owner:None,companions:vec![],completed:None});
        Ok(epoch)
    }
    pub(super) fn activating(&self,id:&str) {
        let mut entries=self.entries.lock().unwrap_or_else(|p|p.into_inner());
        if let Some(entry)=entries.get_mut(id).filter(|entry|entry.pending()&&!entry.cancel_requested) {
            entry.phase="activating";entry.message="Applying the profile and preparing companion screens…".into();
        }
    }
    fn cancel(&self,id:&str,epoch:&AtomicU64)->Result<(),String> {
        let mut entries=self.entries.lock().unwrap_or_else(|p|p.into_inner());
        let entry=entries.get_mut(id).ok_or("This activation is no longer available. Check the active profile.")?;
        if !entry.pending() { return Ok(()); }
        if entry.cancel_requested { return Ok(()); }
        if epoch.compare_exchange(entry.epoch,entry.epoch.wrapping_add(1),Ordering::AcqRel,Ordering::Acquire).is_err() {
            return Err("This activation has finished or been replaced. Check the active profile.".into());
        }
        entry.cancel_requested=true;entry.phase="cancelling";
        entry.message="Cancelling activation and safely releasing any changes…".into();
        Ok(())
    }
}

impl GatewayManager {
    pub(super) fn begin_profile_activation(&self,request:&Value)->Result<u64,String> {
        if !request.is_object()||request.to_string().len()>PROFILE_REQUEST_LIMIT
            || !request["clientId"].as_str().is_some_and(|id|crate::protocol::identifier(id,16,80)) {
            return Err("Invalid Cockpit activation request; reload the application.".into());
        }
        let id=request["activationId"].as_str().ok_or("Activation requires a fresh operation identity.")?;
        let profile=request["id"].as_str().ok_or("Choose a saved profile to activate.")?;
        self.profile_activations.begin(id,profile,||self.invalidate_learning())
    }
    pub(super) fn capture_profile_activation(&self,request:&Value,document:&osg_profiles::ProfileDocument,assignments:&[osg_profiles::store::LocalAssignment]) {
        let Some(id)=request["activationId"].as_str() else{return;};
        let previous_owner=self.profile_session.lock().unwrap_or_else(|p|p.into_inner()).status().owner.clone();
        let companions=document.companion_assignments.iter().filter_map(|desired| {
            let assignment=assignments.iter().find(|a|a.resource_id==desired.resource_id)?;
            let (_,instance)=self.profile_resources.companion_authoring_identity(&assignment.source_id).ok()?;
            Some(CompanionTarget {previous:self.companion_host.profile_panel_operation(&instance,&desired.publication_hash).map(|report|report.operation),
                instance,publication:desired.publication_hash.clone(),
                name:document.resources.iter().find(|r|r.id==desired.resource_id).map(|r|r.name.clone()).unwrap_or_else(||"Companion".into())})
        }).collect();
        if let Some(entry)=self.profile_activations.entries.lock().unwrap_or_else(|p|p.into_inner()).get_mut(id) {
            entry.previous_owner=previous_owner;entry.companions=companions;
        }
    }
    pub(super) fn finish_profile_activation(&self,id:&str,response:&Value) {
        // Called only after the activation released its admission/coordinator
        // guards. Capture the actual remaining owner, never infer it from IPC.
        let status=self.profile_session.lock().unwrap_or_else(|p|p.into_inner()).status().clone();
        let mut entries=self.profile_activations.entries.lock().unwrap_or_else(|p|p.into_inner());
        let Some(entry)=entries.get_mut(id) else {return;};
        let succeeded=response["ok"]==true;
        entry.phase=if succeeded {"succeeded"}else if entry.cancel_requested {"cancelled"}else{"failed"};
        entry.outcome=Some(if matches!(status.state,crate::profile_session::State::RecoveryNeeded|crate::profile_session::State::Faulted) {"recovery-needed"}
            else if status.owner.is_some()&&status.owner==entry.previous_owner {"previous-active"}
            else if status.owner.is_some(){"active"}else{"inactive"});
        entry.error=if entry.cancel_requested&&entry.outcome!=Some("recovery-needed") {None}
            else {response["error"].as_str().map(|error|error.chars().take(512).collect())};
        entry.message=if succeeded&&entry.cancel_requested {"The profile became active before cancellation could take effect.".into()}
            else if succeeded {"Profile activated.".into()}
            else if entry.cancel_requested {match entry.outcome {
                Some("previous-active")=>"Activation cancelled. The previous profile is still active.".into(),
                Some("recovery-needed")=>"Activation cancelled. Recover outputs before activating another profile.".into(),
                Some("active")=>"Activation cancelled. Another profile is active.".into(),
                _=>"Activation cancelled. No profile is active.".into(),
            }} else {entry.error.clone().unwrap_or_else(||"Profile activation failed.".into())};
        entry.completed=Some(Instant::now());
    }
    fn profile_activation_view(&self,id:&str)->Result<Value,String> {
        let activation=self.profile_activations.entries.lock().unwrap_or_else(|p|p.into_inner()).get(id).cloned()
            .filter(|entry|entry.completed.is_none_or(|at|at.elapsed()<RETAIN))
            .ok_or("This activation is no longer available. Check the active profile.")?;
        let companions=activation.companions.iter().map(|target| {
            let operation=self.companion_host.profile_panel_operation(&target.instance,&target.publication)
                .filter(|report|report.fresh&&target.previous.as_ref()!=Some(&report.operation)).map(|report|report.operation);
            json!({"instanceId":target.instance,"publicationHash":target.publication,"name":target.name,"operation":operation})
        }).collect();
        Ok(activation.view(self.profile_admission_epoch.load(Ordering::Acquire),companions))
    }
    pub(super) fn profile_activation_request(&self,request:&Value)->Value {
        let result=(|| {
            if self.closed.load(Ordering::Acquire) {return Err("Cockpit has closed.".into());}
            if !request.is_object()||request.to_string().len()>1024
                ||request.as_object().unwrap().keys().any(|key|!matches!(key.as_str(),"command"|"clientId"|"activationId"))
                ||!request["clientId"].as_str().is_some_and(|id|crate::protocol::identifier(id,16,80)) {
                return Err("Invalid Cockpit activation progress request.".into());
            }
            let id=request["activationId"].as_str().filter(|id|operation_id(id)).ok_or("Choose an exact activation operation.")?;
            if request["command"]=="profile_activation_cancel" {self.profile_activations.cancel(id,&self.profile_admission_epoch)?;}
            self.profile_activation_view(id)
        })();
        match result {Ok(view)=>json!({"ok":true,"state":{"profileActivation":view}}),Err(error)=>json!({"ok":false,"error":error})}
    }
}

#[cfg(test)]
#[path = "profile_activation_progress_tests.rs"]
mod tests;
