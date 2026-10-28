use osg_mapping::{
    desktop::{Control, Delivery},
    engine::{typed::Token, Counter, RunToken},
};
use serde::{Deserialize, Serialize};
pub const FRAME_LIMIT: usize = 65_536;
pub const MAX_BATCH: usize = 64;
pub const MAX_OWNERS: usize = 1024;
pub const LEASE_US: u64 = 250_000;
pub const PROTOCOL: u32 = 1;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessIdentity {
    pub executable: String,
    pub pid: u32,
    pub created: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Scope {
    Application { identity: ProcessIdentity },
    DesktopWide,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum NativeEvent {
    Key {
        usage: u16,
        down: bool,
        repeat: bool,
    },
    MouseButton {
        button: osg_mapping::desktop::MouseButton,
        down: bool,
    },
    Pointer {
        dx: i32,
        dy: i32,
    },
    Wheel {
        horizontal: i32,
        vertical: i32,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Command {
    Activate { run: RunToken, scope: Scope },
    Arm { producer: Token, action_id: String },
    Deliver { delivery: Delivery },
    DeliverPointerBatch { deliveries: Vec<Delivery> },
    Renew { run: RunToken },
    Pause { run: RunToken },
    Recover { controls: Vec<Control> },
    Stop,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub sequence: Counter,
    pub deadline_us: Option<Counter>,
    pub command: Command,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ack {
    pub sequence: Counter,
    pub operation_id: Option<Counter>,
    pub pointer_batch: Option<PointerBatchAck>,
    pub ok: bool,
    pub attempted: usize,
    pub confirmed: usize,
    pub uncertain: bool,
    pub cleanup_complete: bool,
    pub skipped: bool,
    #[serde(default)]
    pub collision: bool,
    pub error: Option<String>,
    pub held: Vec<Control>,
}
/// Exact ordered logical participants and the single physical vector they attest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PointerBatchAck {
    pub operation_ids: Vec<Counter>,
    pub dx: i32,
    pub dy: i32,
}
pub trait Adapter {
    /// Check foreground identity and interactive desktop immediately before submission.
    fn eligible(&mut self, scope: &Scope) -> Result<bool, String>;
    /// Return the exact SendInput count. A short count is an uncertain failure.
    fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String>;
}
pub fn native_count(events: &[NativeEvent]) -> usize {
    events
        .iter()
        .map(|e| match e {
            NativeEvent::Wheel {
                horizontal,
                vertical,
            } => usize::from(*horizontal != 0) + usize::from(*vertical != 0),
            _ => 1,
        })
        .sum()
}
