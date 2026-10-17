//! Native-only, bounded gateway publication ownership. No owner enters UI state.
use super::*;

pub(crate) fn capability() -> Value {
    json!({"schema":1,"scope":"gateway-can","nonpersistent":true,"captureAtClaim":true,
        "restore":"compare-if-unchanged","minLeaseMs":250,"maxLeaseMs":5000,
        "maxTargets":32,"reconnectRequired":true})
}
fn mask(value: &Value) -> Option<u32> {
    let text=value.as_str()?;
    (text.len()==8 && text.bytes().all(|c|c.is_ascii_digit() || (b'a'..=b'f').contains(&c)))
        .then(||u32::from_str_radix(text,16).ok()).flatten()
}
fn exact(value:&Value, keys:&[&str])->bool {
    value.as_object().is_some_and(|object|object.len()==keys.len() && keys.iter().all(|key|object.contains_key(*key)))
}
/// Unknown capabilities remain unsupported; a recognized envelope must be valid.
pub(super) fn metadata(wire:&Value)->Result<Option<Value>,String> {
    if wire["controllerPublicationOverride"] != capability() {return Ok(None);}
    let bad=||"Malformed native controller publication ownership metadata.".to_owned();
    if wire["protocol"]!=3 || wire["canProtocol"]!=3 || wire["maxModules"]!=32 {return Err(bad());}
    for key in ["publicationRevision","configuredGeneration","effectiveGeneration"] {
        if number(&wire[key],u32::MAX as u64).is_none(){return Err(bad());}
    }
    let state=&wire["publication"];
    if !exact(state,&["phase","outcome","leaseId","expiresAtMs","transition","claimRevision","targetMask","enabledMask","expectedReconnect"])
        || !matches!(state["phase"].as_str(),Some("idle"|"owned"|"restoring"|"attention"))
        || !matches!(state["outcome"].as_str(),Some("ok"|"no_change"|"restore"|"preserved"|"unverified"|"exhausted"))
        || !state["expectedReconnect"].is_boolean() {return Err(bad());}
    for key in ["leaseId","expiresAtMs","claimRevision"] {if number(&state[key],u32::MAX as u64).is_none(){return Err(bad());}}
    let transition=state["transition"].as_str().filter(|text|!text.is_empty() && text.len()<=20 && text.bytes().all(|c|c.is_ascii_digit()) && (*text=="0" || !text.starts_with('0')))
        .and_then(|text|text.parse::<u64>().ok()).ok_or_else(bad)?;
    let targets=mask(&state["targetMask"]).ok_or_else(bad)?;
    let enabled=mask(&state["enabledMask"]).ok_or_else(bad)?;
    if enabled&!targets!=0 || (state["phase"]!="idle" && (targets==0 || state["leaseId"]==0 || transition==0 || state["claimRevision"]==0)) {return Err(bad());}
    let mut slots=HashSet::new();
    let mut report_ids=HashSet::new();
    let mut orders=HashSet::new();
    for module in wire["modules"].as_array().ok_or_else(bad)? {
        let slot=module["publicationSlot"].as_i64().filter(|v|(-1..32).contains(v)).ok_or_else(bad)?;
        let report=number(&module["publicationReportId"],255).ok_or_else(bad)?;
        let order=module["configuredDescriptorIndex"].as_i64().filter(|v|(-1..32).contains(v)).ok_or_else(bad)?;
        let configured=module["configuredEnabled"].as_bool().ok_or_else(bad)?;
        if (slot>=0 && !slots.insert(slot)) || (report>0 && !report_ids.insert(report))
            || configured!=(order>=0) || (order>=0 && !orders.insert(order))
            || (slot<0 && (configured || report>0)) || (configured && report==0) {return Err(bad());}
        if slot>=0 && targets&(1u32<<slot)!=0 && state["phase"]=="owned"
            && (module["directUsb"]!=false || module["online"]!=true) {return Err(bad());}
    }
    Ok(Some(state.clone()))
}
pub(super) fn permits_overlay(state:&Value,module:&Value)->bool {
    let Some(slot)=module["publicationSlot"].as_u64().filter(|v|*v<32) else{return false};
    state["phase"]!="idle" && mask(&state["targetMask"]).is_some_and(|targets|targets&(1u32<<slot)!=0)
}
pub(super) fn deliverable(state:&Value,module:&Value,layout_pending:bool)->bool {
    if !permits_overlay(state,module){return true;}
    state["phase"]=="owned" && !layout_pending && module["published"]==true
        && mask(&state["enabledMask"]).is_some_and(|enabled|enabled&(1u32<<module["publicationSlot"].as_u64().unwrap())!=0)
}

#[derive(Clone,Debug)]
pub(crate) struct PublicationRequest {
    pub epoch:u32,
    pub operation:PublicationOperation,
    /// Claim/renew share the run's revocation latch, checked by the serial owner.
    pub authority:Option<Arc<AtomicBool>>,
    pub control_epoch:Option<(Arc<AtomicU64>,u64)>,
}
#[derive(Clone,Debug)]
pub(crate) enum PublicationOperation {
    State,
    Inspect{owner:String,lease:u32},
    Claim{owner:String,revision:u32,target_mask:u32,enabled_mask:u32,ttl_ms:u32},
    Renew{owner:String,lease:u32,ttl_ms:u32},
    Release{owner:String,lease:u32},
}
#[derive(Clone,Debug)]
pub(crate) struct PublicationReply {pub state:Value,pub owned:Option<bool>}
impl PublicationRequest {
    pub(crate) fn mutating(&self)->bool {matches!(self.operation,PublicationOperation::Claim{..}|PublicationOperation::Renew{..}|PublicationOperation::Release{..})}
    fn command(&self)->Result<String,String> {
        let bad=||"Invalid bounded native publication request.".to_owned();
        if self.epoch==0{return Err(bad());}
        let owner=match &self.operation {PublicationOperation::State=>None,PublicationOperation::Inspect{owner,..}|PublicationOperation::Claim{owner,..}|PublicationOperation::Renew{owner,..}|PublicationOperation::Release{owner,..}=>Some(owner)};
        if owner.is_some_and(|owner|!hex_identity(owner,32)){return Err(bad());}
        let text=match &self.operation {
            PublicationOperation::State=>"publication state".into(),
            PublicationOperation::Inspect{owner,lease} if *lease>0=>format!("publication inspect {owner} {lease}"),
            PublicationOperation::Release{owner,lease} if *lease>0=>format!("publication release {owner} {lease}"),
            PublicationOperation::Renew{owner,lease,ttl_ms} if *lease>0 && (250..=5000).contains(ttl_ms)=>format!("publication renew {owner} {lease} {ttl_ms}"),
            PublicationOperation::Claim{owner,revision,target_mask,enabled_mask,ttl_ms} if *revision>0 && *target_mask>0 && enabled_mask&!target_mask==0 && (250..=5000).contains(ttl_ms)=>format!("publication claim {owner} {revision} {target_mask:08x} {enabled_mask:08x} {ttl_ms}"),
            _=>return Err(bad()),
        };
        if matches!(self.operation,PublicationOperation::Claim{..}|PublicationOperation::Renew{..})
            && (self.authority.as_ref().is_none_or(|live|!live.load(Ordering::Acquire))
                || self.control_epoch.as_ref().is_none_or(|(epoch,expected)|epoch.load(Ordering::Acquire)!=*expected)) {return Err("The native publication run was revoked before dispatch.".into());}
        Ok(text)
    }
}
impl Gateway {
    pub(super) fn publication(&mut self,request:PublicationRequest)->Result<PublicationReply,String> {
        let command=request.command()?;
        if self.cancel.load(Ordering::Acquire){return Err(CLOSED.into());}
        if self.role!=Role::Can {return Err("This gateway has no native controller publication capability.".into());}
        if self.connection.is_none(){self.recover_automatic(Instant::now()+Duration::from_millis(1500))?;}
        if self.state["epoch"].as_u64()!=Some(request.epoch as u64) || self.state["controllerPublicationOverride"]!=capability(){return Err("The native publication capability or gateway boot changed.".into());}
        self.exchange_guarded(&command,self.timing.command,request.authority.as_deref(),request.control_epoch.as_ref().map(|(epoch,expected)|(epoch.as_ref(),*expected)))?;
        if self.state["epoch"].as_u64()!=Some(request.epoch as u64){return Err("The gateway restarted during publication dispatch.".into());}
        let owned=if matches!(request.operation,PublicationOperation::Inspect{..}) {
            Some(self.publication_reply.ok_or("The gateway returned no request-scoped publication ownership proof.")?)
        }else{None};
        Ok(PublicationReply{state:self.decorated(None),owned})
    }
}
impl Stm32Service {
    pub(crate) fn publication(&self,request:PublicationRequest)->Result<PublicationReply,String> {
        request.command()?;
        if self.closed.load(Ordering::Acquire){return Err(CLOSED.into());}
        let (sender,receiver)=mpsc::channel();
        self.sender.try_send(Message::Publication(request,sender)).map_err(|error|match error{mpsc::TrySendError::Full(_)=>"The native publication worker queue is full; no request was sent.".to_owned(),mpsc::TrySendError::Disconnected(_)=>CLOSED.to_owned()})?;
        receiver.recv().map_err(|_|CLOSED.to_owned())?
    }
}

#[cfg(test)]
#[path="stm32_publication_tests.rs"]
mod tests;
