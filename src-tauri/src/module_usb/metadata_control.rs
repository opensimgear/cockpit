//! Source metadata admission and command receipt correlation. No periodic I/O.
use super::*;
use crate::module_console::actor::{Input,Sink};
use crate::transport::duplex::CancelIo;

impl Worker {
    pub(super) fn configuration_revision(config:&Value)->Result<u32,String>{
        config["revision"].as_u64().and_then(|v|u32::try_from(v).ok())
            .filter(|v|*v>0).ok_or_else(||"USB configuration lacks a valid committed revision.".into())
    }
    pub(super) fn metadata_sink(&self,events:Sink)->Sink{
        // This slot belongs only to this connection. An old RX callback cannot
        // address the slot installed for its replacement.
        let local=self.metadata.clone();
        Arc::new(move|input|{
            match input {
                Input::Line{line,received_at} if line.starts_with("@meta ")=>{
                    let capture=local.lock().unwrap_or_else(|p|p.into_inner()).clone();
                    if let Some(capture)=capture{capture.accept(Input::Line{line,received_at});}
                },
                Input::Lost(error)=>{
                    let capture=local.lock().unwrap_or_else(|p|p.into_inner()).clone();
                    if let Some(capture)=capture{capture.accept(Input::Lost(error.clone()));}
                    events(Input::Lost(error));
                },
                other=>events(other),
            }
        })
    }
    pub(super) fn start_metadata(&mut self,boot:u32,cancel:Arc<dyn CancelIo>)->Result<(),String>{
        let valid=|reply:Reply,streaming:bool|matches!(reply,Reply::Json(v) if v==json!({"ok":true,"streaming":streaming}));
        if !valid(self.exchange("metadata unsubscribe",ReplyKind::Json)?,false){
            return Err("The module rejected metadata stream teardown.".into());
        }
        let capture=Arc::new(metadata_binding::Capture::new(boot,self.wake.clone(),cancel));
        *self.metadata.lock().unwrap_or_else(|p|p.into_inner())=Some(capture.clone());
        self.metadata_observed=0;
        if !valid(self.exchange("metadata subscribe",ReplyKind::Json)?,true){
            return Err("The module rejected committed metadata streaming.".into());
        }
        // Only the initial baseline has a deadline. A quiet committed source
        // needs no keepalive query and has no arbitrary inactivity expiry.
        let deadline=Instant::now()+self.timing.command;
        loop {
            let observed=self.wake.observed();
            self.metadata_updates()?;
            if capture.snapshot().latest.is_some(){return Ok(());}
            if self.cancel.load(Ordering::Acquire){return Err(CLOSED.into());}
            let Some(remaining)=deadline.checked_duration_since(Instant::now())else{
                return Err("USB metadata stream did not supply its initial baseline.".into());
            };
            self.wake.wait_scheduled(observed,Some(remaining));
        }
    }
    pub(super) fn metadata_updates(&mut self)->Result<bool,String>{
        let capture=self.metadata.lock().unwrap_or_else(|p|p.into_inner()).clone();
        let Some(capture)=capture else{return Ok(false);};
        let Some(update)=capture.take_changed(self.metadata_observed)else{return Ok(false);};
        self.metadata_observed=update.revision;
        if let Some(error)=update.error {self.close();return Err(error);}
        let Some(frame)=update.latest else{return Ok(false);};
        if frame.revision<self.config_revision{return Ok(false);}
        if frame.revision==self.config_revision {
            let config=self.snapshot.config.as_ref().ok_or("USB metadata has no active configuration baseline.")?;
            let state=&frame.state;
            if config["generation"]!=json!(state.generation) || config["savedGeneration"]!=json!(state.saved_generation)
                || config["owner"]!=json!(state.owner) || config["claimExpiresMs"]!=json!(state.claim_expires_ms)
                || config["dirty"]!=json!(state.dirty) || config["pendingApply"]!=json!(state.pending_apply) {
                self.close();return Err("USB metadata disagrees with its committed command revision.".into());
            }
            return Ok(false);
        }
        let Some(route)=self.snapshot.route.as_ref()else{return Err(STALE.into());};
        if frame.boot!=route.boot || frame.state.generation!=route.configuration_generation {
            self.close();return Err(STALE.into());
        }
        // An unseen revision may coalesce expiry + reacquisition by the same
        // numeric owner. Observed source ownership never grants private access.
        self.writer=None;self.snapshot.claimed_by=None;self.snapshot.draft=None;
        self.config_revision=frame.revision;
        if let Some(config)=&mut self.snapshot.config {
            config["revision"]=json!(frame.revision);
            config["revisionExhausted"]=json!(false);
            config["generation"]=json!(frame.state.generation);
            config["savedGeneration"]=json!(frame.state.saved_generation);
            config["owner"]=json!(frame.state.owner);
            config["claimExpiresMs"]=json!(frame.state.claim_expires_ms);
            config["dirty"]=json!(frame.state.dirty);
            config["pendingApply"]=json!(frame.state.pending_apply);
        }
        self.snapshot.observed_at_ms=Some(SystemTime::now().duration_since(UNIX_EPOCH)
            .unwrap_or_default().as_millis().min(u64::MAX as u128)as u64);
        Ok(true)
    }
}
