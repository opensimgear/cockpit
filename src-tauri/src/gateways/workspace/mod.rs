//! Native application ownership, source notifications and passive workspace data.
use super::{Adapter, GatewayManager};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{collections::{BTreeMap,VecDeque},sync::{Arc,Weak,Mutex,Condvar,atomic::{AtomicBool,Ordering}},thread::JoinHandle};
mod policy;
mod publisher;
mod worker;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)] mod command_tests;
#[cfg(test)] mod renewal_tests;
use policy::{Action,Entry,MAX_JOBS};
pub(super) const MAX_GATEWAYS:usize=64;

pub(super) struct Completion {id:String,session:String,token:u64,action:Action,error:Option<String>}
#[derive(Default)]
struct Control {paused:BTreeMap<String,bool>,held:BTreeMap<String,usize>,fences:BTreeMap<String,Arc<CommandFence>>,completed:VecDeque<Completion>,active:usize,ready:Option<Result<(),String>>}
struct Inner {
    wake:Arc<osg_streams::NativeWake>, ports_dirty:AtomicBool, closed:AtomicBool,
    control:Mutex<Control>, changed:Condvar, pages:crate::game_read_models::RecordPages,
}
pub(super) struct Runtime {inner:Arc<Inner>,thread:Mutex<Option<JoinHandle<()>>>}
struct CommandFence {gate:Mutex<()>,epoch:std::sync::atomic::AtomicU64}
impl CommandFence {
    fn dispatch<T>(&self,epoch:u64,action:impl FnOnce()->T)->Result<T,String>{
        let _gate=self.gate.lock().unwrap_or_else(|p|p.into_inner());
        if self.epoch.load(Ordering::Acquire)!=epoch{return Err("Workspace operation was superseded by an explicit disconnect.".into());}
        Ok(action())
    }
}
pub(super) struct CommandHold {inner:Arc<Inner>,id:String,fence:Arc<CommandFence>}
impl CommandHold {
    pub fn dispatch<T>(&self,action:impl FnOnce()->T)->T{let _gate=self.fence.gate.lock().unwrap_or_else(|p|p.into_inner());action()}
}
fn command_fence(control:&mut Control,id:&str)->Result<Arc<CommandFence>,String>{
    if control.fences.len()==MAX_GATEWAYS&&!control.fences.contains_key(id){return Err("Workspace command identity limit reached.".into());}
    Ok(control.fences.entry(id.into()).or_insert_with(||Arc::new(CommandFence{gate:Mutex::new(()),epoch:std::sync::atomic::AtomicU64::new(0)})).clone())
}
impl Drop for CommandHold {
    fn drop(&mut self){
        let mut control=self.inner.control.lock().unwrap_or_else(|p|p.into_inner());
        if let Some(count)=control.held.get_mut(&self.id){*count-=1;if *count==0{control.held.remove(&self.id);}}
        drop(control);self.inner.wake.notify();
    }
}
impl Runtime {
    pub fn new()->Self {Self {inner:Arc::new(Inner {wake:Arc::new(Default::default()),ports_dirty:AtomicBool::new(true),closed:AtomicBool::new(false),
        control:Mutex::new(Control::default()),changed:Condvar::new(),pages:Default::default()}),thread:Mutex::new(None)}}
    pub fn start(&self, manager:Weak<GatewayManager>, notifications:bool)->Result<(),String> {
        let mut thread=self.thread.lock().unwrap_or_else(|p|p.into_inner());
        if self.inner.closed.load(Ordering::Acquire){return Err("Workspace is closed.".into());}
        if thread.is_none(){let inner=self.inner.clone();*thread=Some(std::thread::Builder::new().name("gateway-workspace".into()).spawn(move||worker::run(inner,manager,notifications)).map_err(|e|e.to_string())?);}
        drop(thread);
        let mut control=self.inner.control.lock().unwrap_or_else(|p|p.into_inner());
        while control.ready.is_none()&&!self.inner.closed.load(Ordering::Acquire){control=self.inner.changed.wait(control).unwrap_or_else(|p|p.into_inner());}
        control.ready.clone().unwrap_or_else(||Err("Workspace stopped before initialization.".into()))
    }
    pub fn changed(&self){self.inner.wake.notify();}
    pub fn wake(&self)->Arc<osg_streams::NativeWake>{self.inner.wake.clone()}
    pub fn hold(&self,id:&str)->Result<CommandHold,String>{
        let mut control=self.inner.control.lock().unwrap_or_else(|p|p.into_inner());
        if control.held.values().sum::<usize>()>=MAX_GATEWAYS{return Err("Workspace manual operation limit reached.".into());}
        let fence=command_fence(&mut control,id)?;
        fence.epoch.fetch_update(Ordering::AcqRel,Ordering::Acquire,|epoch|epoch.checked_add(1)).map_err(|_|"Workspace command identity exhausted.")?;
        *control.held.entry(id.into()).or_default()+=1;
        Ok(CommandHold{inner:self.inner.clone(),id:id.into(),fence})
    }
    pub fn pause(&self,id:&str,paused:bool)->Result<(),String>{
        let mut control=self.inner.control.lock().unwrap_or_else(|p|p.into_inner());
        if control.paused.len()==MAX_GATEWAYS&&!control.paused.contains_key(id){return Err("Workspace gateway intent limit reached.".into());}
        control.paused.insert(id.into(),paused);drop(control);self.changed();Ok(())
    }
    pub fn page(&self,request:&Value)->Result<Value,String>{
        if self.inner.closed.load(Ordering::Acquire){return Err("Workspace is closed.".into());}
        let object=request.as_object().ok_or("Invalid workspace page request.")?;
        if object.len()!=3||request["command"]!="workspace_read_model_page"||object.keys().any(|k|!["command","id","offset"].contains(&k.as_str())){return Err("Workspace pages require exactly command, id and offset.".into());}
        self.inner.pages.get(request["id"].as_str().ok_or("Workspace page identity required.")?,request["offset"].as_u64().ok_or("Workspace page offset required.")?)
    }
    pub fn shutdown(&self){
        self.inner.closed.store(true,Ordering::Release);self.inner.wake.notify();self.inner.changed.notify_all();
        if let Some(thread)=self.thread.lock().unwrap_or_else(|p|p.into_inner()).take(){if thread.thread().id()!=std::thread::current().id(){let _=thread.join();}}
        self.inner.pages.clear();
    }
}
impl Drop for Runtime {fn drop(&mut self){self.shutdown();}}
impl GatewayManager {
    pub fn start_workspace(self:&Arc<Self>)->Result<(),String>{
        self.workspace.start(Arc::downgrade(self),self.profile_native)?;
        if self.profile_native {
            let manager = Arc::downgrade(self);
            self.system_inventory.start(move || {
                let Some(manager) = manager.upgrade() else { return false; };
                manager.system_inventory.refresh(&manager.controllers,
                    |sources| manager.system_controller_associations(sources));
                true
            })?;
            self.system_inventory.start_vjoy_status()?;
        }
        Ok(())
    }
    pub(crate) fn system_inventory_vjoy_reader_changed(&self) {
        self.system_inventory.vjoy_reader_changed();
    }
}
