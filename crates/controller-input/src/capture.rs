//! Shared, bounded polling lifecycle for the opt-in platform qualification probes.
use crate::registry::{Definition, Registry};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
pub type Shared = Arc<Mutex<Registry>>;
pub type Values = BTreeMap<u16, u32>;
pub const MAX_EVENTS: usize = 4096;
#[derive(Clone, Debug)]
pub enum Event {
    Values(u8, Values),
    Gap(String),
    PendingBaseline,
}
pub trait Input {
    fn definition(&self) -> Definition;
    /// A complete current snapshot, obtained from the kernel, never a synthetic neutral state.
    fn baseline(&mut self) -> Result<Vec<Event>, String>;
    fn poll(&mut self) -> Result<Vec<Event>, String>;
}
pub trait Platform {
    fn paths(&mut self) -> Result<Vec<String>, String>;
    fn open(&mut self, path: &str) -> Result<Box<dyn Input>, String>;
}
struct Connected {
    input: Box<dyn Input>,
    identity_path: String,
    source_id: String,
    needs_baseline: bool,
}
pub struct Capture<P> {
    platform: P,
    devices: BTreeMap<String, Connected>,
    shared: Shared,
}
impl<P: Platform> Capture<P> {
    pub fn new(platform: P, shared: Shared) -> Self {
        Self {
            platform,
            devices: BTreeMap::new(),
            shared,
        }
    }
    pub fn scan(&mut self) -> Result<(), String> {
        let paths = self.platform.paths()?;
        if paths.len() > 1024 {
            return Err("Controller enumeration exceeds 1024 interfaces.".into());
        }
        let present: BTreeSet<_> = paths.into_iter().collect();
        self.devices.retain(|path, device| {
            if present.contains(path) {
                return true;
            }
            let mut r = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(id) = r.id_for_path(&device.identity_path) {
                r.disconnect(&id);
            }
            false
        });
        let mut diagnostics = Vec::new();
        for path in present {
            if self.devices.contains_key(&path) {
                continue;
            }
            let result = (|| {
                let input = self.platform.open(&path)?;
                let definition = input.definition();
                let identity_path = definition.identity.path.clone();
                let source_id = self
                    .shared
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .connect(definition)?;
                self.devices.insert(
                    path,
                    Connected {
                        input,
                        identity_path,
                        source_id,
                        needs_baseline: true,
                    },
                );
                Ok::<_, String>(())
            })();
            if let Err(e) = result {
                if diagnostics.len() < 4 {
                    diagnostics.push(e);
                }
            }
        }
        let mut r = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        r.backend_status = "qualification-probe".into();
        r.diagnostic = diagnostics.join(" ");
        Ok(())
    }
    pub fn tick(&mut self) {
        self.devices.retain(|_, device| {
            let id = {
                self.shared
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .id_for_path(&device.identity_path)
            };
            let Some(id) = id else { return false };
            if id != device.source_id {
                device.source_id = id.clone();
                device.needs_baseline = true;
            }
            let result = if device.needs_baseline {
                device.input.baseline()
            } else {
                device.input.poll()
            };
            let mut r = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            match result {
                Err(error) => {
                    r.fault(&id, &error);
                    r.disconnect(&id);
                    false
                }
                Ok(events) if events.len() > MAX_EVENTS => {
                    r.fault(
                        &id,
                        "Native input queue exceeded its bound; a fresh baseline is required.",
                    );
                    device.needs_baseline = true;
                    true
                }
                Ok(events) => {
                    device.needs_baseline = false;
                    for event in events {
                        match event {
                            Event::PendingBaseline => {
                                device.needs_baseline = true;
                                break;
                            }
                            Event::Gap(reason) => {
                                r.fault(&id, &reason);
                                device.needs_baseline = true;
                                break;
                            }
                            Event::Values(report, values) => {
                                if let Err(error) = r.delta(&id, report, &values) {
                                    r.fault(&id, &error);
                                    device.needs_baseline = true;
                                    break;
                                }
                            }
                        }
                    }
                    true
                }
            }
        });
    }
}
impl<P> Drop for Capture<P> {
    fn drop(&mut self) {
        self.shared
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .disconnect_all();
    }
}
pub fn run<P: Platform>(platform: P, shared: Shared, stop: Arc<AtomicBool>) -> Result<(), String> {
    let mut capture = Capture::new(platform, shared);
    capture.scan()?;
    let mut last = Instant::now();
    while !stop.load(Ordering::Acquire) {
        if last.elapsed() >= Duration::from_millis(500) {
            capture.scan()?;
            last = Instant::now();
        }
        capture.tick();
        std::thread::sleep(Duration::from_millis(4));
    }
    Ok(())
}
/// All virtual output adapters must use this marker. Unknown virtual transports are rejected too.
pub fn owned_virtual(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.starts_with("opensimgear-virtual:") || value.starts_with("osg-virtual:")
}
pub fn permission_error(platform: &str, error: impl std::fmt::Display) -> String {
    match platform {
        "linux"=>format!("Linux controller access failed: {error}. Grant read access only to the selected controller through your session ACL or a scoped udev rule, then check again."),
        _=>format!("macOS controller access failed: {error}. Check Privacy & Security input access for this application, then restart the probe. No permission is requested automatically."),
    }
}
#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;
