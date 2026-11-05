//! Cheap typed source invalidations and ordered job transitions. No callbacks,
//! snapshot construction, serialization, filesystem or transport runs here.
use serde::Serialize;
use std::{collections::{BTreeMap,VecDeque},sync::{atomic::{AtomicBool, Ordering},Arc,Mutex,Weak}};

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum Domain { Library, Discovery, Runtime, Artwork, Setup, Permissions }
impl Domain { pub(super) fn bit(self)->u32 { 1 << self as u32 } }
pub(super) const ALL:u32=(1<<6)-1;
const MAX_JOBS:usize=16;
const MAX_EVENTS:usize=256;
const MAX_EVENT_BYTES:usize=512*1024;

#[derive(Clone,Debug,Serialize,PartialEq,Eq)]
#[serde(rename_all="camelCase")]
pub(crate) struct Job {
    pub key:String,pub run:String,pub phase:String,pub message:String,
    #[serde(skip_serializing_if="Option::is_none")] pub percent:Option<u8>,
    pub terminal:bool,pub truncated:bool,
}
impl Job {
    fn new(key:&str,run:&str,phase:&str,message:&str,percent:Option<u8>,terminal:bool)->Option<Self> {
        if key.is_empty()||key.len()>64||run.is_empty()||run.len()>128||phase.is_empty()||phase.len()>64
            ||percent.is_some_and(|value|value>100) {return None;}
        let mut end=message.len().min(1024);while !message.is_char_boundary(end){end-=1;}
        Some(Self{key:key.into(),run:run.into(),phase:phase.into(),message:message[..end].into(),
            percent,terminal,truncated:end!=message.len()})
    }
    fn bytes(&self)->usize {self.key.len()+self.run.len()+self.phase.len()+self.message.len()+128}
}
struct State {
    revision:u64,dirty:u32,jobs:BTreeMap<String,Job>,events:VecDeque<Job>,bytes:usize,
    overflow:bool,closed:bool,error:Option<String>,
}
pub(crate) struct Signal { state:Mutex<State>, telemetry:AtomicBool,pub(crate) wake:Arc<osg_streams::NativeWake>, pub(crate) context_wake:osg_streams::NativeWake, pub(crate) activation_wake:Arc<osg_streams::NativeWake>, resource_wake:Mutex<Weak<osg_streams::NativeWake>>, automatic_wake:Mutex<Weak<osg_streams::NativeWake>>, routing_wake:Mutex<Weak<osg_streams::NativeWake>>, activity_wake:Mutex<Weak<osg_streams::NativeWake>> }
pub(super) struct Batch {
    pub revision:u64,pub dirty:u32,pub events:Vec<Job>,pub jobs:Vec<Job>,pub lost:bool,
}
impl Default for Signal {
    fn default()->Self {Self{state:Mutex::new(State{revision:0,dirty:ALL,jobs:BTreeMap::new(),events:VecDeque::new(),
        bytes:0,overflow:false,closed:false,error:None}),telemetry:AtomicBool::new(false),wake:Arc::new(Default::default()),context_wake:Default::default(),activation_wake:Default::default(),resource_wake:Mutex::default(),automatic_wake:Mutex::default(),routing_wake:Mutex::default(),activity_wake:Mutex::default()}}
}
impl std::fmt::Debug for Signal {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.debug_struct("GameChanges").finish_non_exhaustive()}
}
impl Signal {
    fn tick(state:&mut State,domain:Domain) {
        if let Some(revision)=state.revision.checked_add(1){state.revision=revision;state.dirty|=domain.bit();}
        else {state.closed=true;state.error=Some("Game source revision exhausted. Reopen Cockpit.".into());}
    }
    pub(crate) fn mark(&self,domain:Domain) {
        let mut state=self.state.lock().unwrap_or_else(|p|p.into_inner());
        if state.closed{return;}Self::tick(&mut state,domain);drop(state);self.wake.notify();self.wake_activity();
    }
    pub(crate) fn watch_activity(&self, wake: &Arc<osg_streams::NativeWake>) {
        *self.activity_wake.lock().unwrap_or_else(|p| p.into_inner()) = Arc::downgrade(wake);
        wake.notify();
    }
    fn wake_activity(&self) {
        let wake = {
            let mut watch = self.activity_wake.lock().unwrap_or_else(|p| p.into_inner());
            let wake = watch.upgrade();
            if wake.is_none() { *watch = Weak::new(); }
            wake
        };
        if let Some(wake) = wake { wake.notify(); }
    }
    pub(crate) fn mark_library_commit(&self) {
        self.mark(Domain::Library);
        self.context_wake.notify();
        self.activation_wake.notify();
        self.wake_automatic();
        self.mark_resources();
    }
    pub(crate) fn watch_automatic(&self, wake: &Arc<osg_streams::NativeWake>) {
        *self.automatic_wake.lock().unwrap_or_else(|p| p.into_inner()) = Arc::downgrade(wake);
    }
    pub(crate) fn mark_process_activation(&self) {
        self.mark(Domain::Runtime);
        self.wake_automatic();
    }
    fn wake_automatic(&self) {
        if let Some(wake) = self.automatic_wake.lock().unwrap_or_else(|p| p.into_inner()).upgrade() {
            wake.notify();
        }
    }
    pub(crate) fn watch_vehicle_routing(&self, wake: &Arc<osg_streams::NativeWake>) {
        *self.routing_wake.lock().unwrap_or_else(|p| p.into_inner()) = Arc::downgrade(wake);
    }
    pub(crate) fn mark_vehicle_context(&self) {
        if let Some(wake) = self.routing_wake.lock().unwrap_or_else(|p| p.into_inner()).upgrade() {
            wake.notify();
        }
        self.wake_automatic();
    }
    pub(crate) fn watch_resources(&self, wake: &Arc<osg_streams::NativeWake>) {
        *self.resource_wake.lock().unwrap_or_else(|p| p.into_inner()) = Arc::downgrade(wake);
        wake.notify();
    }
    pub(crate) fn mark_resources(&self) {
        if let Some(wake) = self.resource_wake.lock().unwrap_or_else(|p| p.into_inner()).upgrade() {
            wake.notify();
        }
    }
    pub(crate) fn mark_telemetry(&self) {
        if !self.telemetry.swap(true, Ordering::AcqRel) { self.wake.notify(); }
    }
    pub(super) fn take_telemetry(&self) -> bool { self.telemetry.swap(false, Ordering::AcqRel) }
    pub(crate) fn start_job(&self,domain:Domain,key:&str,run:&str,phase:&str,message:&str,percent:Option<u8>) {
        self.job_inner(domain,key,run,phase,message,percent,false,true);
    }
    pub(crate) fn job(&self,domain:Domain,key:&str,run:&str,phase:&str,message:&str,percent:Option<u8>,terminal:bool) {
        self.job_inner(domain,key,run,phase,message,percent,terminal,false);
    }
    fn job_inner(&self,domain:Domain,key:&str,run:&str,phase:&str,message:&str,percent:Option<u8>,terminal:bool,start:bool) {
        let Some(job)=Job::new(key,run,phase,message,percent,terminal) else{return;};
        let mut state=self.state.lock().unwrap_or_else(|p|p.into_inner());
        if state.closed{return;}
        if !start&&state.jobs.get(key).is_none_or(|old|old.run!=run||old.terminal){return;}
        if state.jobs.get(key)==Some(&job){return;}
        let full=!state.jobs.contains_key(key)&&state.jobs.len()==MAX_JOBS;
        let retired=if full {state.jobs.iter().find(|(_,job)|job.terminal).map(|(key,_)|key.clone())}else{None};
        if full&&retired.is_none() {
            state.closed=true;state.error=Some("Game job source limit exceeded.".into());
        }else{
            if let Some(retired)=retired {
                state.jobs.remove(&retired);
                // Removing a baseline key cannot be represented by an append-only
                // transition. Readers must replace their complete current map.
                state.events.clear();state.bytes=0;state.overflow=true;
            }
            Self::tick(&mut state,domain);state.jobs.insert(key.into(),job.clone());
            if state.events.len()==MAX_EVENTS||state.bytes+job.bytes()>MAX_EVENT_BYTES {
                state.events.clear();state.bytes=0;state.overflow=true;
            }
            if !state.overflow{state.bytes+=job.bytes();state.events.push_back(job);}
        }
        drop(state);self.wake.notify();self.wake_activity();
    }
    pub(super) fn take(&self,bootstrap:bool)->Result<Option<Batch>,String> {
        let mut state=self.state.lock().unwrap_or_else(|p|p.into_inner());
        if let Some(error)=&state.error{return Err(error.clone());}
        if state.closed{return Err("Game read models closed.".into());}
        if !bootstrap&&state.dirty==0&&state.events.is_empty()&&!state.overflow{return Ok(None);}
        let batch=Batch{revision:state.revision,dirty:if bootstrap{ALL}else{state.dirty},
            events:if bootstrap{state.events.clear();vec![]}else{state.events.drain(..).collect()},
            jobs:state.jobs.values().cloned().collect(),lost:state.overflow};
        state.dirty=0;state.bytes=0;state.overflow=false;Ok(Some(batch))
    }
    pub(crate) fn revision(&self)->u64 {self.state.lock().unwrap_or_else(|p|p.into_inner()).revision}
    pub(crate) fn close(&self) {self.state.lock().unwrap_or_else(|p|p.into_inner()).closed=true;self.wake.notify();self.wake_activity();}
}

#[cfg(test)]
#[path = "signals_activity_tests.rs"]
mod activity_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_library_commits_wake_the_parked_game_context_owner() {
        let signal = Signal::default();
        let before = signal.context_wake.observed();
        let activation_before = signal.activation_wake.observed();
        let resources = Arc::new(osg_streams::NativeWake::default());
        signal.watch_resources(&resources);
        let before_resources = resources.observed();
        for domain in [Domain::Library, Domain::Discovery, Domain::Runtime, Domain::Artwork, Domain::Setup, Domain::Permissions] {
            signal.mark(domain);
        }
        signal.mark_telemetry();
        assert_eq!(signal.context_wake.observed(), before);
        assert_eq!(signal.activation_wake.observed(), activation_before);
        assert_eq!(resources.observed(), before_resources);
        signal.mark_library_commit();
        assert_ne!(signal.context_wake.observed(), before);
        assert_ne!(signal.activation_wake.observed(), activation_before);
        assert_ne!(resources.observed(), before_resources);
    }

    #[test]
    fn telemetry_changes_wake_once_without_dirtying_the_game_catalogue() {
        let signal = Signal::default();
        signal.take(true).unwrap();
        let before = signal.wake.observed();
        signal.mark_telemetry();
        let after = signal.wake.observed();
        assert_ne!(before, after);
        signal.mark_telemetry();
        assert_eq!(signal.wake.observed(), after);
        assert!(signal.take_telemetry());
        assert!(!signal.take_telemetry());
        assert!(signal.take(false).unwrap().is_none());
    }
    #[test]
    fn ordered_jobs_have_exact_runs_and_overload_requires_new_baseline() {
        let signal=Signal::default();signal.take(true).unwrap();
        signal.start_job(Domain::Discovery,"discovery","one","scanning","start",None);
        signal.job(Domain::Discovery,"discovery","old","ready","late",None,true);
        signal.job(Domain::Discovery,"discovery","one","ready","done",None,true);
        let batch=signal.take(false).unwrap().unwrap();assert_eq!(batch.events.len(),2);assert!(!batch.lost);
        assert!(batch.events[1].terminal);
        signal.start_job(Domain::Discovery,"discovery","two","scanning","start",None);
        for index in 0..MAX_EVENTS+2 {signal.job(Domain::Discovery,"discovery","two","scanning",&index.to_string(),None,false);}
        let batch=signal.take(false).unwrap().unwrap();assert!(batch.lost);assert!(batch.events.is_empty());
        assert_eq!(batch.jobs[0].run,"two");
    }
    #[test]
    fn truncation_and_exhaustion_are_explicit_and_bootstrap_does_not_replay_old_jobs() {
        let signal=Signal::default();signal.start_job(Domain::Setup,"setup:dcs","run","setting-up",&"é".repeat(800),None);
        let batch=signal.take(true).unwrap().unwrap();assert!(batch.events.is_empty());assert!(batch.jobs[0].truncated);
        assert!(batch.jobs[0].message.len()<=1024);
        signal.state.lock().unwrap().revision=u64::MAX;signal.mark(Domain::Library);
        assert!(signal.take(false).is_err());
    }
    #[test]
    fn completed_job_history_is_bounded_and_eviction_requires_a_new_baseline() {
        let signal=Signal::default();signal.take(true).unwrap();
        for index in 0..MAX_JOBS*3 {
            let key=format!("setup:game-{index}");
            signal.start_job(Domain::Setup,&key,"run","setting-up","start",None);
            signal.job(Domain::Setup,&key,"run","complete","done",None,true);
            let batch=signal.take(false).unwrap().unwrap();
            assert_eq!(batch.jobs.len(),(index+1).min(MAX_JOBS));
            assert_eq!(batch.lost,index>=MAX_JOBS);
            assert!(batch.jobs.iter().any(|job|job.key==key&&job.terminal));
            if batch.lost {assert!(batch.events.is_empty());}
            else {assert_eq!(batch.events.len(),2);}
        }
        assert!(signal.take(false).unwrap().is_none());
        assert!(!signal.state.lock().unwrap().closed);
    }
    #[test]
    fn eviction_preserves_active_jobs_and_truly_active_exhaustion_is_explicit() {
        let signal=Signal::default();signal.take(true).unwrap();
        for index in 0..MAX_JOBS {
            signal.start_job(Domain::Setup,&format!("setup:{index}"),"run","setting-up","start",None);
        }
        signal.job(Domain::Setup,"setup:0","run","complete","done",None,true);
        signal.start_job(Domain::Setup,"replacement","run","setting-up","start",None);
        {
            let state=signal.state.lock().unwrap();
            assert_eq!(state.bytes,0);assert!(state.events.is_empty());assert!(state.overflow);
        }
        let batch=signal.take(false).unwrap().unwrap();
        assert!(batch.lost);assert_eq!(batch.jobs.len(),MAX_JOBS);
        assert!(batch.jobs.iter().all(|job|!job.terminal));
        assert!(!batch.jobs.iter().any(|job|job.key=="setup:0"));
        for index in 1..MAX_JOBS {assert!(batch.jobs.iter().any(|job|job.key==format!("setup:{index}")));}
        assert!(batch.jobs.iter().any(|job|job.key=="replacement"));
        signal.start_job(Domain::Setup,"overflow","run","setting-up","start",None);
        assert_eq!(signal.take(false).err().as_deref(),Some("Game job source limit exceeded."));
        assert_eq!(signal.state.lock().unwrap().jobs.len(),MAX_JOBS);
    }
}
