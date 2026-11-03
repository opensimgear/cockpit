use super::*;
use osg_streams::RetainedBytes;

#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub route: UsbRouteToken,
    pub stream: u32,
    /// Current metadata only; never a historical `events read` page.
    pub state: Option<Arc<Value>>,
    /// RX arrival of the last advancing device sample, never admission time.
    pub received_at: Option<Instant>,
    pub ready: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Edge {
    pub sequence: u32,
    pub control: u8,
    pub pressed: bool,
    pub captured_at_ms: u32,
    pub received_at: Instant,
}

#[derive(Clone, Debug)]
pub(crate) struct Batch {
    pub events: Vec<Edge>,
    /// Exact RX witness of the last advancing sample, as in Snapshot.
    /// Typed consumers must not reconstruct this Instant from wire microseconds.
    pub witnessed_at: Instant,
    /// Complete source page's envelope, with metadata-only `state`.
    pub latest: push::Frame,
}

// Include backing capacities and conservative BTreeMap node/Arc overhead.
// This is in-memory accounting, without encoding on the receive path.
fn value_bytes(value: &Value) -> usize {
    let backing = match value {
        Value::String(s) => s.capacity(),
        Value::Array(a) => a.capacity() * std::mem::size_of::<Value>()
            + a.iter().map(|v| value_bytes(v) - std::mem::size_of::<Value>()).sum::<usize>(),
        Value::Object(o) => o.iter().map(|(key, value)| {
            key.capacity() + std::mem::size_of::<String>() + 128 + value_bytes(value)
        }).sum(),
        _ => 0,
    };
    std::mem::size_of::<Value>() + backing
}

impl RetainedBytes for Snapshot {
    fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.route.module_id.capacity()
            + self.route.serial.capacity() + self.error.as_ref().map_or(0, String::capacity)
            + self.state.as_ref().map_or(0, |v| 32 + value_bytes(v))
    }
}
impl RetainedBytes for Batch {
    fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.events.capacity() * std::mem::size_of::<Edge>()
            + self.latest.reason.as_ref().map_or(0, String::capacity)
            + 32 + value_bytes(&self.latest.state)
    }
}

pub(super) fn metadata(frame: &mut push::Frame) {
    let mut value = (*frame.state).clone();
    if let Some(object) = value.as_object_mut() {
        for key in ["events", "after", "next", "more"] { object.remove(key); }
    }
    frame.state = Arc::new(value);
}
