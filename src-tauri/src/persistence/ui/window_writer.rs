//! A bounded pending geometry, with no SQLite work in native window callbacks.
use super::*;
use std::{collections::BTreeSet,sync::Condvar,time::{Duration,Instant}};
struct Job {revision:u64,value:Value,attempt:u8}
#[derive(Default)]
struct Pending {job:Option<Job>,active:bool,closing:bool,error:Option<String>}
pub(crate) struct WindowWriter {service:Preferences,shared:Arc<(Mutex<Pending>,Condvar)>}
impl WindowWriter {
    pub(crate) fn new(service:Preferences)->Self{
        let shared=Arc::new((Mutex::new(Pending::default()),Condvar::new()));let owner=shared.clone();let storage=service.clone();
        std::thread::spawn(move||{
            let(lock,wake)=&*owner;let mut own_revisions=BTreeSet::new();let mut last_owned=None;
            loop {
                let mut pending=lock.lock().unwrap_or_else(|p|p.into_inner());
                while pending.job.is_none()&&!pending.closing{pending=wake.wait(pending).unwrap_or_else(|p|p.into_inner());}
                let Some(mut job)=pending.job.take()else{break};pending.active=true;drop(pending);
                if let Ok(current)=storage.cached_entry("windows"){
                    if Some(current.revision)==last_owned&&own_revisions.contains(&job.revision){job.revision=current.revision;}
                }
                let result=storage.write("windows",job.revision,job.value.clone());
                // Reconcile a foreign edit for the next actual gesture. Never
                // retry this old observation against the newer revision.
                if result.as_ref().is_err_and(|error|error.contains("revision conflict")){let _=storage.read("windows");}
                let mut pending=lock.lock().unwrap_or_else(|p|p.into_inner());pending.active=false;
                match result {
                    Ok(entry)=>{if !own_revisions.contains(&job.revision){own_revisions.clear();}own_revisions.insert(job.revision);own_revisions.insert(entry.revision);while own_revisions.len()>32{own_revisions.pop_first();}last_owned=Some(entry.revision);pending.error=None;},
                    Err(error)=>{let retry=error.contains("busy")&&job.attempt<7&&!pending.closing&&pending.job.is_none();pending.error=Some(error);if retry{job.attempt+=1;pending.job=Some(job);let(next,_)=wake.wait_timeout(pending,Duration::from_millis(250)).unwrap_or_else(|p|p.into_inner());pending=next;}}
                }
                wake.notify_all();if pending.closing&&pending.job.is_none(){break;}
            }
        });Self{service,shared}
    }
    pub(crate) fn queue(&self,value:Value)->Result<(),String>{
        validate("windows",&value)?;let current=self.service.cached_entry("windows")?;
        if current.value==value{return Ok(());}
        let(lock,wake)=&*self.shared;let mut pending=lock.lock().unwrap_or_else(|p|p.into_inner());if pending.closing{return Err("Window persistence is stopped.".into());}
        pending.job=Some(Job{revision:current.revision,value,attempt:0});wake.notify_all();Ok(())
    }
    pub(crate) fn flush(&self)->Result<(),String>{
        let deadline=Instant::now()+Duration::from_secs(2);let(lock,wake)=&*self.shared;let mut pending=lock.lock().unwrap_or_else(|p|p.into_inner());
        while pending.active||pending.job.is_some(){let wait=deadline.saturating_duration_since(Instant::now());if wait.is_zero(){return Err("Window persistence did not finish before the flush deadline.".into());}let(next,_)=wake.wait_timeout(pending,wait).unwrap_or_else(|p|p.into_inner());pending=next;}
        pending.error.clone().map_or(Ok(()),Err)
    }
    pub(crate) fn error(&self)->Option<String>{self.shared.0.lock().unwrap_or_else(|p|p.into_inner()).error.clone()}
    pub(crate) fn stop(&self){let mut pending=self.shared.0.lock().unwrap_or_else(|p|p.into_inner());pending.closing=true;self.shared.1.notify_all();}
}
impl Drop for WindowWriter {fn drop(&mut self){self.stop();}}
#[cfg(test)]
mod tests {
    use super::*;
    fn geometry(width:u32)->Value{json!({"x":0,"y":0,"width":width,"height":800,"scaleFactor":1.0,"maximized":false})}
    #[test]
    fn bursts_keep_only_the_latest_geometry_and_same_snapshot_never_rewrites(){
        let root=crate::test_directory();let db=Database::for_namespace(&root.path().join("config.json")).unwrap();let service=Preferences::from_database(Ok(db.clone()));service.read("windows").unwrap();let writer=WindowWriter::new(service.clone());
        let lock=osg_persistence::test_support::hold_writer(&db).unwrap();for width in 800..900{writer.queue(geometry(width)).unwrap();}drop(lock);writer.flush().unwrap();assert_eq!(service.read("windows").unwrap().value["width"],899);
        let revision=db.revision("ui","windows").unwrap();writer.queue(geometry(899)).unwrap();writer.flush().unwrap();assert_eq!(db.revision("ui","windows").unwrap(),revision);assert!(writer.queue(json!({"bad":true})).is_err());assert!(writer.error().is_none());writer.stop();assert!(writer.queue(geometry(901)).is_err());
    }
    #[test]
    fn another_owners_commit_rejects_pending_observations_and_only_a_new_gesture_can_replace_it(){
        let root=crate::test_directory();let db=Database::for_namespace(&root.path().join("config.json")).unwrap();let service=Preferences::from_database(Ok(db.clone()));service.read("windows").unwrap();
        let(arrived,committed)=std::sync::mpsc::channel();let(release,paused)=std::sync::mpsc::channel();let paused=Mutex::new(paused);
        service.observe(Arc::new(move|key,revision|{if key=="windows"&&revision==1{arrived.send(()).unwrap();paused.lock().unwrap().recv_timeout(Duration::from_secs(3)).unwrap();}}));
        let writer=WindowWriter::new(service.clone());writer.queue(geometry(800)).unwrap();committed.recv_timeout(Duration::from_secs(3)).unwrap();writer.queue(geometry(900)).unwrap();
        let foreign=Preferences::from_database(Ok(db.clone()));foreign.write("windows",1,geometry(1000)).unwrap();release.send(()).unwrap();assert!(writer.flush().unwrap_err().contains("revision conflict"));assert_eq!(foreign.read("windows").unwrap().value["width"],1000);
        writer.queue(geometry(1100)).unwrap();writer.flush().unwrap();assert_eq!(foreign.read("windows").unwrap().value["width"],1100);writer.stop();
    }
    #[test]
    fn flush_is_bounded_on_busy_storage_and_shutdown_does_not_wait_for_sqlite_forever(){
        let root=crate::test_directory();let db=Database::for_namespace(&root.path().join("config.json")).unwrap();let service=Preferences::from_database(Ok(db.clone()));let writer=WindowWriter::new(service);
        let lock=osg_persistence::test_support::hold_writer(&db).unwrap();writer.queue(geometry(800)).unwrap();let start=Instant::now();assert!(writer.flush().is_err());assert!(start.elapsed()<Duration::from_secs(3));writer.stop();drop(lock);
    }
}
