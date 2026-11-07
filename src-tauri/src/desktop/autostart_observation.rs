use super::autostart_source::{Source, Stop};
use crate::observations::Runtime;
use osg_streams::{Publisher, TopicSpec};
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};

pub(super) const TOPIC: &str = "settings.autostart";

struct Projection {
    runtime: Runtime,
    publisher: Publisher,
    previous: Value,
}

fn snapshot(result: Result<bool, String>) -> Value {
    match result {
        Ok(enabled) => json!({"enabled":enabled,"error":null}),
        Err(error) => json!({"enabled":null,"error":error}),
    }
}

impl Projection {
    fn register(runtime: Runtime) -> Result<Self, String> {
        let previous = json!({"enabled":null,"error":null});
        let publisher = runtime.broker().register(TopicSpec {
            topic: TOPIC.into(), source: "desktop-autostart".into(),
            audiences: vec!["ui".into(), "mcp".into()],
        }, previous.clone(), runtime.captured_us(), TOPIC.into()).map_err(|error| error.to_string())?;
        Ok(Self { runtime, publisher, previous })
    }

    fn capture(&mut self, status: Result<bool, String>) {
        let next = snapshot(status);
        if next != self.previous && self.publisher.publish_latest_state(1, next.clone(), self.runtime.captured_us(), TOPIC).is_ok() {
            self.previous = next;
        }
    }
}

pub(super) struct Observer {
    source: Source,
    projection: Arc<Mutex<Projection>>,
    worker: Mutex<Worker>,
}

enum Worker {
    Idle,
    Running(Task),
    Closed,
}

struct Task {
    stop: Arc<Stop>,
    thread: JoinHandle<()>,
}

impl Observer {
    pub(super) fn register(runtime: Runtime, source: Source) -> Result<Self, String> {
        Ok(Self { source, projection: Arc::new(Mutex::new(Projection::register(runtime)?)), worker: Mutex::new(Worker::Idle) })
    }

    pub(super) fn ensure(&self, read: impl Fn() -> Result<bool, String> + Send + 'static) {
        let mut worker = self.worker.lock().unwrap_or_else(|error| error.into_inner());
        match &*worker {
            Worker::Closed => return,
            Worker::Running(task) if !task.thread.is_finished() => return,
            _ => {}
        }
        if let Worker::Running(task) = std::mem::replace(&mut *worker, Worker::Idle) { let _ = task.thread.join(); }
        match self.start(read) {
            Ok(task) => *worker = Worker::Running(task),
            Err(error) => self.projection.lock().unwrap_or_else(|error| error.into_inner())
                .capture(Err(format!("Cannot start auto-start observation: {error}"))),
        }
    }

    fn start(&self, read: impl Fn() -> Result<bool, String> + Send + 'static) -> std::io::Result<Task> {
        let stop = Arc::new(Stop::new()?);
        let source = self.source.clone();
        let projection = self.projection.clone();
        let capture = move |status| projection.lock().unwrap_or_else(|error| error.into_inner()).capture(status);
        let worker_stop = stop.clone();
        let thread = thread::Builder::new().name("desktop-autostart".into()).spawn(move || {
            if let Err(error) = source.run(&worker_stop, || capture(read())) {
                capture(Err(format!("Cannot observe auto-start: {error}")));
            }
        })?;
        Ok(Task { stop, thread })
    }

    pub(super) fn stop(&self) {
        let mut worker = self.worker.lock().unwrap_or_else(|error| error.into_inner());
        if let Worker::Running(task) = std::mem::replace(&mut *worker, Worker::Closed) {
            task.stop.signal();
            let _ = task.thread.join();
        }
    }
}

impl Drop for Observer {
    fn drop(&mut self) { self.stop(); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_worker_publishes_a_baseline_and_joins_on_stop() {
        let runtime = Runtime::new();
        #[cfg(windows)]
        let source = Source;
        #[cfg(not(windows))]
        let source = Source::new(std::env::temp_dir().join(uuid::Uuid::new_v4().to_string()));
        let observer = Observer::register(runtime.clone(), source).unwrap();
        let expired = thread::spawn(|| {});
        while !expired.is_finished() { thread::yield_now(); }
        *observer.worker.lock().unwrap() = Worker::Running(Task {
            stop: Arc::new(Stop::new().unwrap()), thread: expired,
        });
        observer.ensure(|| Ok(true));
        observer.ensure(|| panic!("An existing watcher must not be replaced"));
        let mut reader = runtime.broker().subscribe("desktop", "ui", TOPIC).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            if let Some(frame) = reader.try_recv().unwrap() {
                if frame.payload == Some(json!({"enabled":true,"error":null})) { break; }
            }
            assert!(std::time::Instant::now() < deadline, "The registry watcher did not capture a baseline");
            thread::yield_now();
        }
        observer.stop();
        observer.stop();
        observer.ensure(|| panic!("A closed watcher must not restart"));
        assert!(matches!(*observer.worker.lock().unwrap(), Worker::Closed));
    }

    #[test]
    fn exact_autostart_changes_and_failure_are_retained_without_browser_authority() {
        let runtime = Runtime::new();
        let mut projection = Projection::register(runtime.clone()).unwrap();
        assert!(Projection::register(runtime.clone()).is_err(), "A second owner cannot replace the retained source");
        let broker = runtime.broker();
        let mut ui = broker.subscribe("desktop", "ui", TOPIC).unwrap();
        let mut mcp = broker.subscribe("client", "mcp", TOPIC).unwrap();
        assert!(broker.subscribe("dev", "browser", TOPIC).is_err());
        for reader in [&mut ui, &mut mcp] {
            assert_eq!(reader.try_recv().unwrap().unwrap().payload, Some(json!({"enabled":null,"error":null})));
        }
        for status in [Ok(false), Ok(true), Err("notification lost".into())] {
            let expected = snapshot(status.clone());
            projection.capture(status.clone());
            projection.capture(status);
            for reader in [&mut ui, &mut mcp] {
                assert_eq!(reader.try_recv().unwrap().unwrap().payload, Some(expected.clone()));
                assert!(reader.try_recv().unwrap().is_none());
            }
        }
        assert_eq!(broker.subscribe("reopened", "ui", TOPIC).unwrap().try_recv().unwrap().unwrap().payload,
            Some(json!({"enabled":null,"error":"notification lost"})));
    }
}
