//! Explicitly selected HID capture. The mapping service keeps its own controller projection.
#[cfg(windows)]
mod native;
#[cfg(windows)]
use native::Windows as Native;
#[cfg(target_os = "linux")]
#[path = "linux_hid/mod.rs"]
mod linux;
#[cfg(target_os = "linux")]
use linux::Linux as Native;
#[cfg(any(target_os = "macos", test))]
#[path = "macos_hid/mod.rs"]
mod macos;
#[cfg(target_os = "macos")]
use macos::Mac as Native;
mod catalog;
mod recording;
mod replay;
#[cfg(any(target_os = "linux", target_os = "macos", test))]
mod restrictions;
mod session;
#[cfg(test)]
mod tests;
mod transfer;
mod values;

use serde_json::{json, Value};
use session::{Get, Reader, Session};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use uuid::Uuid;

trait Backend: Send + Sync {
    fn inventory(&self, target: Option<&str>) -> Result<Value, String>;
    fn open(&self, target: &str) -> Result<Box<dyn Reader>, String>;
}

pub(super) struct Service {
    stopped: AtomicBool,
    backend: Option<Arc<dyn Backend>>,
    catalog: Mutex<Result<catalog::Catalog, String>>,
    sessions: Mutex<HashMap<String, Active>>,
    recordings: Mutex<recording::Store>,
    replays: Mutex<replay::Store>,
}

struct Active {
    generation: String,
    fingerprint: String,
    session: Option<Session>,
}

fn retire(active: Option<Active>) {
    if let Some(session) = active.and_then(|slot| slot.session) {
        session.revoke(json!({"kind":"stale-generation","message":"HID interface contract replaced or disconnected"}));
    }
}

fn required<'a>(request: &'a Value, name: &str) -> Result<&'a str, String> {
    request[name]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128)
        .ok_or_else(|| format!("Missing or invalid {name}"))
}

impl Service {
    pub fn new(native: bool, path: Option<&Path>) -> Self {
        Self {
            backend: native.then(|| Arc::new(Native::default()) as Arc<dyn Backend>),
            sessions: Mutex::default(),
            recordings: Mutex::default(),
            replays: Mutex::default(),
            catalog: Mutex::new(catalog::Catalog::new(path)),
            stopped: AtomicBool::new(false),
        }
    }

    pub fn request(&self, command: &str, req: &Value) -> Result<Value, String> {
        if self.stopped.load(Ordering::Acquire) {
            return Err("HID service has stopped".into());
        }
        if command.starts_with("controller_hid_replay_") {
            return self
                .replays
                .lock()
                .map_err(|_| "HID replay lock unavailable")?
                .request(command, req);
        }
        if command.starts_with("controller_hid_record_") && command != "controller_hid_record_start"
        {
            return self
                .recordings
                .lock()
                .map_err(|_| "HID recording store lock unavailable")?
                .request(command, req);
        }
        if matches!(
            command,
            "controller_hid_catalog" | "controller_hid_snapshot"
        ) {
            let guard = self
                .catalog
                .lock()
                .map_err(|_| "HID catalog lock unavailable")?;
            let catalog = guard.as_ref().map_err(Clone::clone)?;
            return if command == "controller_hid_catalog" {
                Ok(json!({"snapshots":catalog.snapshots()}))
            } else {
                catalog
                    .snapshot(required(req, "interfaceId")?)
                    .ok_or("Unknown last-observed HID snapshot".into())
            };
        }
        let backend = self
            .backend
            .as_ref()
            .ok_or("Native HID discovery is disabled")?;
        match command {
            "controller_hid_list" => return self.list(backend.as_ref()),
            "controller_hid_inspect" => {
                let mut guard = self
                    .catalog
                    .lock()
                    .map_err(|_| "HID catalog lock unavailable")?;
                return self.inspect(
                    backend.as_ref(),
                    guard.as_mut().map_err(|e| e.clone())?,
                    required(req, "interfaceId")?,
                );
            }
            _ => {}
        }
        let target = required(req, "interfaceId")?;
        if command == "controller_hid_open" {
            return self.open(backend, target, req);
        }
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "HID session lock unavailable")?;
        let generation = required(req, "generation")?;
        let active = sessions
            .get_mut(target)
            .ok_or("Offline: no open HID session")?;
        if active.generation != generation {
            return Err("Stale HID generation".into());
        }
        let fingerprint = active.fingerprint.clone();
        let session = active
            .session
            .as_ref()
            .ok_or("Busy: HID interface is opening or closing")?;
        let result = match command {
            "controller_hid_record_start" => {
                let mut recordings = self
                    .recordings
                    .lock()
                    .map_err(|_| "HID recording store lock unavailable")?;
                return session.record_start(&mut recordings, req);
            }
            "controller_hid_close" => {
                let closing = active.session.take();
                drop(sessions);
                drop(closing);
                self.sessions
                    .lock()
                    .unwrap()
                    .retain(|id, slot| id != target || slot.generation != generation);
                json!({"closed":true})
            }
            "controller_hid_state" => session.state(),
            "controller_hid_reports" => {
                let after = req
                    .get("after")
                    .map(|n| {
                        n.as_str()
                            .and_then(|s| s.parse::<u64>().ok())
                            .ok_or("Invalid HID cursor")
                    })
                    .transpose()?
                    .unwrap_or(0);
                session.page(after)?
            }
            "controller_hid_get" => {
                let get = Get::parse(req)?;
                let reply = session.submit(get)?;
                drop(sessions);
                let mut result = reply.wait(target, generation)?;
                let sessions = self
                    .sessions
                    .lock()
                    .map_err(|_| "HID session lock unavailable")?;
                if !sessions
                    .get(target)
                    .is_some_and(|s| s.generation == generation && s.session.is_some())
                {
                    return Err("Stale HID generation".into());
                }
                result["contractFingerprint"] = json!(fingerprint);
                return Ok(result);
            }
            _ => return Err("Unknown HID receiver command".into()),
        };
        Ok(
            json!({"interfaceId":target,"generation":generation,"contractFingerprint":fingerprint,"result":result}),
        )
    }

    fn inspect(
        &self,
        backend: &dyn Backend,
        catalog: &mut catalog::Catalog,
        target: &str,
    ) -> Result<Value, String> {
        let observed = backend.inventory(Some(target)).and_then(|mut detail| {
            let snapshot = catalog.observe(target, &detail)?;
            detail["contractFingerprint"] = snapshot["contractFingerprint"].clone();
            detail["snapshotId"] = snapshot["snapshotId"].clone();
            Ok((detail, snapshot))
        });
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "HID session lock unavailable")?;
        match observed {
            Ok((detail, snapshot)) => {
                let previous = snapshot["previousInterfaceId"]
                    .as_str()
                    .filter(|id| *id != target)
                    .and_then(|id| sessions.remove(id));
                let changed = sessions
                    .get(target)
                    .is_some_and(|s| detail["contractFingerprint"] != s.fingerprint);
                let invalid = changed.then(|| sessions.remove(target)).flatten();
                drop(sessions);
                retire(previous);
                retire(invalid);
                Ok(detail)
            }
            Err(e) => {
                let invalid = sessions.remove(target);
                drop(sessions);
                retire(invalid);
                Err(e)
            }
        }
    }

    fn list(&self, backend: &dyn Backend) -> Result<Value, String> {
        let mut guard = self
            .catalog
            .lock()
            .map_err(|_| "HID catalog lock unavailable")?;
        let mut list = match backend.inventory(None) {
            Ok(list) => list,
            Err(error) => {
                let old = std::mem::take(
                    &mut *self
                        .sessions
                        .lock()
                        .map_err(|_| "HID session lock unavailable")?,
                );
                for active in old.into_values() {
                    retire(Some(active));
                }
                return Err(error);
            }
        };
        let rows = list["interfaces"]
            .as_array_mut()
            .ok_or("Invalid HID inventory")?;
        let ids: HashSet<_> = rows
            .iter()
            .filter_map(|row| row["interfaceId"].as_str())
            .collect();
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "HID session lock unavailable")?;
        let missing: Vec<_> = sessions
            .iter()
            .filter(|(id, active)| {
                !ids.contains(id.as_str())
                    || active.session.as_ref().is_some_and(Session::is_terminal)
            })
            .map(|(id, _)| id.clone())
            .collect();
        let retired: Vec<_> = missing.iter().map(|id| sessions.remove(id)).collect();
        drop(sessions);
        for entry in retired {
            retire(entry);
        }
        let mut failures = Vec::new();
        for row in rows {
            let Some(id) = row["interfaceId"].as_str().map(str::to_owned) else {
                continue;
            };
            let active = self
                .sessions
                .lock()
                .map_err(|_| "HID session lock unavailable")?
                .contains_key(&id);
            if let Ok(catalog) = guard.as_mut() {
                if active || catalog.needs_refresh(&id, row) {
                    if let Err(error) = self.inspect(backend, catalog, &id) {
                        failures.push(
                            json!({"interfaceId":id,"stage":"refresh-snapshot","message":error}),
                        );
                    }
                }
            }
            let sessions = self
                .sessions
                .lock()
                .map_err(|_| "HID session lock unavailable")?;
            if let Some(active) = sessions.get(&id).filter(|s| s.session.is_some()) {
                row["captureGeneration"] = json!(active.generation);
                row["contractFingerprint"] = json!(active.fingerprint);
            }
        }
        if !failures.is_empty() {
            list["failures"]
                .as_array_mut()
                .ok_or("Invalid HID inventory diagnostics")?
                .extend(failures);
        }
        if let Err(error) = guard.as_ref() {
            list["catalogError"] = json!(error);
        }
        Ok(list)
    }

    fn open(&self, backend: &Arc<dyn Backend>, target: &str, req: &Value) -> Result<Value, String> {
        // Serialize selection and reservation with rediscovery. Never hold
        // the inventory lock across a native open or a report/GET wait.
        let mut guard = self
            .catalog
            .lock()
            .map_err(|_| "HID catalog lock unavailable")?;
        let detail = self.inspect(
            backend.as_ref(),
            guard.as_mut().map_err(|e| e.clone())?,
            target,
        )?;
        let fingerprint = detail["contractFingerprint"]
            .as_str()
            .ok_or("HID descriptor fingerprint unavailable")?
            .to_owned();
        if req
            .get("contractFingerprint")
            .is_some_and(|expected| expected.as_str() != Some(&fingerprint))
        {
            return Err("Stale HID descriptor fingerprint; inspect again".into());
        }
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "HID session lock unavailable")?;
        if self.stopped.load(Ordering::Acquire) {
            return Err("HID service has stopped".into());
        }
        if sessions
            .get(target)
            .and_then(|s| s.session.as_ref())
            .is_some_and(Session::is_terminal)
        {
            retire(sessions.remove(target));
        }
        if sessions.contains_key(target) {
            return Err("Busy: close the previous HID generation first".into());
        }
        if sessions.len() >= 4 {
            return Err("HID session capacity reached".into());
        }
        let generation = Uuid::new_v4().to_string();
        sessions.insert(
            target.to_owned(),
            Active {
                generation: generation.clone(),
                fingerprint: fingerprint.clone(),
                session: None,
            },
        );
        drop(sessions);
        drop(guard);
        let id = target.to_owned();
        let backend = Arc::clone(backend);
        let opened = Session::start_with_detail(detail, move || backend.open(&id));
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "HID session lock unavailable")?;
        let slot = sessions
            .get_mut(target)
            .filter(|s| s.generation == generation)
            .ok_or("HID generation invalidated during open")?;
        match opened {
            Ok(session) => slot.session = Some(session),
            Err(error) => {
                sessions.remove(target);
                return Err(error);
            }
        }
        Ok(json!({"interfaceId":target,"generation":generation,"contractFingerprint":fingerprint}))
    }

    pub fn shutdown(&self) {
        self.stopped.store(true, Ordering::Release);
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.clear();
        }
        if let Ok(mut replays) = self.replays.lock() {
            replays.clear();
        }
    }
}
