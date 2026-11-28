mod effects;
mod history;
#[cfg(windows)]
mod sdk;

use history::History;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub fn run_vjoy_debug_helper() -> bool {
    #[cfg(windows)]
    {
        sdk::run_helper()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(not(windows))]
mod sdk {
    use super::*;
    pub(super) struct Capture {
        pub(super) capabilities: Value,
    }
    impl Capture {
        pub(super) fn failure(&self) -> Option<String> {
            None
        }
    }
    pub(super) fn inspect(_: u8) -> Result<Value, String> {
        Err("The vJoy 2.2.2 Windows driver is unavailable on this platform".into())
    }
    pub(super) fn start(
        _: u8,
        _: Arc<dyn Fn(u32, Vec<u8>) + Send + Sync>,
    ) -> Result<Capture, String> {
        Err("vJoy capture requires Windows".into())
    }
}
trait CaptureHealth: Send {
    fn failure(&self) -> Option<String>;
}
impl CaptureHealth for sdk::Capture {
    fn failure(&self) -> Option<String> {
        sdk::Capture::failure(self)
    }
}
struct Session {
    slot: u8,
    generation: String,
    owner: String,
    capabilities: Value,
    history: Arc<Mutex<History>>,
    capture: Box<dyn CaptureHealth>,
}
#[derive(Default)]
struct State {
    stopped: bool,
    session: Option<Session>,
}
pub(super) struct Service {
    native: bool,
    state: Mutex<State>,
}
impl Service {
    pub(super) fn new(native: bool) -> Self {
        Self {
            native,
            state: Mutex::default(),
        }
    }
    pub(super) fn shutdown(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.stopped = true;
        state.session.take();
    }
    pub(super) fn release_client(&self, client: &str) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.session.as_ref().is_some_and(|s| s.owner == client) {
            state.session.take();
        }
    }
    pub(super) fn request(&self, command: &str, args: &Value) -> Result<Value, String> {
        self.request_with(command, args, sdk::inspect, |slot, sink| {
            let capture = sdk::start(slot, sink)?;
            Ok((capture.capabilities.clone(), Box::new(capture)))
        })
    }
    fn request_with(
        &self,
        command: &str,
        args: &Value,
        inspect: impl FnOnce(u8) -> Result<Value, String>,
        start: impl FnOnce(
            u8,
            Arc<dyn Fn(u32, Vec<u8>) + Send + Sync>,
        ) -> Result<(Value, Box<dyn CaptureHealth>), String>,
    ) -> Result<Value, String> {
        if command == "controller_vjoy_debug_replay" {
            return history::replay(args);
        }
        let slot = args["slot"]
            .as_u64()
            .filter(|n| (1..=16).contains(n))
            .ok_or("Select a vJoy slot from 1 through 16")? as u8;
        if !self.native {
            return Err("Native vJoy inspection is unavailable in this service".into());
        }
        let client = args["clientId"].as_str().unwrap_or("cockpit-ui");
        let mut state = self
            .state
            .lock()
            .map_err(|_| "vJoy capture lock unavailable")?;
        if state.stopped {
            return Err("vJoy debug service has stopped".into());
        }
        match command {
            "controller_vjoy_debug_inspect" => {
                let mut caps = inspect(slot)?;
                if let Some(active) = state.session.as_ref().filter(|s| s.slot == slot) {
                    caps["captureGeneration"] = json!(active.generation);
                }
                return Ok(caps);
            }
            "controller_vjoy_debug_open" => {
                if state.session.is_some() {
                    return Err("A vJoy debug capture already owns the SDK callback".into());
                }
                let history = Arc::new(Mutex::new(History::default()));
                let sink = history.clone();
                let (capabilities, capture) = start(
                    slot,
                    Arc::new(move |cmd, bytes| {
                        sink.lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .observe(slot, cmd, &bytes);
                    }),
                )?;
                let generation = Uuid::new_v4().to_string();
                let reply =
                    json!({"slot":slot,"generation":generation,"capabilities":capabilities});
                state.session = Some(Session {
                    slot,
                    generation,
                    capabilities,
                    owner: client.into(),
                    history,
                    capture,
                });
                return Ok(reply);
            }
            _ => {}
        }
        let active = state
            .session
            .as_ref()
            .filter(|s| s.slot == slot && args["generation"].as_str() == Some(&s.generation))
            .ok_or("Stale or missing vJoy capture generation")?;
        let writable = matches!(
            command,
            "controller_vjoy_debug_close"
                | "controller_vjoy_debug_record_start"
                | "controller_vjoy_debug_record_stop"
        );
        if writable && active.owner != client {
            return Err("Only the capture owner can change this vJoy session".into());
        }
        let generation = active.generation.clone();
        if command == "controller_vjoy_debug_close" {
            state.session.take();
            return Ok(json!({"slot":slot,"generation":generation,"status":"closed"}));
        }
        let failure = active.capture.failure();
        let mut history = active
            .history
            .lock()
            .map_err(|_| "vJoy callback history lock unavailable")?;
        if failure.is_some() {
            let _ = history.stop("backend-ended");
        }
        let mut reply = match command {
            "controller_vjoy_debug_read" => {
                let mut page = history.read(args)?;
                page["status"] = json!(if failure.is_some() { "ended" } else { "open" });
                page["error"] = json!(failure);
                page
            }
            "controller_vjoy_debug_record_start" => {
                if let Some(error) = failure {
                    return Err(error);
                }
                history.record()?;
                json!({"recording":history.status()})
            }
            "controller_vjoy_debug_record_stop" => {
                history.stop("stopped")?;
                json!({"recording":history.status()})
            }
            "controller_vjoy_debug_export" => {
                json!({"recording":history.export(slot,&active.capabilities)?})
            }
            _ => return Err("Unknown vJoy debug operation".into()),
        };
        reply["slot"] = json!(slot);
        reply["generation"] = json!(generation);
        Ok(reply)
    }
}
#[cfg(test)]
mod tests;
