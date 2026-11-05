//! Native activity collection. Only this background worker writes captured events.
mod audit;
pub(crate) mod changes;
pub(crate) mod context;
mod diagnostics;
mod messages;
mod capture;
mod writer;
pub(crate) mod metadata;
mod revision;
mod session;
mod inventory;
mod effects;
mod observations;
mod reconcile;
mod scopes;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod once_tests;
#[cfg(test)]
mod wake_tests;
#[cfg(test)]
mod writer_wake_tests;
pub(crate) use capture::Snapshot;
use osg_persistence::{Database, NewActivityEvent};
use serde_json::{json, Value};
use std::{path::Path, sync::{mpsc, Arc, Mutex}, thread::JoinHandle, time::{Duration, SystemTime, UNIX_EPOCH}};

type Observe = Box<dyn Fn() -> Snapshot + Send>;
enum Message { Event(NewActivityEvent), Observe(metadata::Source), Flush(mpsc::Sender<Result<(),String>>), Stop }
struct MainSender { sender: mpsc::SyncSender<Message>, wake: Arc<osg_streams::NativeWake> }
impl MainSender {
    fn send(&self,message:Message)->Result<(),mpsc::SendError<Message>> {let result=self.sender.send(message);if result.is_ok(){self.wake.notify();}result}
}
pub(crate) struct Action {
    key:String,
    registry:Arc<Mutex<reconcile::Reconciler>>,
    identity:Option<(String,String)>,
    wake:Arc<osg_streams::NativeWake>,
}
impl Drop for Action {fn drop(&mut self){self.registry.lock().unwrap_or_else(|p|p.into_inner()).abandon(&self.key);self.wake.notify();}}
pub(crate) struct Runtime {
    database: Result<Database, String>,
    sender: MainSender,
    worker: Mutex<Option<JoinHandle<()>>>,
    lifecycle: Arc<Mutex<session::Observer>>,
    started: std::sync::atomic::AtomicBool,
    metadata: Mutex<()>,
    registry:Arc<Mutex<reconcile::Reconciler>>,
    revision: Arc<revision::Revision>,
    wake: Arc<osg_streams::NativeWake>,
    metadata_wakes: metadata::Signals,
}
pub(crate) fn event(category: &str, kind: &str, level: &str, message: String, id: Option<String>, name: Option<String>) -> NewActivityEvent {
    NewActivityEvent { event_key:uuid::Uuid::new_v4().to_string(),occurred_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().min(i64::MAX as u128) as i64,
        category: category.into(), event_type: kind.into(), level: level.into(), message, detail: String::new(), entity_id: id, entity_name: name, source: "native".into() }
}
impl Runtime {
    pub(crate) fn new(anchor: &Path) -> Self {
        let database = Database::for_namespace(anchor);
        let storage = database.clone();
        let (sender, receiver) = mpsc::sync_channel(2048);
        // Lifecycle transitions cannot wait behind metadata collection while the
        // coordinator is locked. This channel receives changes only, never ticks.
        let (lifecycle_sender,lifecycle_receiver)=mpsc::channel();
        let registry=Arc::new(Mutex::new(reconcile::Reconciler::default()));
        let wake=Arc::new(osg_streams::NativeWake::default());
        let lifecycle=Arc::new(Mutex::new(session::Observer::new(lifecycle_sender,registry.clone(),wake.clone())));
        let writer_registry=registry.clone();
        let revision = Arc::new(revision::Revision::default());
        let writer_revision = revision.clone();
        let writer_wake=wake.clone();
        let worker = std::thread::Builder::new().name("cockpit-activity".into()).spawn(move || writer::run(storage,receiver,lifecycle_receiver,writer_registry,&writer_revision,writer_wake)).expect("start native activity worker");
        let metadata_wakes = metadata::Signals::new(wake.clone());
        Self { database, sender:MainSender{sender,wake:wake.clone()}, worker: Mutex::new(Some(worker)), lifecycle,started:std::sync::atomic::AtomicBool::new(false),metadata:Mutex::new(()),registry,revision,wake,metadata_wakes }
    }
    pub(crate) fn observe_revision(&self, observations: &crate::observations::Runtime) -> Result<(), String> {
        self.database.as_ref().map_err(Clone::clone)?;
        self.revision.observe(observations)
    }
    pub(crate) fn metadata_wake(&self, group: metadata::Group) -> Arc<osg_streams::NativeWake> {
        self.metadata_wakes.get(group).clone()
    }
    pub(crate) fn notify_metadata(&self, group: metadata::Group) { self.metadata_wakes.get(group).notify(); }
    pub(crate) fn observe_group(&self, group: metadata::Group, source: impl Fn() -> Snapshot + Send + 'static) {
        self.register_source(metadata::Source::new(Some(group), self.metadata_wake(group), Box::new(source)));
    }
    #[cfg(test)]
    pub(crate) fn observe(&self, source: impl Fn() -> Snapshot + Send + 'static) {
        self.register_source(metadata::Source::new(None, Arc::new(osg_streams::NativeWake::child(self.wake.clone())), Box::new(source)));
    }
    fn register_source(&self, source: metadata::Source) {
        if !self.started.swap(true,std::sync::atomic::Ordering::AcqRel) {let _=self.sender.send(Message::Event(event("system","cockpit_started","info","Cockpit native services started.".into(),None,Some("Cockpit".into()))));}
        let _ = self.sender.send(Message::Observe(source));
    }
    #[cfg(test)]
    pub(crate) fn audit(&self, request: &Value, response: &Value) -> bool {
        self.audit_with_context(request, response, None)
    }
    pub(crate) fn audit_with_context(&self, request: &Value, response: &Value, context: Option<&context::Context>) -> bool {
        let action=self.begin(request);
        self.audit_action(request,response,context,action.as_ref())
    }
    pub(crate) fn begin(&self,request:&Value)->Option<Action> {
        audit::category(request["command"].as_str()?)?;
        let key=uuid::Uuid::new_v4().to_string();
        self.registry.lock().unwrap_or_else(|p|p.into_inner()).begin(key.clone(),scopes::Scope::request(request));
        let identity=self.lifecycle.try_lock().ok().and_then(|observer|observer.identity());
        Some(Action{key,registry:self.registry.clone(),identity,wake:self.wake.clone()})
    }
    pub(crate) fn audit_action(&self,request:&Value,response:&Value,context:Option<&context::Context>,action:Option<&Action>)->bool {
        if let Some(mut event)=audit::mutation_with_context(request,response,context) {
            audit::terminal(&mut event,request,response,action.and_then(|action|action.identity.as_ref()));
            let expected=effects::expected(request,response,context);
            if event.entity_id.is_none()&&expected.len()==1 {
                if let Some((kind,id))=expected.keys().next().and_then(|subject|subject.split_once(':')) {
                    if !matches!(kind,"profile_session"|"discovery"){event.entity_id=Some(id.into());}
                }
            }
            if let Some(action)=action {
                event.event_key=action.key.clone();
                self.registry.lock().unwrap_or_else(|p|p.into_inner()).finish(&action.key,expected);
            }
            let _=self.sender.send(Message::Event(event));true
        } else {false}
    }
    pub(crate) fn session_observer(&self) -> Arc<Mutex<session::Observer>> { self.lifecycle.clone() }
    pub(crate) fn mutation_guard(&self)->std::sync::MutexGuard<'_,()> {self.metadata.lock().unwrap_or_else(|p|p.into_inner())}
    pub(crate) fn flush(&self) -> Result<(), String> {
        let (sender, receiver) = mpsc::channel();
        self.sender.send(Message::Flush(sender)).map_err(|_|"Activity writer has stopped.".to_owned())?;
        receiver.recv_timeout(Duration::from_secs(15)).map_err(|_|"Activity writer did not finish saving pending events.".to_owned())?
    }
    pub(crate) fn request(&self, request: &Value) -> Result<Value, String> {
        let database = self.database.as_ref().map_err(Clone::clone)?;
        match request["command"].as_str().unwrap_or("") {
            "activity_query" => {
                let filter = request.get("activityFilter").cloned().unwrap_or_else(|| json!({}));
                let query = serde_json::from_value::<osg_persistence::ActivityQuery>(filter).map_err(|_| "Invalid activity filters.".to_owned())?;
                match request.get("committedOnly") {
                    Some(Value::Bool(true)) => {}
                    None | Some(Value::Bool(false)) => self.flush()?,
                    _ => return Err("Invalid committed activity read mode.".into()),
                }
                Ok(json!({"activity": database.activity_query(&query)?}))
            }
            "activity_clear" => {
                self.flush()?;
                let deleted = database.activity_clear()?;
                if deleted > 0 { self.revision.changed(); }
                Ok(json!({"activityDeleted":deleted}))
            }
            "activity_settings_get" => Ok(json!({"activitySettings":database.activity_settings()?})),
            "activity_settings_save" => {
                let days = request["retentionDays"].as_u64().and_then(|value| u32::try_from(value).ok()).ok_or("Choose a retention period from 1 to 3650 days.")?;
                let settings = database.activity_settings_save(days)?;
                self.revision.changed();
                Ok(json!({"activitySettings":settings}))
            }
            _ => Err("Unknown activity command.".into()),
        }
    }
    pub(crate) fn shutdown(&self) {
        let mut worker = self.worker.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(worker) = worker.take() {
            if self.started.swap(false,std::sync::atomic::Ordering::AcqRel) {let _=self.sender.send(Message::Event(event("system","cockpit_stopped","info","Cockpit native services stopped.".into(),None,Some("Cockpit".into()))));}
            let _ = self.sender.send(Message::Stop); let _ = worker.join();
        }
    }
}
impl Drop for Runtime { fn drop(&mut self) { self.shutdown(); } }
