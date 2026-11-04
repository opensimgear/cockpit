//! Native capture for one CDC gateway's ordered CAN panel records.
use crate::stm32_panel_wire::{self, Packet, Payload, Record};
use osg_streams::{NativeLimits, NativeStream, NativeSubscription, NativeWake, NativeWatch, RetainedBytes};
use std::{
    collections::HashMap,
    mem::size_of,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const CLOCK: &str = "gateway-panel-receive";
const FRESHNESS: Duration = Duration::from_millis(250);
const LIMITS: NativeLimits = NativeLimits {
    max_subscribers: 16,
    max_snapshot_bytes: 1024,
    max_frame_bytes: 1024,
    max_pending_frames: 64,
    max_pending_bytes: 64 * 1024,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Route {
    pub node: u8,
    pub source_node: u8,
    pub boot_session: u32,
    pub lease: u32,
    pub gateway_epoch: u32,
    pub configuration_generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Layout {
    pub contact_count: u8,
    pub encoder_count: u8,
    pub selector_count: u8,
    pub maintained: [u8; 16],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct State {
    pub held: [u8; 16],
    pub axes: [u16; 8],
    pub axis_available: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Ready {
    pub route: Route,
    pub layout: Layout,
    pub state: State,
    pub cursor: u32,
    pub sampled_at_ms: u32,
    pub state_sampled_at_ms: u32,
    pub received_at: Instant,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Snapshot(pub Option<Ready>);

impl RetainedBytes for Snapshot {
    fn retained_bytes(&self) -> usize {
        size_of::<Self>()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Edge {
    pub control: u8,
    pub pressed: bool,
    pub captured_at_ms: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct Batch {
    pub latest: Ready,
    pub edges: Vec<Edge>,
    pub received_at: Instant,
}

impl RetainedBytes for Batch {
    fn retained_bytes(&self) -> usize {
        size_of::<Self>() + self.edges.capacity() * size_of::<Edge>()
    }
}

struct Cursor {
    route: Route,
    sequence: u32,
    layout: Option<Layout>,
    ready: Option<Ready>,
    // Kept across withdrawal within a route to fence stale recovery.
    last_state: Option<(Ready, Instant)>,
}

pub(crate) struct Capture {
    stream: NativeStream<Snapshot, Batch>,
    cursor: Mutex<Option<Cursor>>,
    origin: Instant,
}

impl Capture {
    fn new() -> Result<Self, String> {
        Ok(Self {
            stream: NativeStream::new(1, Snapshot(None), 0, CLOCK, LIMITS)
                .map_err(|error| format!("Could not start CAN panel capture: {error:?}"))?,
            cursor: Mutex::new(None),
            origin: Instant::now(),
        })
    }

    pub(crate) fn subscribe(&self) -> Result<NativeSubscription<Snapshot, Batch>, String> {
        self.expire(Instant::now());
        self.stream
            .subscribe()
            .map_err(|error| format!("Could not subscribe to CAN panel capture: {error:?}"))
    }

    pub(crate) fn latest(&self, now: Instant) -> Option<Ready> {
        let mut guard = self.cursor.lock().unwrap_or_else(|error| error.into_inner());
        let cursor = guard.as_mut()?;
        self.expire_cursor(cursor, now);
        cursor.ready
    }

    pub(crate) fn watch(&self, wake: Arc<NativeWake>) -> Result<NativeWatch<Snapshot, Batch>, String> {
        self.stream.watch(wake).map_err(|error| format!("Could not watch CAN panel capture: {error:?}"))
    }

    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.cursor
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()?
            .ready
            .map(|state| state.received_at + FRESHNESS)
    }

    pub(crate) fn expire(&self, now: Instant) {
        let mut guard = self.cursor.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cursor) = guard.as_mut() {
            self.expire_cursor(cursor, now);
        }
    }

    fn expire_cursor(&self, cursor: &mut Cursor, now: Instant) {
        if cursor.ready.is_some_and(|ready| now >= ready.received_at + FRESHNESS) {
            self.withdraw(cursor, now);
        }
    }

    fn micros(&self, now: Instant) -> u64 {
        now.saturating_duration_since(self.origin).as_micros().min(u64::MAX as u128) as u64
    }

    fn withdraw(&self, cursor: &mut Cursor, now: Instant) {
        if cursor.ready.take().is_none() {
            return;
        }
        let result = self.stream.generation().checked_add(1).and_then(|generation| {
            self.stream.replace_generation(generation, Snapshot(None), self.micros(now), CLOCK).ok()
        });
        if result.is_none() {
            self.stream.close();
        }
    }

    fn loss(&self, now: Instant, clear: bool) {
        let mut guard = self.cursor.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cursor) = guard.as_mut() {
            self.withdraw(cursor, now);
        }
        if clear {
            *guard = None;
        }
    }

    fn accept(&self, source_node: u8, packet: Packet, now: Instant) {
        let route = Route {
            node: packet.node,
            source_node,
            boot_session: packet.boot_session,
            lease: packet.lease,
            gateway_epoch: packet.gateway_epoch,
            configuration_generation: packet.configuration_generation,
        };
        let mut guard = self.cursor.lock().unwrap_or_else(|e| e.into_inner());
        if guard.as_ref().is_some_and(|cursor| cursor.route != route) {
            if let Some(previous) = guard.as_mut() {
                self.withdraw(previous, now);
            }
            *guard = None;
        }
        let cursor =
            guard.get_or_insert(Cursor { route, sequence: 0, layout: None, ready: None, last_state: None });
        self.expire_cursor(cursor, now);
        if packet.sequence <= cursor.sequence {
            return;
        }
        if cursor.sequence != 0 && packet.sequence != cursor.sequence + 1 {
            self.withdraw(cursor, now);
        }
        cursor.sequence = packet.sequence;
        let mut edges = Vec::new();
        let latest = match packet.payload {
            Payload::Layout { contact_count, encoder_count, selector_count, maintained } => {
                let layout = Layout { contact_count, encoder_count, selector_count, maintained };
                if !layout.valid() || cursor.layout.is_some_and(|previous| previous != layout) {
                    self.withdraw(cursor, now);
                } else {
                    cursor.layout = Some(layout);
                }
                return;
            }
            Payload::State { held, axes, axis_available } => {
                let Some(layout) = cursor.layout else { return };
                if cursor.ready.is_some_and(|ready| packet.cursor != ready.cursor)
                    || cursor.last_state.is_some_and(|(last, _)| packet.cursor < last.cursor)
                    || !layout.valid_held(&held)
                {
                    self.withdraw(cursor, now);
                    return;
                }
                Ready {
                    route,
                    layout,
                    state: State { held, axes, axis_available },
                    cursor: packet.cursor,
                    sampled_at_ms: packet.sampled_at_ms,
                    state_sampled_at_ms: packet.sampled_at_ms,
                    received_at: now,
                }
            }
            Payload::Edges(entries) => {
                let Some(mut latest) = cursor.ready else {
                    return;
                };
                if latest.cursor.checked_add(entries.len() as u32) != Some(packet.cursor)
                    || entries.iter().any(|edge| usize::from(edge.control) >= latest.layout.controls())
                {
                    self.withdraw(cursor, now);
                    return;
                }
                edges = entries
                    .into_iter()
                    .map(|edge| {
                        latest.layout.apply(&mut latest.state.held, &edge);
                        Edge {
                            control: edge.control,
                            pressed: edge.pressed,
                            captured_at_ms: edge.captured_at_ms,
                        }
                    })
                    .collect();
                latest.cursor = packet.cursor;
                latest.sampled_at_ms = packet.sampled_at_ms;
                latest
            }
            Payload::Gap => {
                self.withdraw(cursor, now);
                return;
            }
        };
        self.publish(cursor, latest, edges, now);
    }

    fn publish(&self, cursor: &mut Cursor, mut latest: Ready, edges: Vec<Edge>, now: Instant) {
        let mut advance_receipt = now;
        if let Some((previous, previous_receipt)) = cursor.last_state {
            let delta = latest.sampled_at_ms.wrapping_sub(previous.sampled_at_ms);
            if delta > i32::MAX as u32 || (delta == 0 && cursor.ready.is_none()) {
                self.withdraw(cursor, now);
                return;
            }
            if delta == 0 {
                latest.received_at = previous.received_at;
                advance_receipt = previous_receipt;
            } else {
                latest.received_at = now.min(previous_receipt + Duration::from_millis(u64::from(delta)));
            }
        }
        if now >= latest.received_at + FRESHNESS {
            self.withdraw(cursor, now);
            return;
        }
        let batch = Batch { latest, edges, received_at: now };
        if self
            .stream
            .publish(self.stream.generation(), batch, Snapshot(Some(latest)), self.micros(now), CLOCK)
            .is_err()
        {
            self.stream.close();
            cursor.ready = None;
            return;
        }
        cursor.last_state = Some((latest, advance_receipt));
        cursor.ready = Some(latest);
    }
}

impl Layout {
    fn controls(self) -> usize {
        usize::from(self.contact_count)
            + 2 * usize::from(self.encoder_count)
            + 3 * usize::from(self.selector_count)
    }

    fn valid(self) -> bool {
        self.encoder_count <= 8
            && self.selector_count <= 8
            && self.controls() <= 128
            && (usize::from(self.contact_count)..128).all(|i| self.maintained[i / 8] & (1 << (i % 8)) == 0)
    }

    fn apply(self, held: &mut [u8; 16], edge: &stm32_panel_wire::Edge) {
        let index = usize::from(edge.control);
        let contacts = usize::from(self.contact_count);
        if (contacts..contacts + 2 * usize::from(self.encoder_count)).contains(&index) {
            return;
        }
        let mask = 1 << (index % 8);
        if edge.pressed {
            held[index / 8] |= mask;
        } else {
            held[index / 8] &= !mask;
        }
    }

    fn valid_held(self, held: &[u8; 16]) -> bool {
        let contacts = usize::from(self.contact_count);
        let encoders = contacts..contacts + 2 * usize::from(self.encoder_count);
        encoders.chain(self.controls()..128).all(|i| held[i / 8] & (1 << (i % 8)) == 0)
    }
}

#[derive(Default)]
pub(crate) struct Hub(Mutex<HashMap<String, Arc<Capture>>>);

impl Hub {
    pub(crate) fn capture(&self, module_id: &str) -> Result<Arc<Capture>, String> {
        if module_id.len() != 32 || !module_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("Invalid CAN panel identity.".into());
        }
        let id = module_id.to_ascii_lowercase();
        let mut captures = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(capture) = captures.get(&id) {
            return Ok(capture.clone());
        }
        if captures.len() == 32 {
            let unused = captures
                .iter()
                .filter(|(_, capture)| Arc::strong_count(capture) == 1)
                .min_by_key(|(_, capture)| capture.deadline())
                .map(|(id, _)| id.clone())
                .ok_or("CAN panel capture limit reached.")?;
            captures.remove(&unused);
        }
        let capture = Arc::new(Capture::new()?);
        captures.insert(id, capture.clone());
        Ok(capture)
    }

    pub(crate) fn accept(&self, record: Record, now: Instant) {
        match record {
            Record::Reset => {
                for capture in self.0.lock().unwrap_or_else(|e| e.into_inner()).values() {
                    capture.loss(now, true);
                }
            }
            Record::Loss { module_id } => {
                if let Some(capture) = self.0.lock().unwrap_or_else(|e| e.into_inner()).get(&module_id) {
                    capture.loss(now, false);
                }
            }
            Record::Event { module_id, source_node, packet } => {
                if let Ok(capture) = self.capture(&module_id) {
                    capture.accept(source_node, packet, now);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use osg_streams::Delivery;

    const MODULE: &str = "0123456789abcdef0123456789abcdef";

    fn packet(sequence: u32, cursor: u32, sampled_at_ms: u32, payload: Payload) -> Record {
        Record::Event {
            module_id: MODULE.into(),
            source_node: 44,
            packet: Packet {
                node: 3,
                boot_session: 4,
                lease: 5,
                gateway_epoch: 6,
                configuration_generation: 7,
                sequence,
                cursor,
                sampled_at_ms,
                payload,
            },
        }
    }

    fn layout() -> Payload {
        Payload::Layout { contact_count: 1, encoder_count: 1, selector_count: 0, maintained: [0; 16] }
    }

    fn state() -> Payload {
        Payload::State { held: [0; 16], axes: [123; 8], axis_available: 1 }
    }

    #[test]
    fn late_subscription_starts_at_state_without_replaying_edges() {
        let hub = Hub::default();
        let start = Instant::now();
        let capture = hub.capture(MODULE).unwrap();
        let mut reader = capture.subscribe().unwrap();
        assert!(
            matches!(reader.try_recv().unwrap(), Some(Delivery::Snapshot(initial)) if initial.value.0.is_none())
        );
        hub.accept(packet(1, 0, 0, layout()), start);
        hub.accept(packet(2, 0, 0, state()), start);
        assert!(
            matches!(reader.try_recv().unwrap(), Some(Delivery::Event(event)) if event.value.edges.is_empty())
        );
        let now = start + Duration::from_millis(2);
        hub.accept(
            packet(
                3,
                2,
                2,
                Payload::Edges(vec![
                    stm32_panel_wire::Edge { control: 0, pressed: true, captured_at_ms: 1 },
                    stm32_panel_wire::Edge { control: 1, pressed: true, captured_at_ms: 2 },
                ]),
            ),
            now,
        );
        let Some(Delivery::Event(event)) = reader.try_recv().unwrap() else { panic!("missing edge batch") };
        assert_eq!(event.value.edges.len(), 2);
        assert_eq!(event.value.edges[0].captured_at_ms, 1);
        assert_eq!(event.value.latest.state.held[0], 1);
        assert_eq!(event.value.latest.state_sampled_at_ms, 0);
        assert_eq!(event.value.latest.route.source_node, 44);
        let mut late = capture.subscribe().unwrap();
        assert!(matches!(late.try_recv().unwrap(), Some(Delivery::Snapshot(snapshot))
            if snapshot.value.0 == Some(event.value.latest)));
        assert!(late.try_recv().unwrap().is_none());
    }

    #[test]
    fn expiry_and_recovery_require_new_sample_and_never_replay() {
        let hub = Hub::default();
        let start = Instant::now();
        let capture = hub.capture(MODULE).unwrap();
        hub.accept(packet(1, 0, 100, layout()), start);
        hub.accept(packet(2, 0, 100, state()), start);
        let mut reader = capture.subscribe().unwrap();
        assert!(matches!(reader.try_recv().unwrap(), Some(Delivery::Snapshot(_))));
        hub.accept(packet(3, 0, 100, state()), start + Duration::from_millis(100));
        assert_eq!(capture.deadline(), Some(start + FRESHNESS));
        capture.expire(start + FRESHNESS);
        assert!(matches!(reader.try_recv().unwrap(), Some(Delivery::Loss(_))));
        assert!(
            matches!(reader.try_recv().unwrap(), Some(Delivery::Snapshot(snapshot)) if snapshot.value.0.is_none())
        );
        hub.accept(packet(4, 0, 100, state()), start + FRESHNESS);
        assert!(reader.try_recv().unwrap().is_none());
        hub.accept(packet(5, 3, 350, state()), start + FRESHNESS);
        assert!(matches!(reader.try_recv().unwrap(), Some(Delivery::Event(event))
            if event.value.latest.cursor == 3 && event.value.edges.is_empty()));
        hub.accept(Record::Reset, start + FRESHNESS);
        assert!(matches!(reader.try_recv().unwrap(), Some(Delivery::Loss(_))));
    }
}
