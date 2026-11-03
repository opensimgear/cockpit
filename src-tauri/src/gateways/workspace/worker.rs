use super::*;
use crate::transport::notifications::PortNotifications;
use std::time::Instant;

pub(super) fn run(inner:Arc<Inner>,manager:Weak<GatewayManager>,notifications:bool){
    let mut view=publisher::View::new();
    let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||run_loop(&inner,&manager,notifications,&mut view)))
        .unwrap_or_else(|_|Err("Workspace owner panicked.".into()));
    if let Some(manager)=manager.upgrade(){view.retire(&manager);}
    inner.closed.store(true,Ordering::Release);
    let mut control=inner.control.lock().unwrap_or_else(|p|p.into_inner());
    if control.ready.is_none()||result.is_err(){control.ready=Some(result);}
    inner.changed.notify_all();
}

fn run_loop(inner:&Arc<Inner>,manager:&Weak<GatewayManager>,notifications:bool,view:&mut publisher::View)->Result<(),String>{
    let ports_wake=Arc::new(osg_streams::NativeWake::default());
    let _=inner.wake.bind_thread(std::thread::current());let _=ports_wake.bind_thread(std::thread::current());
    let mut notification_error=String::new();
    let mut source=if notifications {match PortNotifications::start(ports_wake.clone()){
        Ok(source)=>Some(source),Err(error)=>{notification_error=error;None}
    }}else{None};
    let mut seen_ports=ports_wake.observed();
    let mut entries=BTreeMap::<String,Entry>::new();
    let mut discovery_error=String::new();
    let mut token=0u64;
    let mut pending_pauses=BTreeMap::new();
    loop {
        if inner.closed.load(Ordering::Acquire){break;}
        let Some(manager)=manager.upgrade() else{break;};
        if manager.closed.load(Ordering::Acquire){break;}
        let current_ports=ports_wake.observed();
        if inner.ports_dirty.swap(false,Ordering::AcqRel)||current_ports!=seen_ports {
            seen_ports=current_ports;
            discovery_error=manager.discover().err().unwrap_or_default();
        }
        if let Some(error)=source.as_ref().and_then(PortNotifications::error){notification_error=error;}
        let ports=manager.inventory.lock().unwrap_or_else(|p|p.into_inner()).clone();
        synchronize(inner,&manager,&mut entries,&ports)?;
        let (paused,completed)={let mut control=inner.control.lock().unwrap_or_else(|p|p.into_inner());
            (std::mem::take(&mut control.paused),control.completed.drain(..).collect::<Vec<_>>())};
        for (id,pause) in paused {
            if pending_pauses.len()==MAX_GATEWAYS&&!pending_pauses.contains_key(&id){return Err("Workspace pending intent limit reached.".into());}
            pending_pauses.insert(id,pause);
        }
        // Unknown identities retain intent until discovery. Applying an intent
        // invalidates the public row even if no adapter cache changed.
        for id in pending_pauses.keys().cloned().collect::<Vec<_>>() {
            if let Some(entry)=entries.get_mut(&id){
                entry.paused=pending_pauses.remove(&id).unwrap();entry.reset();entry.touch()?;
            }
        }
        for completion in completed {if let Some(entry)=entries.get_mut(&completion.id){entry.complete(&completion)?;}}
        for entry in entries.values(){if let Some(adapter)=&entry.adapter {adapter.set_workspace_owned(!entry.paused);}}
        for entry in entries.values_mut(){
            if let Some(action)=entry.plan(Instant::now()){
                let dispatch={let mut control=inner.control.lock().unwrap_or_else(|p|p.into_inner());
                    if control.active>=MAX_JOBS||control.held.contains_key(&entry.id) {None}else{
                        let fence=command_fence(&mut control,&entry.id)?;
                        let epoch=fence.epoch.load(Ordering::Acquire);control.active+=1;Some((fence,epoch))}};
                let Some(dispatch)=dispatch else{continue;};
                token=token.checked_add(1).ok_or("Workspace task identity exhausted.")?;
                entry.job=Some((token,entry.session.clone()));entry.job_source=Some(entry.state.clone());entry.touch()?;
                spawn(inner,Arc::downgrade(&manager),entry,action,token,dispatch);
            }
        }
        let error=[discovery_error.as_str(),notification_error.as_str()].into_iter().filter(|s|!s.is_empty()).collect::<Vec<_>>().join("; ");
        // Projection failure never cancels native connection ownership/routing.
        match view.publish(inner,&manager,&entries,&ports,&error){
            Ok(())=>{let mut control=inner.control.lock().unwrap_or_else(|p|p.into_inner());if control.ready.is_none(){control.ready=Some(Ok(()));inner.changed.notify_all();}},
            Err(error)=>{view.retire(&manager);let mut control=inner.control.lock().unwrap_or_else(|p|p.into_inner());if control.ready.is_none(){control.ready=Some(Err(error));inner.changed.notify_all();}},
        }
        let deadline={let control=inner.control.lock().unwrap_or_else(|p|p.into_inner());
            entries.values().filter(|entry|!control.held.contains_key(&entry.id)).filter_map(Entry::deadline).min()};
        drop(manager);
        if let Some(at)=deadline {std::thread::park_timeout(at.saturating_duration_since(Instant::now()));}
        else{std::thread::park();}
    }
    if let Some(source)=source.as_mut(){source.shutdown()?;}
    Ok(())
}

fn synchronize(inner:&Inner,manager:&GatewayManager,entries:&mut BTreeMap<String,Entry>,ports:&[Value])->Result<(),String>{
    if ports.len()>MAX_GATEWAYS{return Err("Workspace port limit exceeded.".into());}
    let ids=ports.iter().filter_map(|p|p["gatewayId"].as_str()).collect::<std::collections::BTreeSet<_>>();
    for port in ports {
        let id=port["gatewayId"].as_str().ok_or("Workspace port lacks its registered gateway identity.")?;
        if !entries.contains_key(id){
            if entries.len()==MAX_GATEWAYS{return Err("Workspace gateway limit exceeded.".into());}
            entries.insert(id.into(),Entry::new(id.into(),port.clone()));
        }
        entries.get_mut(id).unwrap().port(port)?;
    }
    for entry in entries.values_mut(){if entry.present&&!ids.contains(entry.id.as_str()){entry.present=false;entry.touch()?;}}
    let sessions=manager.sessions.lock().unwrap_or_else(|p|p.into_inner()).iter().map(|(id,adapter)|(id.clone(),adapter.clone())).collect::<Vec<_>>();
    let session_ids=sessions.iter().map(|(id,_)|id.as_str()).collect::<std::collections::BTreeSet<_>>();
    for entry in entries.values_mut(){
        if entry.adapter.is_some()&&!session_ids.contains(entry.id.as_str()){
            entry.adapter.as_ref().unwrap().set_workspace_owned(false);
            entry.adapter=None;entry.state=Arc::new(Value::Null);entry.owners.clear();
            entry.session=uuid::Uuid::new_v4().to_string();entry.reset();entry.touch()?;
        }
    }
    for (id,adapter) in sessions {
        adapter.watch_workspace(inner.wake.clone());
        let (mut state,mut owners)=adapter.workspace_snapshot();
        if let Some(entry)=entries.get_mut(&id){
            let invalid=state["gatewaySerial"].as_str().is_some_and(|serial|super::super::canonical_id(id.split_once(':').unwrap().0,serial).as_deref()!=Some(id.as_str()))
                ||state["connected"]==true&&state["gatewaySerial"].is_null();
            if invalid {state=if entry.state.is_null(){entry.state.clone()}else{Arc::new(Value::Null)};owners.clear();}
            entry.accept(adapter,state,owners)?;
            if invalid&&entry.error.is_empty(){entry.error="Waiting for a gateway cache with the verified native identity.".into();entry.touch()?;}
        }
    }
    entries.retain(|_,entry|entry.present||entry.adapter.is_some()||entry.job.is_some());
    Ok(())
}

fn spawn(inner:&Arc<Inner>,manager:Weak<GatewayManager>,entry:&Entry,action:Action,token:u64,dispatch:(Arc<CommandFence>,u64)){
    let id=entry.id.clone();let session=entry.session.clone();let request=action.request(&id,&entry.port);
    let shared=inner.clone();let fallback_id=id.clone();let fallback_session=session.clone();let fallback_action=action.clone();
    let result=std::thread::Builder::new().name("gateway-workspace-command".into()).spawn(move||{
        let manager=manager.upgrade();
        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{
            if shared.closed.load(Ordering::Acquire){return Some("Workspace is closed.".into());}
            let Some(manager)=manager.as_ref()else{return Some("Workspace manager stopped.".into());};
            let response=match dispatch.0.dispatch(dispatch.1,||manager.request(request)){Ok(response)=>response,Err(error)=>return Some(error)};
            (response["ok"]!=true).then(||response["error"].as_str().unwrap_or("Gateway operation failed.").to_owned())
        })).unwrap_or_else(|_|Some("Workspace operation panicked.".into()));
        complete(&shared,Completion{id,session,token,action,error:result});
        drop(manager);
    });
    if let Err(error)=result{complete(inner,Completion{id:fallback_id,session:fallback_session,token,action:fallback_action,error:Some(error.to_string())});}
}
fn complete(inner:&Inner,completion:Completion){
    let mut control=inner.control.lock().unwrap_or_else(|p|p.into_inner());
    control.active=control.active.saturating_sub(1);control.completed.push_back(completion);drop(control);inner.wake.notify();
}
