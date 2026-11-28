//! Explicitly selected HID capture. The mapping service keeps its own controller projection.
mod native;
mod session;
#[cfg(test)]
mod tests;

use serde_json::{json, Value};
use session::{Get, Reader, Session};
use std::{
    collections::HashMap,
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
    sessions: Mutex<HashMap<String, (String, Option<Session>)>>,
}

fn required<'a>(request: &'a Value, name: &str) -> Result<&'a str, String> {
    request[name]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128)
        .ok_or_else(|| format!("Missing or invalid {name}"))
}

impl Service {
    pub fn new(native: bool) -> Self {
        Self {
            backend: native.then(|| Arc::new(native::Windows::default()) as Arc<dyn Backend>),
            sessions: Mutex::default(),
            stopped: AtomicBool::new(false),
        }
    }

    pub fn request(&self, command: &str, req: &Value) -> Result<Value, String> {
        let backend = self
            .backend
            .as_ref()
            .ok_or("Native HID discovery is disabled")?;
        match command {
            "controller_hid_list" => return backend.inventory(None),
            "controller_hid_inspect" => {
                return backend.inventory(Some(required(req, "interfaceId")?))
            }
            _ => {}
        }
        let target = required(req, "interfaceId")?;
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "HID session lock unavailable")?;
        if command == "controller_hid_open" {
            if self.stopped.load(Ordering::Acquire) {
                return Err("HID service has stopped".into());
            }
            if sessions.contains_key(target) {
                return Err("Busy: close the previous HID generation first".into());
            }
            if sessions.len() >= 4 {
                return Err("HID session capacity reached".into());
            }
            let generation = Uuid::new_v4().to_string();
            let backend = Arc::clone(backend);
            let id = target.to_owned();
            sessions.insert(target.to_owned(), (generation.clone(), None));
            drop(sessions);
            let opened = Session::start(move || backend.open(&id));
            let mut sessions = self.sessions.lock().unwrap();
            let (_, slot) = sessions
                .get_mut(target)
                .ok_or("HID service stopped during open")?;
            match opened {
                Ok(session) => *slot = Some(session),
                Err(error) => {
                    sessions.remove(target);
                    return Err(error);
                }
            }
            return Ok(json!({"interfaceId":target,"generation":generation}));
        }
        let generation = required(req, "generation")?;
        let (current, slot) = sessions
            .get_mut(target)
            .ok_or("Offline: no open HID session")?;
        if current != generation {
            return Err("Stale HID generation".into());
        }
        let session = slot
            .as_ref()
            .ok_or("Busy: HID interface is opening or closing")?;
        let result = match command {
            "controller_hid_close" => {
                let closing = slot.take();
                drop(sessions);
                drop(closing);
                self.sessions.lock().unwrap().remove(target);
                json!({"closed":true})
            }
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
                return reply.wait(target, generation);
            }
            _ => return Err("Unknown HID receiver command".into()),
        };
        Ok(json!({"interfaceId":target,"generation":generation,"result":result}))
    }

    pub fn shutdown(&self) {
        self.stopped.store(true, Ordering::Release);
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.clear();
        }
    }
}
