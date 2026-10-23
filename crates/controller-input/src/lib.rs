//! Native input service shared by desktop and browser development transports.
#[cfg(any(target_os = "linux", target_os = "macos", test))]
mod capture;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
pub mod registry;
pub mod inventory;
pub mod stream;
pub mod visibility;
#[cfg(windows)]
mod windows;
use registry::{Registry, SavedSource};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
};
const MAX_SETTINGS: u64 = 1_048_576;
fn validate_saved(saved:&Vec<SavedSource>)->Result<(),String> {Registry::default().restore(saved.clone())}
pub struct ControllerService {
    registry: Arc<Mutex<Registry>>,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
    store: Option<osg_persistence::Repository<Vec<SavedSource>>>,
    persistence: Mutex<()>,
    storage_valid: bool,
    visibility: visibility::service::Service,
}
impl ControllerService {
    /// Capture all retained metadata atomically; unlike UI pages, this cannot
    /// mix different connection generations between list and inspect calls.
    pub fn resource_inventory(&self) -> registry::ResourceInventory {
        self.registry.lock().unwrap_or_else(|p| p.into_inner()).resource_inventory()
    }
    /// Failed retained settings cannot authoritatively forget cached inventory.
    pub fn authoritative_resource_inventory(&self) -> Result<registry::ResourceInventory, String> {
        if !self.storage_valid { return Err("Remembered controller settings could not be loaded; retained inventory is preserved.".into()); }
        Ok(self.resource_inventory())
    }

    pub fn new(path: Option<PathBuf>, native: bool) -> Self {
        Self::start(path, native, false)
    }
    /// Explicit read-only qualification tool; never called by ordinary desktop startup.
    pub fn qualification_probe() -> Self {
        Self::start(None, true, true)
    }
    fn start(path: Option<PathBuf>, native: bool, probe: bool) -> Self {
        let mut registry = Registry::default();
        let storage=path.as_ref().map(|path| {
            let db=osg_persistence::Database::for_source(path)?;
            osg_persistence::Repository::new(db,"controllers","identities",1,Some(path.clone()),MAX_SETTINGS as usize,validate_saved)
        }).transpose();
        let loaded=storage.as_ref().map_err(Clone::clone).and_then(|store|store.as_ref().map_or(Ok(vec![]),|store|store.load().map(|saved|saved.unwrap_or_default())))
            .and_then(|saved|registry.restore(saved));
        let storage_valid = loaded.is_ok();
        if let Err(error) = loaded {
            registry.diagnostic=format!("Remembered controllers could not be loaded; settings will not be overwritten: {error}");
        }
        let registry = Arc::new(Mutex::new(registry));
        let stop = Arc::new(AtomicBool::new(false));
        let suppression = visibility::suppression::Control::default();
        let enabled = native
            && (cfg!(windows) || (probe && cfg!(any(target_os = "linux", target_os = "macos"))));
        let mut visibility = visibility::service::Service::new(
            path.as_ref().and_then(|path| path.parent()).map(|path| path.to_path_buf()),
            registry.clone(),
        ).with_suppression(suppression.clone());
        if native && !probe && !cfg!(test) {
            visibility.startup_self_access();
        }
        let worker = if enabled {
            let shared = registry.clone();
            let stopped = stop.clone();
            let suppression = suppression.clone();
            match std::thread::Builder::new()
                .name("controller-input".into())
                .spawn(move || {
                    let result = std::panic::catch_unwind(|| {
                        native_run(shared.clone(), stopped, suppression)
                    })
                    .unwrap_or_else(|_| Err("Controller capture stopped unexpectedly.".into()));
                    let mut r = shared.lock().unwrap_or_else(|p| p.into_inner());
                    r.disconnect_all();
                    match result {
                        Ok(()) => r.backend_status = "stopped".into(),
                        Err(e) => {
                            r.backend_status = "unavailable".into();
                            r.diagnostic = e;
                        }
                    }
                }) {
                Ok(worker) => Some(worker),
                Err(error) => {
                    let mut r = registry.lock().unwrap_or_else(|p| p.into_inner());
                    r.backend_status = "unavailable".into();
                    r.diagnostic = format!("Cannot start controller capture: {error}");
                    None
                }
            }
        } else {
            None
        };
        if !enabled {
            let mut r = registry.lock().unwrap_or_else(|p| p.into_inner());
            r.backend_status = "unavailable".into();
            if r.diagnostic.is_empty() {
                r.diagnostic =
                    "Native controller capture is not qualified on this platform yet. The read-only qualification probe is available separately; ordinary capture stays disabled.".into();
            }
        }
        Self {
            registry,
            stop,
            visibility,
            worker: Mutex::new(worker),
            store: storage.ok().flatten(),
            persistence: Mutex::new(()),
            storage_valid,
        }
    }
    fn save(&self) -> Result<(), String> {
        let _guard = self.persistence.lock().unwrap_or_else(|p| p.into_inner());
        let Some(store)=&self.store else {return if self.storage_valid {Ok(())} else {Err("Repair controller persistence before saving.".into())};};
        if !self.storage_valid {return Err("Repair the existing controller settings before saving changes.".into());}
        let saved={let r=self.registry.lock().unwrap_or_else(|p|p.into_inner());if !r.dirty {return Ok(());}r.saved()};
        store.save(&saved)?;
        let mut r=self.registry.lock().unwrap_or_else(|p|p.into_inner());
        r.dirty=r.saved()!=saved;
        Ok(())
    }
    fn edit(&self,request:&Value)->Result<Value,String> {
        let _writer=self.persistence.lock().unwrap_or_else(|p|p.into_inner());
        if !self.storage_valid {return Err("Repair controller persistence before editing remembered metadata.".into());}
        let before=self.registry.lock().unwrap_or_else(|p|p.into_inner()).clone();let mut staged=before.clone();
        let id=request["sourceId"].as_str().unwrap_or("");
        match request["command"].as_str().unwrap_or("") {
            "controller_update"=>staged.update(id,request["name"].as_str().unwrap_or(""),request["role"].as_str().unwrap_or(""))?,
            "controller_forget"=>staged.forget(id)?,
            "controller_reassign"=>staged.reassign(id,request["observedId"].as_str().unwrap_or(""))?,
            _=>return Err("Unknown controller edit.".into()),
        }
        if let Some(store)=&self.store {store.save(&staged.saved())?;}
        let mut r=self.registry.lock().unwrap_or_else(|p|p.into_inner());r.commit_metadata(&before,&staged);
        let mut result=if request["command"]=="controller_forget" {r.list(0)} else {r.inspect(id,0)?};
        drop(r);
        result["selfAccess"]=serde_json::json!(self.visibility.self_access_status());Ok(result)
    }
    pub fn request(&self, request: &Value) -> Result<Value, String> {
        if self.stop.load(Ordering::Acquire) {
            return Err("Controller service has stopped.".into());
        }
        let command = request["command"]
            .as_str()
            .ok_or("Missing controller command.")?;
        if command.starts_with("controller_visibility_") {
            return self.visibility.request(request);
        }
        if self.visibility.active()
            && matches!(
                command,
                "controller_update" | "controller_forget" | "controller_reassign"
            )
        {
            return Err("Restore visibility before editing controller identities.".into());
        }
        let id = request["sourceId"].as_str().unwrap_or("");
        let offset = match request.get("offset") {
            None => 0,
            Some(v) => v
                .as_u64()
                .filter(|v| *v <= 256)
                .ok_or("Invalid controller page.")? as usize,
        };
        if matches!(command,"controller_update"|"controller_forget"|"controller_reassign") {return self.edit(request);}
        let mut result = {
            let r = self.registry.lock().unwrap_or_else(|p| p.into_inner());
            match command {
                "controller_list" => r.list(offset),
                "controller_inspect" => r.inspect(id, offset)?,
                _ => return Err("Unknown controller command.".into()),
            }
        };
        result["selfAccess"] = serde_json::json!(self.visibility.self_access_status());
        if let Err(error) = self.save() {
            self.registry
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .diagnostic = format!("Controller metadata is not saved: {error}");
            if !["controller_list", "controller_inspect"].contains(&command) {
                return Err(error);
            }
        }
        Ok(result)
    }
    pub fn stream_snapshot(&self, ids: &[String]) -> Result<stream::Snapshot, String> {
        if self.stop.load(Ordering::Acquire) {
            return Err("Controller capture has stopped.".into());
        }
        self.registry
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .stream_snapshot(ids)
    }
    pub fn stream_page(&self, session: &str, after: u64) -> stream::Page {
        self.registry
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .stream_page(session, after)
    }
    pub fn visibility_active(&self) -> bool {
        self.visibility.active()
    }
    /// Internal coordinator boundary, deliberately separate from frontend IPC.
    pub fn game_visibility(&self, operation: &str, value: &Value) -> Result<Value, String> {
        if self.stop.load(Ordering::Acquire)
            && matches!(operation, "capture" | "preflight" | "handoff_preflight" | "handoff_validate" | "start")
        {
            return Err("Controller capture has stopped; game visibility cannot activate.".into());
        }
        self.visibility.game_request(operation, value)
    }
    pub fn shutdown(&self) {
        self.visibility.shutdown();
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = worker.join();
        }
        let _ = self.save();
    }
    pub fn status(&self) -> Value {
        let mut result = self.registry
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .list(0);
        result["selfAccess"] = serde_json::json!(self.visibility.self_access_status());
        result
    }
}
impl Drop for ControllerService {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn native_run(
    shared: Arc<Mutex<Registry>>,
    stop: Arc<AtomicBool>,
    suppression: visibility::suppression::Control,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        let _ = suppression;
        windows::run(shared, stop)
    }
    #[cfg(target_os = "linux")]
    {
        capture::run_controlled(linux::Linux::new(), shared, stop, suppression)
    }
    #[cfg(target_os = "macos")]
    {
        capture::run_controlled(macos::Mac::new()?, shared, stop, suppression)
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = (shared, stop, suppression);
        Err("Unsupported native capture platform.".into())
    }
}
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn test_directory()->tempfile::TempDir {
    let parent=std::env::temp_dir();
    #[cfg(unix)]
    let parent=parent.canonicalize().expect("canonical OS temporary directory");
    tempfile::tempdir_in(parent).expect("isolated controller fixture")
}
