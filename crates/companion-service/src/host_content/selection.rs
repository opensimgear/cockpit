//! One guarded authoring commit for a saved head and all direct selections.
use super::*;
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Selection {expected_host_token:String,companion_ids:Option<Vec<String>>}
impl Hub {
    pub(in crate::hub) fn snapshot_token(&self)->String {
        format!("{}:{}",self.runtime_identity,self.record_revision.load(std::sync::atomic::Ordering::Acquire))
    }
    pub(in crate::hub) fn publish_selection(&self,inner:&mut Inner,publication:Publication,value:&Value)->Result<Value,String>{
        let selection:Selection=serde_json::from_value(value.clone()).map_err(|_|"Invalid saved panel selection.")?;
        ensure(selection.expected_host_token==self.snapshot_token(),"Companion assignments changed. Refresh before saving.")?;
        let owned=|assignment:&&Assignment|inner.publications.get(&assignment.publication_hash).is_some_and(|p|p.ui_id==publication.ui_id);
        let current=inner.record.instances.iter().filter(owned).cloned().collect::<Vec<_>>();
        let desired=match selection.companion_ids {
            Some(ids)=>{ensure(ids.len()<=MAX_INSTANCES,"Too many selected companions.")?;ids.into_iter().collect::<BTreeSet<_>>()},
            None=>current.iter().map(|a|a.companion_id.clone()).collect(),
        };
        for companion in &desired {
            ensure(id(companion)&&inner.record.credentials.iter().any(|c|c.companion_id==*companion),"The selected companion is no longer paired.")?;
            Self::validate_assignment_target(inner,companion,&publication)?;
        }
        let profile_owned=inner.record.instances.iter().filter(|a|a.ownership.is_some()).map(|a|a.companion_id.clone()).collect::<BTreeSet<_>>();
        let mut record=inner.record.clone();
        record.instances.retain(|a|!current.iter().any(|old|old.instance_id==a.instance_id)
            ||profile_owned.contains(&a.companion_id)||desired.contains(&a.companion_id));
        for companion in desired.iter().filter(|id|!profile_owned.contains(*id)) {
            let mut targets=current.iter().filter(|a|a.companion_id==*companion).map(|a|a.instance_id.clone()).collect::<Vec<_>>();
            if targets.is_empty(){
                let existing=inner.record.instances.iter().filter(|a|a.companion_id==*companion).collect::<Vec<_>>();
                ensure(existing.len()<=1,"This companion has multiple controller assignments. Disconnect its extra assignments before assigning a panel.")?;
                targets.push(existing.first().map(|a|a.instance_id.clone()).unwrap_or_else(||uuid::Uuid::new_v4().to_string()));
            }
            for instance in targets {
                record.instances.retain(|a|a.instance_id!=instance);
                record.instances.push(Assignment{definition:publication.definition(&instance),instance_id:instance,
                    companion_id:companion.clone(),publication_hash:publication.publication_hash.clone(),ownership:None});
            }
        }
        ensure(record.instances.len()<=MAX_INSTANCES,"Companion instance budget exhausted.")?;
        let changed=inner.record.instances.iter().chain(&record.instances).filter(|a|{
            inner.record.instances.iter().find(|old|old.instance_id==a.instance_id)
                !=record.instances.iter().find(|new|new.instance_id==a.instance_id)
        }).map(|a|a.instance_id.clone()).collect::<BTreeSet<_>>();
        let old_publication=inner.publications.get(&publication.publication_hash).cloned();
        let hash=publication.publication_hash.clone();let panel_id=publication.ui_id.clone();let old_heads=inner.saved_heads.clone();
        let mut result=Self::retain_publication(inner,publication)?;
        inner.saved_heads.insert(panel_id,hash.clone());
        // Source callbacks never run inside a database transaction. Failed Save
        // preserves durable authority; affected live inputs remain disarmed.
        let sessions=inner.sessions.iter().filter(|(_,s)|changed.contains(&s.token.source_id)).map(|(id,_)|id.clone()).collect::<Vec<_>>();
        for session in sessions {self.disarm(inner,&session,"saved-panel-changed",true);}
        let commit=(||{
            for instance in &changed {
                match record.instances.iter().find(|a|a.instance_id==*instance){
                    Some(a)=>self.sink.register(a.definition.clone())?,None=>self.sink.unregister(instance)?,
                }
            }
            self.persist_content(&record,inner)
        })();
        if let Err(error)=commit {
            match old_publication{Some(old)=>{inner.publications.insert(hash,old);},None=>{inner.publications.remove(&hash);}}
            inner.saved_heads=old_heads;
            let mut failures=Vec::new();
            for instance in &changed {
                let restored=match inner.record.instances.iter().find(|a|a.instance_id==*instance){
                    Some(a)=>self.sink.register(a.definition.clone()),None=>self.sink.unregister(instance),
                };
                if let Err(error)=restored {failures.push(error);}
            }
            if !failures.is_empty(){inner.diagnostic=Some(format!("Saved panel authority was preserved; descriptor restoration needs attention: {}",failures.join("; ")));}
            return Err(format!("Panel selection was not committed; changed inputs remain disarmed: {error}"));
        }
        inner.record=record;
        for instance in changed {inner.feedback.remove(&instance);}
        result["panelSaved"]=serde_json::to_value(Self::saved_panels_inner(inner).into_iter().find(|panel|panel.publication_hash==hash).ok_or("Saved panel disappeared.")?).map_err(|_|"Cannot encode saved panel.")?;
        result["selectionCommitted"]=json!(true);Ok(result)
    }
}
