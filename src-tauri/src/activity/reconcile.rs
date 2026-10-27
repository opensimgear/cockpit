use super::{observations::{Batch,Observation},scopes::Scope,NewActivityEvent};
use std::{collections::BTreeMap,time::Instant};

struct Active {scope:Scope,started:Instant}
struct Receipt { expected:BTreeMap<String,String>, started:Instant,finished:Instant }
#[derive(Default)]
pub(super) struct Reconciler {
    active:BTreeMap<String,Active>,
    receipts:Vec<Receipt>,
    deferred:Vec<Observation>,
}
impl Reconciler {
    pub fn begin(&mut self,key:String,scope:Scope) {self.active.insert(key,Active{scope,started:Instant::now()});}
    pub fn finish(&mut self,key:&str,expected:BTreeMap<String,String>) {
        let started=self.active.remove(key).map(|active|active.started).unwrap_or_else(Instant::now);
        if !expected.is_empty(){self.receipts.push(Receipt{expected,started,finished:Instant::now()});}
    }
    pub fn abandon(&mut self,key:&str) {self.active.remove(key);}
    pub fn watches_session(&self)->bool {
        self.active.values().any(|active|active.scope.watches("profile_session"))||self.receipts.iter().any(|receipt|receipt.expected.contains_key("profile_session:current"))
    }
    fn resolve(&mut self,observation:Observation,ready:&mut Vec<NewActivityEvent>) {
        if let Some(receipt)=self.receipts.iter_mut().find(|receipt|observation.observed_at>=receipt.started&&receipt.expected.get(&observation.subject)==Some(&observation.state)) {
            receipt.expected.remove(&observation.subject);
        } else if self.active.values().any(|active|observation.observed_at>=active.started&&active.scope.covers(&observation.subject)) {
            self.deferred.push(observation);
        } else {ready.push(observation.event);}
    }
    pub fn ready(&mut self)->Vec<NewActivityEvent> {
        let mut ready=vec![];
        for observation in std::mem::take(&mut self.deferred) {self.resolve(observation,&mut ready);}
        self.receipts.retain(|receipt|!receipt.expected.is_empty());
        ready
    }
    pub fn observe(&mut self,batch:Batch)->Vec<NewActivityEvent> {
        let mut ready=self.ready();
        for observation in batch.events {self.resolve(observation,&mut ready);}
        // A capture wholly after completion acknowledges even a no-op or a
        // superseded terminal state. Never leave a claim for a later action.
        for receipt in &mut self.receipts {
            if batch.started>receipt.finished {
                receipt.expected.retain(|subject,_| {
                    // Independent producers acknowledge only their own kind.
                    // A missing subject is a deletion only for a kind sampled.
                    let kind=subject.split(':').next().unwrap_or("");
                    !batch.states.contains_key(subject)&&!batch.states.contains_key(&format!("@{kind}"))
                });
            }
        }
        self.receipts.retain(|receipt|!receipt.expected.is_empty());
        ready
    }
}

#[cfg(test)]
#[path="reconcile_tests.rs"]
mod tests;
