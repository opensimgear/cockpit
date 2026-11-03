use super::*;
use osg_streams::{Publisher,TopicSpec};

pub(super) struct View {
    publisher:Option<Publisher>, previous:Option<Value>,
    encoded:BTreeMap<String,(u64,Value)>, ports:Vec<Value>, clock:String,
}
impl View {
    pub fn new()->Self{Self{publisher:None,previous:None,encoded:BTreeMap::new(),ports:vec![],clock:format!("workspace:{}",uuid::Uuid::new_v4())}}
    pub fn publish(&mut self,inner:&Inner,manager:&GatewayManager,entries:&BTreeMap<String,Entry>,ports:&[Value],error:&str)->Result<(),String>{
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
