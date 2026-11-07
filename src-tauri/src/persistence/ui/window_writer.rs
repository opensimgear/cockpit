//! A bounded pending geometry, with no SQLite work in native window callbacks.
use super::*;
use crate::observations::Runtime;
use osg_streams::{Publisher, TopicSpec};
use std::{collections::BTreeSet,sync::{Condvar,PoisonError},time::{Duration,Instant}};

const TOPIC: &str = "settings.window-state";
const FLUSH_TIMEOUT: &str = "Window persistence did not finish before the flush deadline.";

struct Job {revision:u64,value:Value,attempt:u8}
#[derive(Default)]
struct Pending {
    job: Option<Job>,
    active: bool,
    closing: bool,
    error: Option<String>,
    restore_error: Option<String>,
    capture_error: Option<String>,
    flush_timed_out: bool,
    observation: Option<Observation>,
}

struct Observation {
    runtime: Runtime,
    publisher: Publisher,
    previous: Value,
}

impl Pending {
    fn diagnostic(&self) -> Option<&str> {
        self.error.as_deref()
            .or(self.capture_error.as_deref())
            .or(self.flush_timed_out.then_some(FLUSH_TIMEOUT))
            .or(self.restore_error.as_deref())
    }

    fn publish(&mut self) {
        let next = diagnostic_payload(self.diagnostic());
        if let Some(observation) = &mut self.observation {
            if observation.previous != next && observation.publisher
                .publish_latest_state(1, next.clone(), observation.runtime.captured_us(), TOPIC).is_ok()
            {
                observation.previous = next;
            }
        }
    }
}

fn diagnostic_payload(error: Option<&str>) -> Value {
    json!({"error":error.map(|message| message.chars().take(512).collect::<String>())})
}

pub(crate) struct WindowWriter {service:Preferences,shared:Arc<(Mutex<Pending>,Condvar)>}
impl WindowWriter {
    pub(crate) fn observe(service: Preferences, runtime: Runtime, restore_error: Option<String>) -> Result<Self, String> {
        let previous = diagnostic_payload(restore_error.as_deref());
        let publisher = runtime.broker().register(TopicSpec {
            topic: TOPIC.into(), source: "window-persistence".into(),
            audiences: vec!["ui".into(), "mcp".into()],
        }, previous.clone(), runtime.captured_us(), TOPIC.into()).map_err(|error| error.to_string())?;
        let observation = Some(Observation { runtime, publisher, previous });
        Ok(Self::start(service, Pending { restore_error, observation, ..Pending::default() }))
    }

    fn start(service: Preferences, state: Pending) -> Self {
        let shared=Arc::new((Mutex::new(state),Condvar::new()));let owner=shared.clone();let storage=service.clone();
        std::thread::spawn(move||{
            let(lock,wake)=&*owner;let mut own_revisions=BTreeSet::new();let mut last_owned=None;
            loop {
                let mut pending=lock.lock().unwrap_or_else(PoisonError::into_inner);
                while pending.job.is_none()&&!pending.closing{pending=wake.wait(pending).unwrap_or_else(PoisonError::into_inner);}
                let Some(mut job)=pending.job.take()else{break};pending.active=true;drop(pending);
                if let Ok(current)=storage.cached_entry("windows"){
                    if Some(current.revision)==last_owned&&own_revisions.contains(&job.revision){job.revision=current.revision;}
                }
                let result=storage.write("windows",job.revision,job.value.clone());
                // Reconcile a foreign edit for the next actual gesture. Never
                // retry this old observation against the newer revision.
                if result.as_ref().is_err_and(|error|error.contains("revision conflict")){let _=storage.read("windows");}
                let mut pending=lock.lock().unwrap_or_else(PoisonError::into_inner);pending.active=false;
                pending.flush_timed_out = false;
                let retry = match result {
                    Ok(entry)=>{if !own_revisions.contains(&job.revision){own_revisions.clear();}own_revisions.insert(job.revision);own_revisions.insert(entry.revision);while own_revisions.len()>32{own_revisions.pop_first();}last_owned=Some(entry.revision);pending.error=None;pending.restore_error=None;false},
                    Err(error)=>{let retry=error.contains("busy")&&job.attempt<7&&!pending.closing&&pending.job.is_none();pending.error=Some(error);retry}
                };
                pending.publish();
                if retry {
                    job.attempt += 1;
                    pending.job = Some(job);
                    let(next,_)=wake.wait_timeout(pending,Duration::from_millis(250)).unwrap_or_else(PoisonError::into_inner);pending=next;
                }
                wake.notify_all();if pending.closing&&pending.job.is_none(){break;}
            }
        });Self{service,shared}
    }
    pub(crate) fn queue(&self,value:Value)->Result<(),String>{
        validate("windows",&value)?;let current=self.service.cached_entry("windows")?;
        if current.value==value{return Ok(());}
        let(lock,wake)=&*self.shared;let mut pending=lock.lock().unwrap_or_else(PoisonError::into_inner);if pending.closing{return Err("Window persistence is stopped.".into());}
        pending.job=Some(Job{revision:current.revision,value,attempt:0});wake.notify_all();Ok(())
    }
    pub(crate) fn flush(&self)->Result<(),String>{
        let deadline=Instant::now()+Duration::from_secs(2);let(lock,wake)=&*self.shared;let mut pending=lock.lock().unwrap_or_else(PoisonError::into_inner);
        while pending.active||pending.job.is_some(){let wait=deadline.saturating_duration_since(Instant::now());if wait.is_zero(){pending.flush_timed_out=true;pending.publish();return Err(FLUSH_TIMEOUT.into());}let(next,_)=wake.wait_timeout(pending,wait).unwrap_or_else(PoisonError::into_inner);pending=next;}
        pending.error.clone().map_or(Ok(()),Err)
    }
    pub(crate) fn capture_result(&self, error: Option<String>) {
        let mut pending = self.shared.0.lock().unwrap_or_else(PoisonError::into_inner);
        pending.capture_error = error;
        pending.publish();
    }
    pub(crate) fn error(&self)->Option<String>{self.shared.0.lock().unwrap_or_else(PoisonError::into_inner).diagnostic().map(str::to_owned)}
    pub(crate) fn stop(&self){let mut pending=self.shared.0.lock().unwrap_or_else(PoisonError::into_inner);pending.closing=true;self.shared.1.notify_all();}
}
impl Drop for WindowWriter {fn drop(&mut self){self.stop();}}
#[cfg(test)]
mod tests {
    use super::*;
    fn new(service: Preferences) -> WindowWriter { WindowWriter::start(service, Pending::default()) }
    fn geometry(width:u32)->Value{json!({"x":0,"y":0,"width":width,"height":800,"scaleFactor":1.0,"maximized":false})}
    #[test]
    fn bursts_keep_only_the_latest_geometry_and_same_snapshot_never_rewrites(){
        let root=crate::test_directory();let db=Database::for_namespace(&root.path().join("config.json")).unwrap();let service=Preferences::from_database(Ok(db.clone()));service.read("windows").unwrap();let writer=new(service.clone());
        let lock=osg_persistence::test_support::hold_writer(&db).unwrap();for width in 800..900{writer.queue(geometry(width)).unwrap();}drop(lock);writer.flush().unwrap();assert_eq!(service.read("windows").unwrap().value["width"],899);
        let revision=db.revision("ui","windows").unwrap();writer.queue(geometry(899)).unwrap();writer.flush().unwrap();assert_eq!(db.revision("ui","windows").unwrap(),revision);assert!(writer.queue(json!({"bad":true})).is_err());assert!(writer.error().is_none());writer.stop();assert!(writer.queue(geometry(901)).is_err());
    }
    #[test]
    fn another_owners_commit_rejects_pending_observations_and_only_a_new_gesture_can_replace_it(){
        let root=crate::test_directory();let db=Database::for_namespace(&root.path().join("config.json")).unwrap();let service=Preferences::from_database(Ok(db.clone()));service.read("windows").unwrap();
        let runtime = Runtime::new();
        let writer = WindowWriter::observe(service.clone(), runtime.clone(), Some("restore failed".into())).unwrap();
        assert!(WindowWriter::observe(service.clone(), runtime.clone(), None).is_err());
        let broker = runtime.broker();
        let mut ui = broker.subscribe("geometry-settings", "ui", TOPIC).unwrap();
        let mut mcp = broker.subscribe("geometry-mcp", "mcp", TOPIC).unwrap();
        assert!(broker.subscribe("geometry-dev", "browser", TOPIC).is_err());
        for reader in [&mut ui, &mut mcp] {
            assert_eq!(reader.try_recv().unwrap().unwrap().payload, Some(json!({"error":"restore failed"})));
        }
        writer.capture_result(Some("capture failed".into()));
        writer.capture_result(Some("capture failed".into()));
        for reader in [&mut ui, &mut mcp] {
            assert_eq!(reader.try_recv().unwrap().unwrap().payload, Some(json!({"error":"capture failed"})));
            assert!(reader.try_recv().unwrap().is_none());
        }
        let(arrived,committed)=std::sync::mpsc::channel();let(release,paused)=std::sync::mpsc::channel();let paused=Mutex::new(paused);
        service.observe(Arc::new(move|key,revision|{if key=="windows"&&revision==1{arrived.send(()).unwrap();paused.lock().unwrap().recv_timeout(Duration::from_secs(3)).unwrap();}}));
        writer.queue(geometry(800)).unwrap();committed.recv_timeout(Duration::from_secs(3)).unwrap();writer.queue(geometry(900)).unwrap();
        let foreign=Preferences::from_database(Ok(db.clone()));foreign.write("windows",1,geometry(1000)).unwrap();release.send(()).unwrap();
        let error = writer.flush().unwrap_err();
        assert!(error.contains("revision conflict"));
        assert_eq!(foreign.read("windows").unwrap().value["width"],1000);
        for reader in [&mut ui, &mut mcp] {
            assert_eq!(reader.try_recv().unwrap().unwrap().payload, Some(json!({"error":error})));
        }

        writer.queue(geometry(1100)).unwrap();writer.flush().unwrap();
        assert_eq!(foreign.read("windows").unwrap().value["width"],1100);
        assert_eq!(writer.error().as_deref(), Some("capture failed"), "A successful write must not hide a distinct capture failure");
        for reader in [&mut ui, &mut mcp] {
            assert_eq!(reader.try_recv().unwrap().unwrap().payload, Some(json!({"error":"capture failed"})));
        }
        writer.capture_result(None);
        for reader in [&mut ui, &mut mcp] {
            assert_eq!(reader.try_recv().unwrap().unwrap().payload, Some(json!({"error":null})));
        }
        writer.stop();
    }
    #[test]
    fn flush_is_bounded_on_busy_storage_and_shutdown_does_not_wait_for_sqlite_forever(){
        let root=crate::test_directory();let db=Database::for_namespace(&root.path().join("config.json")).unwrap();let service=Preferences::from_database(Ok(db.clone()));let writer=new(service);
        let lock=osg_persistence::test_support::hold_writer(&db).unwrap();writer.queue(geometry(800)).unwrap();let start=Instant::now();assert!(writer.flush().is_err());assert!(start.elapsed()<Duration::from_secs(3));writer.stop();drop(lock);
    }
}
