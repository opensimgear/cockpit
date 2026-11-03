use super::*;
use crate::module_usb::stream::{Batch, Capture, Snapshot};
use osg_streams::{Broker, Delivery, NativeSubscription, NativeWake, NativeWatch, Publisher, TopicSpec};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    sync::atomic::AtomicU64,
    thread::{self, JoinHandle},
};

const MAX_INPUT_BRIDGES: usize = 4;

pub(crate) fn topic(key: &str) -> String {
    format!("usb.inputs.{:x}", Sha256::digest(key.as_bytes()))
}

pub(super) struct Entry {
    bridge: Arc<Bridge>,
    worker: Option<JoinHandle<()>>,
}
impl Entry {
    #[cfg(test)]
    pub(super) fn idle_probe(&self)->std::sync::mpsc::Receiver<()>{
        let (tx,rx)=std::sync::mpsc::channel();
        *self.bridge.probe.lock().unwrap_or_else(|p|p.into_inner())=Some(tx);
        self.bridge.wake.notify();rx
    }
    pub(super) fn stop(&mut self) {
        self.bridge.stop.store(true, Ordering::Release);
        self.bridge.wake.notify();
        if let Some(worker) = self.worker.take() {
            if worker.thread().id() != thread::current().id() { let _ = worker.join(); }
        }
        self.bridge.broker.retire(&self.bridge.publisher);
    }
}
struct Bridge {
    #[cfg(test)]
    probe:Mutex<Option<std::sync::mpsc::Sender<()>>>,
    key: String,
    wake: Arc<NativeWake>,
    stop: AtomicBool,
    armed: AtomicBool,
    running: AtomicBool,
    generation: AtomicU64,
    admission: Mutex<()>,
    publisher: Publisher,
    broker: Broker,
    clock: Mutex<(u64, String)>,
    payload: Mutex<Value>,
}
struct Binding {
    capture: Arc<Capture>,
    subscription: Option<NativeSubscription<Snapshot, Batch>>,
    _watch: Option<NativeWatch<Snapshot, Batch>>,
    native_generation: u64,
    retry: bool,
    route: crate::module_usb::UsbRouteToken,
}
struct Initial {
    binding: Option<Binding>,
    payload: Value,
    captured_us: u64,
    clock_id: String,
}

impl Fleet {
    pub(crate) fn with_input_observation<T>(
        self: &Arc<Self>, topic_name: &str, broker: Broker,
        before_subscribe: impl FnOnce(Arc<NativeWake>), action: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let key = self.known_input_key(topic_name)?;
        let mut before_subscribe=Some(before_subscribe);let mut action=Some(action);
        loop {
            if self.closed.load(Ordering::Acquire){return Err("USB input fleet is closed.".into());}
            let bridge=self.input_bridge(&key,broker.clone())?;
            let admission=bridge.admission.lock().unwrap_or_else(|p|p.into_inner());
            if bridge.stop.load(Ordering::Acquire) {
                drop(admission);self.retire_input_bridge(&key,&bridge);continue;
            }
            let before=before_subscribe.take().expect("observation admission callback");
            before(bridge.wake.clone());
            let subscribe=action.take().expect("observation subscribe action");
            let result=subscribe();
            let retire=!bridge.publisher.has_readers();
            if retire{bridge.stop.store(true,Ordering::Release);}
            bridge.armed.store(true, Ordering::Release);bridge.wake.notify();drop(admission);
            if retire {self.retire_input_bridge(&key,&bridge);}
            return result;
        }
    }

    fn known_input_key(&self, requested: &str) -> Result<String, String> {
        let suffix=requested.strip_prefix("usb.inputs.").ok_or("Invalid USB input observation topic.")?;
        if suffix.len()!=64 || !suffix.bytes().all(|c|c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
            return Err("Invalid USB input observation topic hash.".into());
        }
        self.state.lock().unwrap_or_else(|p|p.into_inner()).slots.keys()
            .find(|key|topic(key)==requested).cloned()
            .ok_or_else(||"Unknown USB input endpoint topic.".into())
    }

    fn input_bridge(self: &Arc<Self>, key: &str, broker: Broker) -> Result<Arc<Bridge>, String> {
        let mut bridges=self.input_bridges.lock().unwrap_or_else(|p|p.into_inner());
        if self.closed.load(Ordering::Acquire){return Err("USB input fleet is closed.".into());}
        if let Some(entry)=bridges.get(key) {
            if entry.bridge.running.load(Ordering::Acquire)&&!entry.bridge.stop.load(Ordering::Acquire) { return Ok(entry.bridge.clone()); }
        }
        if let Some(mut retired)=bridges.remove(key) {
            drop(bridges);retired.stop();return self.input_bridge(key,broker);
        }
        if bridges.len()>=MAX_INPUT_BRIDGES { return Err("USB input observation bridge limit reached.".into()); }
        let wake=Arc::new(NativeWake::default());
        self.watch_inputs(key,wake.clone())?;
        let initial=self.initial(key,&wake)?;
        let topic_name=topic(key);
        let publisher=broker.register(TopicSpec { topic:topic_name, source:"usb-inputs".into(),
            audiences:vec!["ui".into(),"browser".into(),"mcp".into()] },
            initial.payload.clone(),initial.captured_us,initial.clock_id.clone()).map_err(|e|e.to_string())?;
        let bridge=Arc::new(Bridge {
            #[cfg(test)]
            probe:Mutex::new(None),
            key:key.into(),wake:wake.clone(),stop:AtomicBool::new(false),
            armed:AtomicBool::new(false),running:AtomicBool::new(true),generation:AtomicU64::new(1),
            publisher,broker,clock:Mutex::new((initial.captured_us,initial.clock_id)),
            payload:Mutex::new(initial.payload),admission:Mutex::new(()) });
        let weak=Arc::downgrade(self);let cleanup_fleet=weak.clone();let worker_bridge=bridge.clone();
        let spawned=thread::Builder::new().name("usb-input-observation".into()).spawn(move||{
            let result=catch_unwind(AssertUnwindSafe(|| run(weak,worker_bridge.clone(),initial.binding)));
            if result.is_err() { worker_bridge.broker.retire(&worker_bridge.publisher); }
            worker_bridge.running.store(false,Ordering::Release);
            if let Some(fleet)=cleanup_fleet.upgrade(){fleet.retire_input_bridge(&worker_bridge.key,&worker_bridge);}
        });
        let worker=match spawned {
            Ok(worker)=>worker,
            Err(error)=>{bridge.broker.retire(&bridge.publisher);return Err(format!("Cannot start USB input observation bridge: {error}"));}
        };
        bridges.insert(key.into(),Entry{bridge:bridge.clone(),worker:Some(worker)});
        Ok(bridge)
    }

    fn retire_input_bridge(&self,key:&str,bridge:&Arc<Bridge>){
        let entry={let mut bridges=self.input_bridges.lock().unwrap_or_else(|p|p.into_inner());
            if bridges.get(key).is_some_and(|entry|Arc::ptr_eq(&entry.bridge,bridge)){bridges.remove(key)}else{None}};
        if let Some(mut entry)=entry{entry.stop();}
    }

    fn initial(&self,key:&str,wake:&Arc<NativeWake>)->Result<Initial,String>{
        let capture=self.input_capture(key).ok();
        let Some(capture)=capture else{return Ok(unavailable(key,"USB input capture is not connected.",0,"usb-inputs".into()));};
        let _=self.watch_inputs(key,wake.clone());
        match bind(&capture,wake) {
            Ok((binding,delivery))=>{
                let Delivery::Snapshot(snapshot)=delivery else{return Err("USB capture did not provide its initial snapshot.".into());};
                Ok(Initial {payload:snapshot_payload(key,&snapshot.value),captured_us:snapshot.captured_us,
                    clock_id:snapshot.clock_id.clone(),binding:Some(binding)})
            }
            Err(error)=>{
                let snapshot=capture.snapshot();
                let mut initial=unavailable_snapshot(key,&snapshot,&error);
                initial.binding=Some(detached(capture,snapshot.route));
                Ok(initial)
            }
        }
    }
}

fn bind(capture:&Arc<Capture>,wake:&Arc<NativeWake>)->Result<(Binding,Delivery<Snapshot,Batch>),String>{
    let watch=capture.watch(wake.clone())?;
    let mut subscription=capture.subscribe()?;
    let first=subscription.try_recv().map_err(|e|format!("{e:?}"))?.ok_or("USB capture baseline is not ready yet.")?;
    let route=match &first {Delivery::Snapshot(s)=>s.value.route.clone(),Delivery::Event(_)|Delivery::Loss(_)=>capture.snapshot().route};
    let native_generation=match &first {Delivery::Snapshot(s)=>s.generation,Delivery::Event(e)=>e.generation,Delivery::Loss(l)=>l.generation};
    Ok((Binding{capture:capture.clone(),subscription:Some(subscription),_watch:Some(watch),native_generation,retry:true,route},first))
}
fn detached(capture:Arc<Capture>,route:UsbRouteToken)->Binding{
    Binding{capture,subscription:None,_watch:None,native_generation:0,retry:false,route}
}

fn snapshot_payload(key:&str,snapshot:&Snapshot)->Value{
    json!({"key":key,"route":&snapshot.route,"ready":snapshot.ready,
        "state":snapshot.state.as_deref(),"error":snapshot.error})
}
fn unavailable(key:&str,error:&str,captured_us:u64,clock_id:String)->Initial{
    Initial{binding:None,payload:json!({"key":key,"route":null,"ready":false,"state":null,"error":error}),captured_us,clock_id}
}
fn unavailable_snapshot(key:&str,snapshot:&Snapshot,error:&str)->Initial{
    Initial{binding:None,payload:json!({"key":key,"route":&snapshot.route,"ready":false,
        "state":null,"error":error}),captured_us:0,clock_id:"usb-inputs".into()}
}

fn run(fleet:Weak<Fleet>,bridge:Arc<Bridge>,mut binding:Option<Binding>){
    let _=bridge.wake.bind_thread(thread::current());
    loop{
        let observed=bridge.wake.observed();
        if bridge.stop.load(Ordering::Acquire){break;}
        let Some(fleet)=fleet.upgrade() else{break;};
        let current=fleet.input_capture(&bridge.key).ok();
        let replaced=match (&binding,&current){(Some(old),Some(new))=>!Arc::ptr_eq(&old.capture,new),(Some(_),None)=>true,(None,Some(_))=>true,(None,None)=>false};
        if replaced{
            if let Some(capture)=current{
                let _=fleet.watch_inputs(&bridge.key,bridge.wake.clone());
                match bind(&capture,&bridge.wake){
                    Ok((next,delivery))=>{
                        let (payload,at,clock)=delivery_payload(&bridge.key,&next,&delivery);
                        if replace(&bridge,&payload,at,&clock){binding=Some(next);}else{binding=None;}
                    }
                    Err(error)=>{
                        let snapshot=capture.snapshot();let initial=unavailable_snapshot(&bridge.key,&snapshot,&error);
                        replace(&bridge,&initial.payload,initial.captured_us,&initial.clock_id);
                        // A failed watch/subscriber admission may itself have
                        // notified this wake. Never turn that into a retry loop.
                        binding=Some(detached(capture,snapshot.route));
                    }
                }
            }else{
                let (at,clock)=bridge.clock.lock().unwrap_or_else(|p|p.into_inner()).clone();
                let value=unavailable(&bridge.key,"USB input capture is disconnected.",at,clock);
                replace(&bridge,&value.payload,value.captured_us,&value.clock_id);binding=None;
            }
        }
        if let Some(source)=binding.as_mut(){
            if source.subscription.is_none()&&source.retry{
                source._watch.take();
                if let Ok((mut next,delivery))=bind(&source.capture,&bridge.wake){
                    let (payload,at,clock)=delivery_payload(&bridge.key,&next,&delivery);
                    if replace(&bridge,&payload,at,&clock){
                        source.subscription=next.subscription.take();source._watch=next._watch.take();
                        source.native_generation=next.native_generation;source.route=next.route;
                    }
                }else{source.retry=false;}
            }
            if let Some(subscription)=source.subscription.as_mut(){
                let mut detach_subscription=false;
                loop{if bridge.stop.load(Ordering::Acquire){break;}match subscription.try_recv(){
                    Ok(Some(Delivery::Snapshot(snapshot)))=>{
                        if snapshot.generation!=source.native_generation{
                            source.native_generation=snapshot.generation;
                            replace(&bridge,&snapshot_payload(&bridge.key,&snapshot.value),snapshot.captured_us,&snapshot.clock_id);
                        }else{publish(&bridge,&snapshot_payload(&bridge.key,&snapshot.value),snapshot.captured_us,&snapshot.clock_id);}
                    }
                    Ok(Some(Delivery::Event(event)))=>{
                        let mut metadata=(*event.value.latest.state).clone();
                        if let Some(object)=metadata.as_object_mut(){for key in ["events","after","next","more"]{object.remove(key);}}
                        let payload=json!({"key":bridge.key,"route":source.route,"ready":true,"state":metadata,"error":null});
                        if event.generation!=source.native_generation{source.native_generation=event.generation;replace(&bridge,&payload,event.captured_us,&event.clock_id);}
                        else{publish(&bridge,&payload,event.captured_us,&event.clock_id);}
                    }
                    Ok(Some(Delivery::Loss(loss)))=>{
                        let (at,clock)=bridge.clock.lock().unwrap_or_else(|p|p.into_inner()).clone();
                        let payload=json!({"key":bridge.key,"route":source.route,"ready":false,"state":null,
                            "error":format!("USB input capture lost continuity: {:?}",loss.reason)});
                        replace(&bridge,&payload,at,&clock);
                        if loss.reason==osg_streams::LossReason::GenerationReplaced{continue;}
                        source.retry=loss.reason==osg_streams::LossReason::Overflow;
                        detach_subscription=true;break;
                    }
                    Ok(None)=>break,
                    Err(error)=>{
                        let (at,clock)=bridge.clock.lock().unwrap_or_else(|p|p.into_inner()).clone();
                        let payload=json!({"key":bridge.key,"route":source.route,"ready":false,"state":null,
                            "error":format!("USB input capture is unavailable: {error:?}")});
                        replace(&bridge,&payload,at,&clock);source.retry=false;detach_subscription=true;break;
                    }
                }}
                if detach_subscription { source.subscription=None;source._watch.take();bridge.wake.notify(); }
            }
        }
        drop(fleet);
        let admission=bridge.admission.lock().unwrap_or_else(|p|p.into_inner());
        let retire=bridge.armed.load(Ordering::Acquire)&&!bridge.publisher.has_readers();
        if retire{bridge.stop.store(true,Ordering::Release);}
        drop(admission);
        if retire{break;}
        #[cfg(test)]
        if let Some(probe)=bridge.probe.lock().unwrap_or_else(|p|p.into_inner()).take(){let _=probe.send(());}
        bridge.wake.wait_scheduled(observed,None);
    }
    bridge.broker.retire(&bridge.publisher);
    if let Some(fleet)=fleet.upgrade(){fleet.retire_input_bridge(&bridge.key,&bridge);}
}

fn delivery_payload(key:&str,binding:&Binding,delivery:&Delivery<Snapshot,Batch>)->(Value,u64,String){
    match delivery{
        Delivery::Snapshot(snapshot)=>(snapshot_payload(key,&snapshot.value),snapshot.captured_us,snapshot.clock_id.clone()),
        Delivery::Event(event)=>{
            let mut state=(*event.value.latest.state).clone();if let Some(o)=state.as_object_mut(){for name in ["events","after","next","more"]{o.remove(name);}}
            (json!({"key":key,"route":binding.route,"ready":true,"state":state,"error":null}),event.captured_us,event.clock_id.clone())
        }
        Delivery::Loss(loss)=>(json!({"key":key,"route":null,"ready":false,"state":null,"error":format!("USB input capture lost continuity: {:?}",loss.reason)}),0,"usb-inputs".into()),
    }
}
fn publish(bridge:&Bridge,payload:&Value,at:u64,clock:&str){
    let last=bridge.payload.lock().unwrap_or_else(|p|p.into_inner());if *last==*payload{return;}
    if bridge.publisher.publish_latest_state(bridge.generation.load(Ordering::Acquire),payload.clone(),at,clock).is_ok(){
        *bridge.clock.lock().unwrap_or_else(|p|p.into_inner())=(at,clock.to_owned());
        drop(last);*bridge.payload.lock().unwrap_or_else(|p|p.into_inner())=payload.clone();
    }else{fail_bridge(bridge);}
}
fn replace(bridge:&Bridge,payload:&Value,at:u64,clock:&str)->bool{
    let next=match bridge.generation.load(Ordering::Acquire).checked_add(1){Some(v)=>v,None=>{fail_bridge(bridge);return false;}};
    if bridge.publisher.replace_generation(next,payload.clone(),at,clock).is_err(){fail_bridge(bridge);return false;}
    bridge.generation.store(next,Ordering::Release);*bridge.clock.lock().unwrap_or_else(|p|p.into_inner())=(at,clock.to_owned());
    *bridge.payload.lock().unwrap_or_else(|p|p.into_inner())=payload.clone();true
}
fn fail_bridge(bridge:&Bridge){bridge.stop.store(true,Ordering::Release);bridge.wake.notify();bridge.broker.retire(&bridge.publisher);}
