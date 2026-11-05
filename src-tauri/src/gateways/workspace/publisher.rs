use super::*;
use osg_streams::{Publisher,TopicSpec};

pub(super) struct View {
    publisher:Option<Publisher>, previous:Option<Value>,
    encoded:BTreeMap<String,(u64,Value)>, ports:Vec<Value>, clock:String,
    metadata:Option<Metadata>,
}
#[derive(PartialEq)]
struct Metadata {
    entries:BTreeMap<String,(String,u64)>, ports:Vec<Value>, error:String,
}
impl View {
    pub fn new()->Self{Self{publisher:None,previous:None,encoded:BTreeMap::new(),ports:vec![],clock:format!("workspace:{}",uuid::Uuid::new_v4()),metadata:None}}
    pub fn publish(&mut self,inner:&Inner,manager:&GatewayManager,entries:&BTreeMap<String,Entry>,ports:&[Value],error:&str)->Result<(),String>{
        // Source ownership has already committed. Activity observes that boundary
        // even if the independent UI projection or broker cannot accept it.
        let metadata=Metadata{entries:entries.iter().map(|(id,entry)|(id.clone(),(entry.session.clone(),entry.version))).collect(),ports:ports.to_vec(),error:error.into()};
        if self.metadata.as_ref()!=Some(&metadata){
            self.metadata=Some(metadata);
            manager.activity.notify_metadata(crate::activity::metadata::Group::Devices);
        }
        let ports_changed=self.ports!=ports;
        let mut projection=inner.pages.begin();
        let mut encoded=BTreeMap::new();
        let mut nodes=Vec::with_capacity(entries.len());
        for (id,entry) in entries {
            let node=match self.encoded.get(id).filter(|(version,_)|*version==entry.version&&!ports_changed){
                Some((_,node))=>projection.encode(node)?,
                None=>projection.encode(&entry.wire(ports))?,
            };
            encoded.insert(id.clone(),(entry.version,node.clone()));nodes.push(node);
        }
        let payload=json!({"value":projection.encode(&json!({"ports":ports,"inventoryError":error,"entries":nodes}))?});
        projection.commit("workspace.state")?;
        self.encoded=encoded;
        if self.previous.as_ref()!=Some(&payload){
            let captured=manager.observations.captured_us();
            if let Some(publisher)=&self.publisher {
                publisher.publish_latest_state(publisher.generation(),payload.clone(),captured,&self.clock).map_err(|e|e.to_string())?;
            }else{self.publisher=Some(manager.observations.broker().register(TopicSpec{topic:"workspace.state".into(),source:"workspace".into(),
                audiences:osg_streams::KNOWN_AUDIENCES.iter().map(|s|(*s).into()).collect()},payload.clone(),captured,self.clock.clone()).map_err(|e|e.to_string())?);}
            self.previous=Some(payload);
        }
        self.ports=ports.to_vec();Ok(())
    }
    pub fn retire(&mut self,manager:&GatewayManager){if let Some(publisher)=self.publisher.take(){manager.observations.broker().retire(&publisher);}self.previous=None;}
}

impl Drop for View {
    fn drop(&mut self) { /* Broker shutdown or explicit owner cleanup retires registration. */ }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::metadata::Group;
    struct EmptyProvider;
    impl crate::gateways::Provider for EmptyProvider {
        fn ports(&self)->Result<Vec<crate::transport::PortInfo>,String>{Ok(vec![])}
        fn adapter(&self,_:&str)->Arc<dyn Adapter>{panic!("Passive publication cannot acquire an adapter")}
    }

    #[test]
    fn committed_sources_wake_activity_when_broker_fails_and_unchanged_retries_are_quiet() {
        let manager=GatewayManager::with_provider(Arc::new(EmptyProvider));
        let runtime=Runtime::new();
        let wake=manager.activity.metadata_wake(Group::Devices);
        manager.observations.broker().close();
        let mut view=View::new();
        let mut entries=BTreeMap::from([("gateway".into(),Entry::new("gateway".into(),json!({"path":"COM1"})))]);
        let ports=vec![json!({"path":"COM1"})];
        let mut epoch=wake.observed();
        assert!(view.publish(&runtime.inner,&manager,&entries,&ports,"").is_err());
        assert_eq!(wake.observed(),epoch+1);
        epoch=wake.observed();
        assert!(view.publish(&runtime.inner,&manager,&entries,&ports,"").is_err());
        assert_eq!(wake.observed(),epoch);
        entries.get_mut("gateway").unwrap().touch().unwrap();
        assert!(view.publish(&runtime.inner,&manager,&entries,&ports,"").is_err());
        assert_eq!(wake.observed(),epoch+1);
        epoch=wake.observed();
        entries.get_mut("gateway").unwrap().session="replacement".into();
        assert!(view.publish(&runtime.inner,&manager,&entries,&ports,"").is_err());
        assert_eq!(wake.observed(),epoch+1);
        epoch=wake.observed();
        entries.clear();
        assert!(view.publish(&runtime.inner,&manager,&entries,&ports,"").is_err());
        assert_eq!(wake.observed(),epoch+1);
        epoch=wake.observed();
        assert!(view.publish(&runtime.inner,&manager,&entries,&[],"enumeration failed").is_err());
        assert_eq!(wake.observed(),epoch+1);
        epoch=wake.observed();
        assert!(view.publish(&runtime.inner,&manager,&entries,&[],"enumeration failed").is_err());
        assert_eq!(wake.observed(),epoch);
    }

    #[test]
    fn committed_sources_wake_before_failed_encoding_without_repeated_notifications() {
        let manager=GatewayManager::with_provider(Arc::new(EmptyProvider));
        let runtime=Runtime::new();
        let wake=manager.activity.metadata_wake(Group::Devices);
        let mut view=View::new();
        let mut entry=Entry::new("gateway".into(),json!({"path":"COM1"}));
        entry.state=Arc::new(json!({"connected":true,"port":"COM1","modules":[],"invalid":{("x".repeat(4097)):true}}));
        let entries=BTreeMap::from([("gateway".into(),entry)]);
        let epoch=wake.observed();
        assert!(view.publish(&runtime.inner,&manager,&entries,&[],"").unwrap_err().contains("key exceeds"));
        assert_eq!(wake.observed(),epoch+1);
        assert!(view.publish(&runtime.inner,&manager,&entries,&[],"").is_err());
        assert_eq!(wake.observed(),epoch+1);
    }
}
