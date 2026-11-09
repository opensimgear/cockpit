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
/// Ephemeral native registration and loss epoch. Capture with the source shadow,
/// never replace this proof with a newer epoch while dispatching queued work.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopeAdmission {
    pub registration: Counter,
    pub epoch: Counter,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopeReceipt {
    pub admission: ScopeAdmission,
    pub live: bool,
    pub eligible: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct ScopeEvidence {
    pub loss_epoch: u64,
    pub eligible: bool,
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
    /// Native release may run while the parent commits the successful down ACK.
    DeliverGuarded { delivery: Delivery, release_after_us: Counter },
    DeliverKeyPair { deliveries: Box<[Delivery; 2]> },
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
    #[serde(deserialize_with = "required_admission")]
    pub admission: Option<ScopeAdmission>,
    pub command: Command,
}
fn required_admission<'de, D: serde::Deserializer<'de>>(deserializer: D)
    -> Result<Option<ScopeAdmission>, D::Error> {
    Option::deserialize(deserializer)
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
    /// Scope rejection is a healthy pause, distinct from an authored deadline.
    pub scope_rejected: bool,
    pub scopes: Vec<ScopeReceipt>,
    #[serde(default)]
    pub collision: bool,
    pub error: Option<String>,
    pub held: Vec<Control>,
    /// Exact channel sequences of release guards fired since the preceding ACK.
    pub released_guards: Vec<Counter>,
}

impl Command {
    pub(crate) fn delivery(&self) -> Option<&Delivery> {
        match self {
            Self::Deliver { delivery } | Self::DeliverGuarded { delivery, .. } => Some(delivery),
            _ => None,
        }
    }
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
    /// Recording adapters may sample their explicit test state. The native
    /// adapter overrides these methods with scoped notification evidence.
    fn bind_scope(&mut self, _registration: u64, scope: &Scope) -> Result<ScopeEvidence, String> {
        self.scope_evidence(_registration, scope)
    }
    fn scope_evidence(&mut self, _registration: u64, scope: &Scope) -> Result<ScopeEvidence, String> {
        Ok(ScopeEvidence { loss_epoch: 0, eligible: self.eligible(scope)? })
    }
    fn retire_scope(&mut self, _registration: u64) {}
    fn set_scope_wake(&mut self, _wake: std::sync::Arc<dyn Fn() + Send + Sync>) {}
    fn notification_driven(&self) -> bool { false }
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
