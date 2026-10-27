use super::*;
use crate::profile_session::{RunOwner,State,Status};

/// Called by the native coordinator worker while its status is current. This
/// compares in-memory state only and queues transitions, never writing storage.
pub(crate) struct Observer {
    sender: mpsc::Sender<observations::Batch>,
    registry:Arc<Mutex<reconcile::Reconciler>>,
    previous: Option<(State,Option<RunOwner>,Vec<String>,Option<String>)>,
    snapshot: Snapshot,
    names: std::collections::BTreeMap<String,String>,
}
impl Observer {
    pub(super) fn identity(&self)->Option<(String,String)> {self.snapshot.identity("profile_session:current")}
    pub(super) fn new(sender: mpsc::Sender<observations::Batch>,registry:Arc<Mutex<reconcile::Reconciler>>) -> Self { Self { sender,registry,previous: None, snapshot: Snapshot::default(),names:Default::default() } }
    pub(crate) fn names(&mut self, profiles: &[osg_profiles::ProfileDocument]) { self.names=profiles.iter().map(|profile|(profile.id.clone(),profile.name.clone())).collect(); }
    pub(crate) fn observe(&mut self,status: &Status) {
        let signature=(status.state,status.owner.clone(),status.degradation.clone(),status.fault.clone());
        let started=std::time::Instant::now();
        if self.previous.as_ref()==Some(&signature) {
            if self.registry.lock().unwrap_or_else(|p|p.into_inner()).watches_session() {
                let _=self.sender.send(observations::Batch::events(vec![],&self.snapshot,started).acknowledge(&["profile_session"]));
            }
            return;
        }
        let mut snapshot = Snapshot::default();
        let mut value=serde_json::to_value(status).unwrap_or_default();
        if status.owner.is_none() && status.applied_snapshot.is_none() {
            if let Some((_,Some(owner),_,_))=&self.previous { value["appliedSnapshot"]=json!({"profileId":owner.profile_id}); }
        }
        snapshot.profile_session(value,&self.names);
        if self.previous.is_some() {
            let _=self.sender.send(observations::Batch::between(&self.snapshot,&snapshot,started).acknowledge(&["profile_session"]));
        }
        self.snapshot=snapshot;
        self.previous=Some(signature);
    }
}
