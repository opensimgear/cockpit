//! Passive UI, browser, and MCP observation of the shared CAN panel capture.
use crate::stm32_panel_capture::{Batch, Capture, Edge, Ready, Snapshot};
use osg_streams::{Broker, Delivery, LossReason, NativeSubscription, NativeWake, Publisher, TopicSpec};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
    thread,
    time::Instant,
};

const MAX_BRIDGES: usize = 4;
const CLOCK: &str = "gateway-panel-receive";

pub(crate) fn route_topic(topic: &str) -> Option<(&str, &str)> {
    let (gateway, module) = topic.strip_prefix("can.inputs.")?.split_once('.')?;
    let lower_hex = |id: &str, length| {
        id.len() == length && id.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    };
    (lower_hex(gateway, 24) && lower_hex(module, 32)).then_some((gateway, module))
}

#[derive(Default)]
pub(crate) struct Bridges(Arc<Mutex<Registry>>);

#[derive(Default)]
struct Registry {
    closed: bool,
    entries: HashMap<String, Arc<Bridge>>,
}

struct Bridge {
    capture: Arc<Capture>,
    wake: Arc<NativeWake>,
    broker: Broker,
    publisher: Publisher,
    stopped: AtomicBool,
}

impl Bridge {
    fn stop(&self) {
        if !self.stopped.swap(true, Ordering::AcqRel) {
            self.broker.retire(&self.publisher);
            self.wake.notify();
        }
    }
}

impl Bridges {
    pub(crate) fn with_topic<T>(
        &self,
        topic: &str,
        capture: Arc<Capture>,
        broker: Broker,
        watch_lifecycle: impl FnOnce(Arc<NativeWake>),
        action: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        route_topic(topic).ok_or("Invalid CAN panel input observation topic.")?;
        let mut registry = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if registry.closed {
            return Err("CAN panel input observations are closed.".into());
        }
        if registry.entries.get(topic).is_some_and(|entry| !Arc::ptr_eq(&entry.capture, &capture)) {
            if let Some(old) = registry.entries.remove(topic) {
                old.stop();
            }
        }
        let bridge = if let Some(existing) = registry.entries.get(topic) {
            existing.clone()
        } else {
            if registry.entries.len() >= MAX_BRIDGES {
                return Err("CAN panel input observation bridge limit reached.".into());
            }
            let bridge = self.start(topic, capture, broker)?;
            registry.entries.insert(topic.into(), bridge.clone());
            bridge
        };
        watch_lifecycle(bridge.wake.clone());
        let result = action();
        if !bridge.publisher.has_readers() {
            bridge.stop();
            registry.entries.remove(topic);
        }
        result
    }

    fn start(&self, topic: &str, capture: Arc<Capture>, broker: Broker) -> Result<Arc<Bridge>, String> {
        let wake = Arc::new(NativeWake::default());
        let watch = capture.watch(wake.clone())?;
        let mut reader = capture.subscribe()?;
        let initial =
            reader.try_recv().map_err(|error| format!("CAN panel observation baseline failed: {error:?}"))?;
        let (payload, at, clock) = match initial {
            Some(Delivery::Snapshot(snapshot)) => (
                snapshot.value.0.map_or_else(
                    || unavailable("Waiting for a fresh CAN panel baseline."),
                    |ready| preview_payload(&ready, &[]),
                ),
                snapshot.captured_us,
                snapshot.clock_id.clone(),
            ),
            _ => (unavailable("Waiting for a fresh CAN panel baseline."), 0, CLOCK.into()),
        };
        let ready = payload["status"] == "ready";
        let publisher = broker
            .register(
                TopicSpec {
                    topic: topic.into(),
                    source: "can-inputs".into(),
                    audiences: vec!["ui".into(), "browser".into(), "mcp".into()],
                },
                payload,
                at,
                clock.clone(),
            )
            .map_err(|error| error.to_string())?;
        let bridge = Arc::new(Bridge { capture, wake, broker, publisher, stopped: AtomicBool::new(false) });
        let worker = bridge.clone();
        let registry = Arc::downgrade(&self.0);
        let topic = topic.to_owned();
        if let Err(error) = thread::Builder::new().name("can-panel-observations".into()).spawn(move || {
            let _watch = watch;
            let mut relay = Relay { reader: Some(reader), at, clock, ready };
            run(&registry, &topic, &worker, &mut relay);
            retire(&registry, &topic, &worker);
        }) {
            bridge.stop();
            return Err(format!("Could not start CAN panel observation bridge: {error}"));
        }
        Ok(bridge)
    }

    pub(crate) fn close(&self) {
        let mut registry = self.0.lock().unwrap_or_else(|error| error.into_inner());
        registry.closed = true;
        for (_, bridge) in registry.entries.drain() {
            bridge.stop();
        }
    }
}

impl Drop for Bridges {
    fn drop(&mut self) {
        self.close();
    }
}

fn run(registry: &Weak<Mutex<Registry>>, topic: &str, bridge: &Arc<Bridge>, relay: &mut Relay) {
    let _ = bridge.wake.bind_thread(thread::current());
    loop {
        let observed = bridge.wake.observed();
        if bridge.stopped.load(Ordering::Acquire) || idle(registry, topic, bridge) {
            return;
        }
        bridge.capture.expire(Instant::now());
        if relay.receive(bridge).is_err() {
            return;
        }
        let delay = relay
            .reader
            .as_ref()
            .and_then(|_| bridge.capture.deadline())
            .map(|deadline| deadline.saturating_duration_since(Instant::now()));
        bridge.wake.wait_scheduled(observed, delay);
    }
}

fn idle(registry: &Weak<Mutex<Registry>>, topic: &str, bridge: &Arc<Bridge>) -> bool {
    let Some(registry) = registry.upgrade() else {
        return true;
    };
    let mut registry = registry.lock().unwrap_or_else(|error| error.into_inner());
    if !bridge.publisher.has_readers() {
        remove(&mut registry, topic, bridge);
        return true;
    }
    false
}

fn retire(registry: &Weak<Mutex<Registry>>, topic: &str, bridge: &Arc<Bridge>) {
    if let Some(registry) = registry.upgrade() {
        remove(&mut registry.lock().unwrap_or_else(|error| error.into_inner()), topic, bridge);
    }
    bridge.stop();
}

fn remove(registry: &mut Registry, topic: &str, bridge: &Arc<Bridge>) {
    if registry.entries.get(topic).is_some_and(|current| Arc::ptr_eq(current, bridge)) {
        bridge.stop();
        registry.entries.remove(topic);
    }
}

struct Relay {
    reader: Option<NativeSubscription<Snapshot, Batch>>,
    at: u64,
    clock: String,
    ready: bool,
}

impl Relay {
    fn receive(&mut self, bridge: &Bridge) -> Result<(), String> {
        // Match the native pending-frame cap. A busy capture must still yield
        // to shutdown and the subscriber lifecycle after each bounded drain.
        for _ in 0..64 {
            if bridge.stopped.load(Ordering::Acquire) {
                return Ok(());
            }
            let Some(reader) = self.reader.as_mut() else {
                return Ok(());
            };
            let delivery = match reader.try_recv() {
                Ok(Some(delivery)) => delivery,
                Ok(None) => return Ok(()),
                Err(error) => {
                    self.withdraw(bridge, &format!("CAN panel capture is unavailable: {error:?}"))?;
                    self.reader = None;
                    return Ok(());
                }
            };
            match delivery {
                Delivery::Snapshot(snapshot) => {
                    if let Some(ready) = snapshot.value.0 {
                        self.publish(bridge, &ready, &[], snapshot.captured_us, &snapshot.clock_id)?;
                    } else if self.ready {
                        self.withdraw(bridge, "CAN panel capture has no fresh baseline.")?;
                    }
                }
                Delivery::Event(event) => {
                    self.publish(
                        bridge,
                        &event.value.latest,
                        &event.value.edges,
                        event.captured_us,
                        &event.clock_id,
                    )?;
                }
                Delivery::Loss(loss) => {
                    self.withdraw(bridge, &format!("CAN panel capture lost continuity: {:?}", loss.reason))?;
                    match loss.reason {
                        LossReason::GenerationReplaced => {}
                        LossReason::Overflow => {
                            self.reader.take();
                            self.reader = bridge.capture.subscribe().ok();
                        }
                        _ => self.reader = None,
                    }
                }
            }
        }
        bridge.wake.notify();
        Ok(())
    }

    fn publish(
        &mut self,
        bridge: &Bridge,
        ready: &Ready,
        edges: &[Edge],
        at: u64,
        clock: &str,
    ) -> Result<(), String> {
        let snapshot = preview_payload(ready, &[]);
        let generation = bridge.publisher.generation();
        if self.ready {
            let event = if edges.is_empty() { snapshot.clone() } else { preview_payload(ready, edges) };
            bridge
                .publisher
                .publish(generation, snapshot, event, at, clock)
                .map_err(|error| error.to_string())?;
        } else {
            // The first ready state establishes a baseline. Preserve a pending
            // loss ahead of it, and never replay edges into a fresh reader.
            bridge
                .publisher
                .publish_latest_state(generation, snapshot, at, clock)
                .map_err(|error| error.to_string())?;
        }
        self.at = at;
        self.clock = clock.into();
        self.ready = true;
        Ok(())
    }

    fn withdraw(&mut self, bridge: &Bridge, error: &str) -> Result<(), String> {
        let generation =
            bridge.publisher.generation().checked_add(1).ok_or("CAN observation generation exhausted.")?;
        bridge
            .publisher
            .replace_generation(generation, unavailable(error), self.at, &self.clock)
            .map_err(|error| error.to_string())?;
        self.ready = false;
        Ok(())
    }
}

fn unavailable(error: &str) -> Value {
    json!({"status":"unavailable","error":error})
}

pub(crate) fn preview_payload(ready: &Ready, edges: &[Edge]) -> Value {
    let route = ready.route;
    let layout = ready.layout;
    let controls = usize::from(layout.contact_count)
        + 2 * usize::from(layout.encoder_count)
        + 3 * usize::from(layout.selector_count);
    let bit = |bytes: &[u8], index: usize| bytes[index / 8] & (1 << (index % 8)) != 0;
    json!({
        "status": "ready",
        "route": {
            "node": route.node, "sourceNode": route.source_node, "boot": route.boot_session,
            "lease": route.lease, "epoch": route.gateway_epoch,
            "configurationGeneration": route.configuration_generation,
        },
        "layout": {
            "contactCount": layout.contact_count, "encoderCount": layout.encoder_count,
            "selectorCount": layout.selector_count,
            "maintainedButtons": (0..usize::from(layout.contact_count))
                .filter(|&index| bit(&layout.maintained, index)).collect::<Vec<_>>(),
        },
        "cursor": ready.cursor,
        "sampledAtMs": ready.sampled_at_ms,
        "stateSampledAtMs": ready.state_sampled_at_ms,
        "held": (0..controls).map(|index| bit(&ready.state.held, index)).collect::<Vec<_>>(),
        "axes": ready.state.axes,
        "axisAvailable": (0..8).map(|index| bit(&[ready.state.axis_available], index)).collect::<Vec<_>>(),
        "edges": edges.iter().map(|edge| json!({
            "control": edge.control, "pressed": edge.pressed, "capturedAtMs": edge.captured_at_ms,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        stm32_panel_capture::Hub,
        stm32_panel_wire::{self, Packet, Payload, Record},
    };
    use osg_streams::{Reader, WireFrame};
    use std::time::Duration;

    const GATEWAY: &str = "0123456789abcdef01234567";
    const MODULE: &str = "0123456789abcdef0123456789abcdef";

    fn next(reader: &mut Reader) -> WireFrame {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(frame) = reader.try_recv().unwrap() {
                return frame;
            }
            assert!(Instant::now() < deadline, "CAN observation timed out");
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn record(sequence: u32, cursor: u32, sampled_at_ms: u32, payload: Payload) -> Record {
        Record::Event {
            module_id: MODULE.into(),
            source_node: 11,
            packet: Packet {
                node: 1,
                boot_session: 2,
                lease: 3,
                gateway_epoch: 4,
                configuration_generation: 5,
                sequence,
                cursor,
                sampled_at_ms,
                payload,
            },
        }
    }

    #[test]
    fn can_topic_binds_exact_canonical_identities() {
        let topic = format!("can.inputs.{GATEWAY}.{MODULE}");
        assert_eq!(route_topic(&topic), Some((GATEWAY, MODULE)));
        assert_eq!(route_topic(&topic.to_ascii_uppercase()), None);
        assert_eq!(route_topic(&format!("{topic}.extra")), None);
    }

    #[test]
    fn can_observation_shares_ordered_edges_and_withdraws_on_loss() {
        let hub = Hub::default();
        let capture = hub.capture(MODULE).unwrap();
        let topic = format!("can.inputs.{GATEWAY}.{MODULE}");
        let broker = Broker::new("test".into()).unwrap();
        let bridges = Bridges::default();
        let mut reader = bridges
            .with_topic(
                &topic,
                capture.clone(),
                broker.clone(),
                |_| {},
                || broker.subscribe("inspector", "ui", &topic).map_err(|error| error.to_string()),
            )
            .unwrap();
        assert_eq!(next(&mut reader).payload.unwrap()["status"], "unavailable");
        let start = Instant::now();
        hub.accept(
            record(
                1,
                0,
                10,
                Payload::Layout {
                    contact_count: 1,
                    encoder_count: 0,
                    selector_count: 0,
                    maintained: [0; 16],
                },
            ),
            start,
        );
        hub.accept(
            record(2, 0, 10, Payload::State { held: [0; 16], axes: [123; 8], axis_available: 1 }),
            start,
        );
        let baseline = next(&mut reader);
        assert_eq!(baseline.kind, "snapshot");
        let baseline = baseline.payload.unwrap();
        assert_eq!(baseline["status"], "ready");
        assert_eq!(baseline["route"]["sourceNode"], 11);
        assert_eq!(baseline["axisAvailable"][0], true);
        hub.accept(
            record(
                3,
                2,
                12,
                Payload::Edges(vec![
                    stm32_panel_wire::Edge { control: 0, pressed: true, captured_at_ms: 11 },
                    stm32_panel_wire::Edge { control: 0, pressed: false, captured_at_ms: 12 },
                ]),
            ),
            Instant::now(),
        );
        let change = next(&mut reader);
        assert_eq!(change.kind, "change");
        assert_eq!(
            change.payload.unwrap()["edges"],
            json!([
                {"control":0,"pressed":true,"capturedAtMs":11},
                {"control":0,"pressed":false,"capturedAtMs":12},
            ])
        );
        let mut late = bridges
            .with_topic(
                &topic,
                capture,
                broker.clone(),
                |_| {},
                || broker.subscribe("mcp", "mcp", &topic).map_err(|error| error.to_string()),
            )
            .unwrap();
        let retained = next(&mut late).payload.unwrap();
        assert_eq!(retained["cursor"], 2);
        assert_eq!(retained["edges"], json!([]));
        hub.accept(Record::Loss { module_id: MODULE.into() }, Instant::now());
        assert_eq!(next(&mut late).kind, "loss");
        assert_eq!(next(&mut late).payload.unwrap()["status"], "unavailable");
        bridges.close();
        assert!(broker.subscribe("third", "ui", &topic).is_err());
    }
}
