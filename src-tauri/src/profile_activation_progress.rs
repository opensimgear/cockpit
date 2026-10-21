//! Volatile operation identities for native profile controls. Progress never acquires
//! the profile admission or coordinator locks; cancellation revokes only the
//! exact captured epoch and cannot deactivate a subsequently admitted owner.
use super::*;
use std::time::{Duration, Instant};

const RETAIN: Duration = Duration::from_secs(600);
const MAX_RETAINED: usize = 16;
// Automatic switches can finish between the application's 150 ms polls. Keep
// the newest switch result visible briefly so the native notification is seen.
const VEHICLE_SWITCH_CONFIRMATION: Duration = Duration::from_millis(1200);

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
    started: Instant,
    step: String,
    origin: String,
    profile_name: String,
    vehicle_name: Option<String>,
    steps: Vec<Value>,
    cancellable: bool,
}
impl Activation {
    fn pending(&self) -> bool { self.completed.is_none() }
    fn view(&self, epoch: u64, companions: Vec<Value>) -> Value {
        json!({"activationId":self.id,"profileId":self.profile,"phase":self.phase,
            "cancelRequested":self.cancel_requested,"canCancel":self.cancellable&&self.pending()&&!self.cancel_requested&&self.epoch==epoch,
            "message":self.message,"step":self.step,"steps":self.steps,"origin":self.origin,"profileName":self.profile_name,"vehicleName":self.vehicle_name,
            "error":self.error,"outcome":self.outcome,"companions":companions})
    }
}

#[derive(Default)]
pub(super) struct Tracker { entries: Mutex<BTreeMap<String, Activation>> }
impl Tracker {
    fn begin(&self, id: &str, profile: &str, begin_epoch: impl FnOnce() -> u64) -> Result<u64,String> {
        self.begin_with(id,profile,begin_epoch,|_|{})
    }
    fn begin_with(&self,id:&str,profile:&str,begin_epoch:impl FnOnce()->u64,configure:impl FnOnce(&mut Activation))->Result<u64,String> {
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
        let mut activation=Activation {id:id.into(),profile:profile.into(),epoch,phase:"preparing",cancel_requested:false,
            message:"Checking devices and preparing the profile…".into(),error:None,outcome:None,previous_owner:None,companions:vec![],completed:None,
            started:Instant::now(),step:"preparing".into(),origin:"manual".into(),profile_name:profile.into(),vehicle_name:None,steps:vec![],cancellable:true};
        configure(&mut activation);entries.insert(id.into(),activation);
        Ok(epoch)
    }
    fn current(&self) -> Option<String> {
        // Admission epoch also fences a stale switch tracker inserted after a
        // newer manual control. Consider the newest operation before visibility
        // so an older switch cannot reappear after newer work finishes.
        self.entries.lock().unwrap_or_else(|p|p.into_inner()).values()
            .max_by_key(|entry|(entry.epoch,entry.started))
            .filter(|entry|entry.pending() || entry.origin=="vehicle-switch"
                && entry.completed.is_some_and(|at|at.elapsed()<VEHICLE_SWITCH_CONFIRMATION))
            .map(|entry|entry.id.clone())
    }
    pub(super) fn activating(&self,id:&str) {
        let mut entries=self.entries.lock().unwrap_or_else(|p|p.into_inner());
        if let Some(entry)=entries.get_mut(id).filter(|entry|entry.pending()&&!entry.cancel_requested) {
            entry.phase="activating";entry.message="Applying the profile and preparing companion screens…".into();
        }
    }
    pub(super) fn note(&self, epoch:u64, step:&str, message:&str) {
        let mut entries=self.entries.lock().unwrap_or_else(|p|p.into_inner());
        if let Some(entry)=entries.values_mut().find(|entry|entry.pending()&&entry.epoch==epoch&&!entry.cancel_requested) {
            entry.phase=if entry.origin=="deactivation" {"deactivating"}else if entry.origin=="recovery" {"recovering"}
                else if step=="launching" {"launching"}else if matches!(step,"preparing"|"validating"|"compiling") {"preparing"}else{"activating"};
            entry.step=step.into();entry.message=message.chars().take(1024).collect();
            if entry.steps.last().is_none_or(|previous|previous["step"]!=step||previous["message"]!=entry.message) {
                if entry.steps.len()>=128 {entry.steps.remove(0);}
                entry.steps.push(json!({"step":entry.step,"message":entry.message}));
            }
        }
    }
    pub(super) fn rebind(&self,id:&str,old:u64,new:u64)->Result<(),String> {
        let mut entries=self.entries.lock().unwrap_or_else(|p|p.into_inner());
        let entry=entries.get_mut(id).filter(|entry|entry.pending()&&entry.epoch==old&&!entry.cancel_requested)
            .ok_or("The activation was cancelled before game launch admission.")?;
        entry.epoch=new;Ok(())
    }
    fn cancel(&self,id:&str,epoch:&AtomicU64)->Result<(),String> {
        let mut entries=self.entries.lock().unwrap_or_else(|p|p.into_inner());
        let entry=entries.get_mut(id).ok_or("This activation is no longer available. Check the active profile.")?;
        if !entry.pending() { return Ok(()); }
        if !entry.cancellable {return Err(match entry.origin.as_str() {
            "vehicle-switch"=>"Vehicle switching cannot be cancelled here. Deactivate the profile to stop it.",
            "peripheral-reconnect"=>"Peripheral reconnection cannot be cancelled here. Deactivate the profile to stop it.",
            _=>"Output cleanup must finish before another profile operation.",
        }.into());}
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
    pub(super) fn begin_profile_cleanup(&self,request:&mut Value,epoch:u64)->Option<String> {
        let id=request["activationId"].as_str().filter(|id|operation_id(id)).map(str::to_owned).unwrap_or_else(||uuid::Uuid::new_v4().to_string());
        // Progress is best effort. Emergency neutralization must still proceed
        // if all volatile tracker slots are occupied or an ID is reused.
        if self.profile_activations.begin(&id,osg_profiles::DEFAULT_PROFILE_ID,||epoch).is_err(){return None;}
        let recovering=request["command"]=="profile_recover";
        if let Some(entry)=self.profile_activations.entries.lock().unwrap_or_else(|p|p.into_inner()).get_mut(&id) {
            entry.cancellable=false;entry.origin=if recovering{"recovery"}else{"deactivation"}.into();
            entry.phase=if recovering{"recovering"}else{"deactivating"};
            entry.step="cleanup-starting".into();entry.message=if recovering{"Recovering outputs…"}else{"Deactivating profile…"}.into();
            entry.profile_name="Profile".into();
        }
        request["activationId"]=json!(id);Some(id)
    }
    pub(super) fn identify_profile_cleanup(&self,id:&str,profile:Option<&str>) {
        if let Some(profile)=profile {if let Some(entry)=self.profile_activations.entries.lock().unwrap_or_else(|p|p.into_inner()).get_mut(id)
            .filter(|entry|entry.pending()&&!entry.cancellable){entry.profile=profile.into();}}
    }
    pub(super) fn start_native_profile_activation(&self,profile:&str,epoch:u64,origin:&str,_game_id:Option<&str>)->Result<String,String> {
        let id=uuid::Uuid::new_v4().to_string();
        self.profile_activations.begin(&id,profile,||epoch)?;
        let name=self.profiles.lock().unwrap_or_else(|p|p.into_inner()).compile_source(profile).map(|source|source.0.name).unwrap_or_else(|_|profile.into());
        if let Some(entry)=self.profile_activations.entries.lock().unwrap_or_else(|p|p.into_inner()).get_mut(&id) {
            entry.origin=origin.into();entry.profile_name=name;
        }
        Ok(id)
    }
    pub(super) fn start_vehicle_profile_switch(&self,candidate:&crate::profile_session::Candidate,selection:&str)->Result<String,String> {
        let id=uuid::Uuid::new_v4().to_string();
        let epoch=candidate.admission_epoch.ok_or("Vehicle switching requires native admission.")?;
        let vehicle=candidate.document.vehicle_tabs.iter().find(|tab|tab.id==selection)
            .map(|tab|tab.vehicle_name.as_str()).unwrap_or("Base");
        self.profile_activations.begin_with(&id,&candidate.document.id,||epoch,|entry| {
            entry.origin="vehicle-switch".into();entry.cancellable=false;
            entry.profile_name=candidate.document.name.clone();entry.vehicle_name=Some(vehicle.into());
            entry.step="preparing-vehicle".into();entry.message=format!("Preparing {vehicle} settings…");
        })?;
        self.capture_profile_activation_id(&id,&candidate.document,&candidate.assignments);
        Ok(id)
    }
    pub(super) fn start_peripheral_profile_reconnect(&self,candidate:&crate::profile_session::Candidate)->Result<String,String> {
        let id=uuid::Uuid::new_v4().to_string();
        let epoch=candidate.admission_epoch.ok_or("Peripheral reconnection requires native admission.")?;
        self.profile_activations.begin_with(&id,&candidate.document.id,||epoch,|entry| {
            entry.origin="peripheral-reconnect".into();entry.cancellable=false;
            entry.profile_name=candidate.document.name.clone();
            entry.step="preparing-peripherals".into();entry.message="Reconnecting available profile devices…".into();
        })?;
        self.capture_profile_activation_id(&id,&candidate.document,&candidate.assignments);
        Ok(id)
    }
    pub(super) fn note_profile_activation(&self,epoch:u64,step:&str,message:&str) {self.profile_activations.note(epoch,step,message);}
    pub(super) fn rebind_profile_activation_epoch(&self,id:&str,old:u64,new:u64)->Result<(),String> {self.profile_activations.rebind(id,old,new)}
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
        self.capture_profile_activation_id(id,document,assignments);
    }
    pub(super) fn capture_profile_activation_id(&self,id:&str,document:&osg_profiles::ProfileDocument,assignments:&[osg_profiles::store::LocalAssignment]) {
        let previous_owner=self.profile_session.lock().unwrap_or_else(|p|p.into_inner()).status().owner.clone();
        let companions=document.companion_assignments.iter().filter_map(|desired| {
            let assignment=assignments.iter().find(|a|a.resource_id==desired.resource_id)?;
            let (_,instance)=self.profile_resources.companion_authoring_identity(&assignment.source_id).ok()?;
            Some(CompanionTarget {previous:self.companion_host.profile_panel_operation(&instance,&desired.publication_hash).map(|report|report.operation),
                instance,publication:desired.publication_hash.clone(),
                name:document.resources.iter().find(|r|r.id==desired.resource_id).map(|r|r.name.clone()).unwrap_or_else(||"Companion".into())})
        }).collect();
        if let Some(entry)=self.profile_activations.entries.lock().unwrap_or_else(|p|p.into_inner()).get_mut(id) {
            entry.previous_owner=previous_owner;entry.companions=companions;entry.profile_name=document.name.clone();
        }
    }
    pub(super) fn finish_profile_activation(&self,id:&str,response:&Value) {
        // The operation's coordinator guard has been released. Cleanup keeps
        // admission until this snapshot, so a later activation cannot delay its
        // completion. Capture the actual owner, never infer it from IPC.
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
        entry.message=if succeeded&&entry.origin=="deactivation" {"Profile deactivated.".into()}
            else if succeeded&&entry.origin=="recovery" {"Outputs recovered.".into()}
            else if succeeded&&entry.origin=="vehicle-switch" {format!("{} settings applied.",entry.vehicle_name.as_deref().unwrap_or("Vehicle"))}
            else if succeeded&&entry.origin=="peripheral-reconnect" {"Available profile devices reconnected.".into()}
            else if succeeded&&entry.cancel_requested {"The profile became active before cancellation could take effect.".into()}
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
            if request["command"]=="profile_activation_current" {
                let current=self.profile_activations.current();
                return current.map(|id|self.profile_activation_view(&id)).unwrap_or(Ok(Value::Null));
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
