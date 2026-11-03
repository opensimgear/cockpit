use super::*;
use crate::stm32::workspace::Cache;
use std::time::{Duration,Instant};
const ID:&str="stm32:00112233445566778899aabb";

struct Device {cache:Cache,calls:Mutex<Vec<String>>,reject_disconnect:AtomicBool}
impl Device {
    fn new()->Self{Self{cache:Cache::new(Value::Null),calls:Mutex::new(vec![]),reject_disconnect:AtomicBool::new(false)}}
}
impl Adapter for Device {
    fn watch_workspace(&self,wake:Arc<osg_streams::NativeWake>){self.cache.watch(wake)}
    fn workspace_snapshot(&self)->(Arc<Value>,BTreeMap<String,String>){self.cache.visible_snapshot()}
    fn set_workspace_owned(&self,owned:bool){self.cache.set_owned(owned)}
    fn cached_state(&self)->Value{self.cache.visible_snapshot().0.as_ref().clone()}
    fn request(&self,request:Value)->Value{
        let command=request["command"].as_str().unwrap();
        self.calls.lock().unwrap().push(command.into());
        let mut state=self.cached_state();
        if command=="disconnect"&&self.reject_disconnect.load(Ordering::Acquire){return json!({"ok":false,"error":"Release setup claims first.","state":state});}
        match command {
            "connect"=>state=json!({"connected":true,"port":request["port"],"backend":"stm32","gatewaySerial":"00112233445566778899aabb",
                "modules":[{"id":"panel","online":true,"canConfigure":true,"boot":1}],"moduleConfigs":{},"moduleActions":{}}),
            "config_read"=>state["moduleConfigs"]["panel"]=json!({"boot":1,"settings":{}}),
            "action_describe"=>state["moduleActions"]["panel"]=json!({"boot":1,"actions":[]}),
            "disconnect"=>state["connected"]=json!(false),
            _=>return json!({"ok":false,"error":"unexpected command","state":state}),
        }
        self.cache.commit(state.clone(),BTreeMap::new());
        json!({"ok":true,"state":state})
    }
    fn shutdown(&self){self.cache.set_owned(false)}
}
struct Devices {created:Mutex<Vec<Arc<Device>>>}
impl super::super::Provider for Devices {
    fn ports(&self)->Result<Vec<crate::transport::PortInfo>,String>{Ok(vec![crate::transport::PortInfo{
        path:"COM1".into(),serial_number:Some("00112233445566778899aabb".into()),vendor_id:Some("1209".into()),
        product_id:Some("4010".into()),manufacturer:Some("OpenSimGear".into()),friendly_name:Some("OpenSimGear Gateway".into()),
    }])}
    fn adapter(&self,_:&str)->Arc<dyn Adapter>{let adapter=Arc::new(Device::new());self.created.lock().unwrap().push(adapter.clone());adapter}
}
fn fixture()->(Arc<GatewayManager>,Arc<Devices>){
    let provider=Arc::new(Devices{created:Mutex::new(vec![])});
    let manager=Arc::new(GatewayManager::with_provider(provider.clone()));
    manager.start_workspace().unwrap();(manager,provider)
}
fn until(mut condition:impl FnMut()->bool){let deadline=Instant::now()+Duration::from_secs(3);while !condition(){assert!(Instant::now()<deadline,"workspace lifecycle did not settle");std::thread::sleep(Duration::from_millis(1));}}
fn ready(provider:&Devices)->Option<Arc<Device>>{
    provider.created.lock().unwrap().last().cloned().filter(|device|device.cached_state()["moduleActions"]["panel"].is_object()&&device.cache.owned())
}
fn wait_ready(manager:&GatewayManager,provider:&Devices){
    let deadline=Instant::now()+Duration::from_secs(3);
    while ready(provider).is_none(){
        if Instant::now()>=deadline {
            let mut reader=manager.observations.broker().subscribe("failure","ui","workspace.state").unwrap();
            panic!("Workspace failed: {:?}; devices: {:?}",reader.try_recv().unwrap().map(|frame|frame.payload),
                provider.created.lock().unwrap().iter().map(|device|(device.cached_state(),device.calls.lock().unwrap().clone())).collect::<Vec<_>>());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn workspace_native_owner_connects_hydrates_and_preserves_connection_after_rejected_disconnect(){
    let(manager,provider)=fixture();
    wait_ready(&manager,&provider);let device=ready(&provider).unwrap();
    assert_eq!(*device.calls.lock().unwrap(),vec!["connect","config_read","action_describe"]);
    device.reject_disconnect.store(true,Ordering::Release);
    let response=manager.request(json!({"command":"disconnect","gatewayId":ID,"clientId":"workspace-lifecycle-test"}));
    assert_eq!(response["ok"],false);
    manager.workspace.changed();std::thread::sleep(Duration::from_millis(25));
    assert!(device.cache.owned());assert_eq!(device.cached_state()["connected"],true);
    assert_eq!(device.calls.lock().unwrap().len(),4,"unchanged state must not request refresh or retry rejected manual commands");
    device.reject_disconnect.store(false,Ordering::Release);
    assert_eq!(manager.request(json!({"command":"disconnect","gatewayId":ID,"clientId":"workspace-lifecycle-test"}))["ok"],true);
    until(||!device.cache.owned());
    std::thread::sleep(Duration::from_millis(25));
    assert_eq!(device.cached_state()["connected"],false);
    assert_eq!(device.calls.lock().unwrap().len(),5,"explicit disconnect must remain paused");
    manager.shutdown();
}

#[test]
fn workspace_session_removal_withdraws_old_cache_and_recreates_current_native_session(){
    let(manager,provider)=fixture();wait_ready(&manager,&provider);
    let old=ready(&provider).unwrap();
    manager.sessions.lock().unwrap().remove(ID);manager.workspace.changed();
    until(||provider.created.lock().unwrap().len()==2&&ready(&provider).is_some());
    assert!(!old.cache.owned());
    let current=ready(&provider).unwrap();assert!(!Arc::ptr_eq(&old,&current));
    assert_eq!(*current.calls.lock().unwrap(),vec!["connect","config_read","action_describe"]);
    manager.shutdown();assert!(!current.cache.owned());
}
