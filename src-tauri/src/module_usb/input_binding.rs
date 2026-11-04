//! Connection-scoped capture binding. Old receivers never address a new capture.
use super::{stream::Capture, UsbRouteToken};
use crate::module_console::actor::{Input, Sink};
use std::sync::{Arc,Mutex,Weak};

#[derive(Default)]
pub(super) struct Inputs {
    current:Mutex<Option<Arc<Capture>>>,
    watchers:Mutex<Vec<Weak<osg_streams::NativeWake>>>,
}
impl Inputs {
    pub fn current(&self)->Option<Arc<Capture>>{self.current.lock().unwrap_or_else(|p|p.into_inner()).clone()}
    pub fn watch(&self,wake:Arc<osg_streams::NativeWake>){
        let mut watchers=self.watchers.lock().unwrap_or_else(|p|p.into_inner());
        watchers.retain(|w|w.strong_count()>0);
        let weak=Arc::downgrade(&wake);
        if watchers.len()<8&&!watchers.iter().any(|old|old.ptr_eq(&weak)){watchers.push(weak);}
    }
    pub(super) fn changed(&self){
        let wakes={let mut watchers=self.watchers.lock().unwrap_or_else(|p|p.into_inner());
            watchers.retain(|w|w.strong_count()>0);watchers.iter().filter_map(Weak::upgrade).collect::<Vec<_>>()};
        for wake in wakes{wake.notify();}
    }
}
pub(super) struct Binding {inputs:Arc<Inputs>,local:Arc<Mutex<Option<Arc<Capture>>>>}
impl Binding {
    pub fn new(inputs:Arc<Inputs>)->Self{Self{inputs,local:Arc::new(Mutex::new(None))}}
    pub fn sink(&self)->Sink{
        let local=self.local.clone();
        Arc::new(move|input:Input|{
            let capture=local.lock().unwrap_or_else(|p|p.into_inner()).clone();
            if let Some(capture)=capture{capture.accept(input);}
        })
    }
    pub fn install(&self,route:UsbRouteToken)->Result<Arc<Capture>,String>{
        self.close();let capture=Capture::new(route)?;
        *self.local.lock().unwrap_or_else(|p|p.into_inner())=Some(capture.clone());
        *self.inputs.current.lock().unwrap_or_else(|p|p.into_inner())=Some(capture.clone());
        self.inputs.changed();Ok(capture)
    }
    pub fn close(&self){
        let Some(capture)=self.local.lock().unwrap_or_else(|p|p.into_inner()).take()else{return;};
        capture.close();
        let mut current=self.inputs.current.lock().unwrap_or_else(|p|p.into_inner());
        if current.as_ref().is_some_and(|active|Arc::ptr_eq(active,&capture)){current.take();}
        drop(current);self.inputs.changed();
    }
}
impl Drop for Binding {fn drop(&mut self){self.close();}}
