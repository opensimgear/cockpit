//! Saving a panel advances its stable identity and refreshes exact profile
//! references. Active output changes use the ordinary journaled takeover.
use super::*;
use crate::profile_session::{Candidate, State};

impl GatewayManager {
    pub(super) fn refresh_saved_panel_references(&self) -> Result<(),String> {
        let has_references={
            let profiles=self.profiles.lock().unwrap_or_else(|p|p.into_inner());
            if profiles.compile_source(osg_profiles::DEFAULT_PROFILE_ID)?.2{return Ok(());}
            profiles.companion_references()?["hashes"].as_array().is_some_and(|hashes|!hashes.is_empty())
        };
        if !has_references { return Ok(()); }
        let updates=self.companion_host.saved_panel_references()?;
        let mut profiles=self.profiles.lock().unwrap_or_else(|p|p.into_inner());
        for saved in updates {
            profiles.refresh_companion_panel(&saved.panel_id,&saved.publication_hash,&saved.previous_hashes.into_iter().collect())?;
        }
        Ok(())
    }
    pub(super) fn normalize_editor_panels(&self, request:&Value)->Result<Value,String> {
        let mut request=request.clone();
        let Some(assignments)=request["document"]["companionAssignments"].as_array_mut() else { return Ok(request); };
        if assignments.is_empty(){return Ok(request);}
        let updates=self.companion_host.saved_panel_references()?;
        for assignment in assignments {
            let current=updates.iter().find(|saved|
                assignment["panelId"].as_str().map(|panel|panel==saved.panel_id)
                    .unwrap_or_else(||assignment["publicationHash"].as_str().is_some_and(|hash|saved.previous_hashes.iter().any(|previous|previous==hash))));
            if let Some(saved)=current {
                assignment["panelId"]=json!(saved.panel_id);
                assignment["publicationHash"]=json!(saved.publication_hash);
            }
        }
        Ok(request)
    }
    pub(super) fn apply_saved_panel(&self,state:&mut Value,arrival:Option<u64>) {
        let result=(||->Result<bool,String>{
            let saved:crate::companion_panels::SavedPanel=serde_json::from_value(state["panelSaved"].clone())
                .map_err(|_|"The committed panel identity is missing or invalid.".to_owned())?;
            let (owner,mut candidate,apply_epoch,_pending,_policy)={
                let _admission=self.profile_admission.write().unwrap_or_else(|p|p.into_inner());
                if self.closed.load(Ordering::Acquire)||arrival!=Some(self.profile_admission_epoch.load(Ordering::Acquire)) {
                    return Err("Applying the saved panel was cancelled by a newer profile control.".into());
                }
                let status=self.profile_session.lock().unwrap_or_else(|p|p.into_inner()).status().clone();
                let mut profiles=self.profiles.lock().unwrap_or_else(|p|p.into_inner());
                profiles.refresh_companion_panel(&saved.panel_id,&saved.publication_hash,&saved.previous_hashes.iter().cloned().collect())?;
                let owner=status.owner.filter(|_|matches!(status.state,State::Running|State::Degraded));
                let candidate=owner.as_ref().map(|owner|{
                    let (document,assignments,read_only)=profiles.compile_source(&owner.profile_id)?;
                    // A passive catalogue read can already have advanced saved
                    // references; a failed earlier apply leaves them advanced
                    // too. Compare the durable revision to the actual owner,
                    // rather than relying on which reader changed it first.
                    let uses_panel=document.companion_assignments.iter().any(|assignment|
                        assignment.panel_id.as_deref().map(|id|id==saved.panel_id)
                            .unwrap_or(assignment.publication_hash==saved.publication_hash));
                    if !uses_panel||document.revision==owner.profile_revision {return Ok(None);}
                    Ok::<_,String>(Some(Candidate{document,assignments,read_only,game_id:owner.game_id.clone(),launch_preparation:false,admission_epoch:arrival,stage:None}))
                }).transpose()?.flatten();
                // Uploading/restoring an immutable payload is passive. Only a
                // real change to this active owner cancels older preparation.
                // Verify the captured arrival before allocating that new epoch
                // so a delayed Save can never overtake Stop or a newer Apply.
                let (apply_epoch,pending,policy)=if candidate.is_some(){
                    self.profile_pending.fetch_add(1,Ordering::AcqRel);
                    (Some(self.invalidate_learning()),Some(PendingProfileApply(&self.profile_pending)),self.profile_automatic.begin_control("profile_editor_save"))
                }else{(arrival,None,None)};
                (owner,candidate,apply_epoch,pending,policy)
            };
            if let Some(candidate)=candidate.as_mut(){candidate.admission_epoch=apply_epoch;}
            // Direct selections already committed with the publication/head.
            // Profile-owned effects continue through the journaled takeover.
            match (owner,candidate) {
                (Some(owner),Some(candidate))=>{self.apply_saved_editor(candidate,&owner,apply_epoch)?;Ok(true)},
                _=>Ok(false),
            }
        })();
        state["panelSaveApplication"]=match result {
            Ok(true)=>json!({"state":"applied","error":null}),
            Ok(false)=>json!({"state":"inactive","error":null}),
            Err(error)=>json!({"state":"failed","error":format!("Panel saved, but the active profile could not use its latest view: {error}. Check the profile's selected controls and connections before activating it again.")}),
        };
    }
}
