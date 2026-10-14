//! Native input service shared by desktop and browser development transports.
pub mod registry;
#[cfg(windows)]
mod windows;
use registry::{Registry, SavedSource};
use serde_json::Value;
use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
};
const MAX_SETTINGS: u64 = 1_048_576;
pub struct ControllerService {
    registry: Arc<Mutex<Registry>>,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
    path: Option<PathBuf>,
    persistence: Mutex<()>,
    storage_valid: bool,
}
impl ControllerService {
    pub fn new(path: Option<PathBuf>, native: bool) -> Self {
        let mut registry = Registry::default();
        let loaded = path
            .as_ref()
            .map_or(Ok(vec![]), |path| match std::fs::File::open(path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
                Err(e) => Err(e.to_string()),
                Ok(file) => {
                    let mut bytes = Vec::new();
                    file.take(MAX_SETTINGS + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|e| e.to_string())
                        .and_then(|_| {
                            if bytes.len() as u64 > MAX_SETTINGS {
                                Err("Controller settings exceed 1 MiB.".into())
                            } else {
                                serde_json::from_slice::<Vec<SavedSource>>(&bytes)
                                    .map_err(|e| e.to_string())
                            }
                        })
                }
            })
            .and_then(|saved| registry.restore(saved));
        let storage_valid = loaded.is_ok();
        if let Err(error) = loaded {
            registry.diagnostic=format!("Remembered controllers could not be loaded; settings will not be overwritten: {error}");
        }
        let registry = Arc::new(Mutex::new(registry));
        let stop = Arc::new(AtomicBool::new(false));
        #[cfg(windows)]
        let worker = if native {
            let shared = registry.clone();
            let stopped = stop.clone();
            match std::thread::Builder::new()
                .name("controller-input".into())
                .spawn(move || {
                    let result = std::panic::catch_unwind(|| windows::run(shared.clone(), stopped))
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
        #[cfg(not(windows))]
        let worker = None;
        if !native || !cfg!(windows) {
            let mut r = registry.lock().unwrap_or_else(|p| p.into_inner());
            r.backend_status = "unavailable".into();
            if r.diagnostic.is_empty() {
                r.diagnostic =
                    "Native controller capture is not qualified on this platform yet.".into();
            }
        }
        Self {
            registry,
            stop,
            worker: Mutex::new(worker),
            path,
            persistence: Mutex::new(()),
            storage_valid,
        }
    }
    fn save(&self) -> Result<(), String> {
        let _guard = self.persistence.lock().unwrap_or_else(|p| p.into_inner());
        let Some(path) = &self.path else {
            return Ok(());
        };
        if !self.storage_valid {
            return Err("Repair the existing controller settings before saving changes.".into());
        }
        let bytes = {
            let mut r = self.registry.lock().unwrap_or_else(|p| p.into_inner());
            if !r.dirty {
                return Ok(());
            }
            let bytes = serde_json::to_vec(&r.saved()).map_err(|e| e.to_string())?;
            if bytes.len() as u64 > MAX_SETTINGS {
                return Err("Controller settings exceed 1 MiB.".into());
            }
            r.dirty = false;
            bytes
        };
        let result = (|| {
            let parent = path.parent().ok_or("Invalid controller settings path.")?;
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            let mut temporary =
                tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
            temporary.write_all(&bytes).map_err(|e| e.to_string())?;
            temporary.as_file().sync_all().map_err(|e| e.to_string())?;
            temporary.persist(path).map_err(|e| e.to_string())?;
            Ok(())
        })();
        if result.is_err() {
            self.registry
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .dirty = true;
        }
        result
    }
    pub fn request(&self, request: &Value) -> Result<Value, String> {
        if self.stop.load(Ordering::Acquire) {
            return Err("Controller service has stopped.".into());
        }
        let command = request["command"]
            .as_str()
            .ok_or("Missing controller command.")?;
        let id = request["sourceId"].as_str().unwrap_or("");
        let offset = match request.get("offset") {
            None => 0,
            Some(v) => v
                .as_u64()
                .filter(|v| *v <= 256)
                .ok_or("Invalid controller page.")? as usize,
        };
        let result = {
            let mut r = self.registry.lock().unwrap_or_else(|p| p.into_inner());
            match command {
                "controller_list" => r.list(offset),
                "controller_inspect" => r.inspect(id, offset)?,
                "controller_update" => {
                    r.update(
                        id,
                        request["name"].as_str().unwrap_or(""),
                        request["role"].as_str().unwrap_or(""),
                    )?;
                    r.inspect(id, 0)?
                }
                "controller_forget" => {
                    r.forget(id)?;
                    r.list(0)
                }
                "controller_reassign" => {
                    r.reassign(id, request["observedId"].as_str().unwrap_or(""))?;
                    r.inspect(id, 0)?
                }
                _ => return Err("Unknown controller command.".into()),
            }
        };
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
    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = worker.join();
        }
        let _ = self.save();
    }
    pub fn status(&self) -> Value {
        self.registry
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .list(0)
    }
}
impl Drop for ControllerService {
    fn drop(&mut self) {
        self.shutdown();
    }
}
#[cfg(test)]
mod tests;
