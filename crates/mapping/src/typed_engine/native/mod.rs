//! Portable native acquisition evidence on the pinned host journal clock.
use super::{Counter, Frame, SignalSample};
use serde::{Deserialize, Serialize};

pub const MAX_NATIVE_CAPTURES: usize = 65_536;
pub const MAX_NATIVE_LEASES: usize = 64;
pub const MAX_NATIVE_REFERENCES: usize = 1_048_576;
pub const MAX_NATIVE_FRAME_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_ADMISSION_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_NATIVE_ID_BYTES: usize = 512;
pub const MAX_NATIVE_VALIDITY_US: u64 = 11_000_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "frame",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum Admission {
    Direct(Frame),
    Native(NativeFrame),
}
impl From<Frame> for Admission {
    fn from(frame: Frame) -> Self {
        Self::Direct(frame)
    }
}
impl From<NativeFrame> for Admission {
    fn from(frame: NativeFrame) -> Self {
        Self::Native(frame)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativePolicy {
    Sampled,
    Latched,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativePortRegistration {
    pub port_id: String,
    pub family: String,
    pub policy: NativePolicy,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeSourceRegistration {
    pub source: String,
    pub clock_id: String,
    pub ports: Vec<NativePortRegistration>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeCapture {
    pub producer_id: String,
    pub generation: Counter,
    pub sequence: Counter,
    pub byte_offset: Option<u32>,
    pub captured_us: Counter,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeLease {
    pub domain_id: String,
    pub generation: Counter,
    pub observation: u32,
    pub valid_until_us: Counter,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativeInput {
    Byte(u16),
    Channel(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum NativeValidity {
    Sampled { valid_until_us: Counter },
    Latched { lease: u32 },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeContributor {
    pub input: NativeInput,
    pub capture: u32,
    pub validity: NativeValidity,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativeChangeKind {
    Baseline,
    Update,
    Withdraw,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeChange {
    pub port_id: String,
    pub continuity: Counter,
    pub kind: NativeChangeKind,
    pub sample: SignalSample,
    pub contributors: Vec<NativeContributor>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeFrame {
    pub source: String,
    pub generation: Counter,
    pub sequence: Counter,
    pub clock_id: String,
    pub captures: Vec<NativeCapture>,
    pub leases: Vec<NativeLease>,
    pub changes: Vec<NativeChange>,
}

mod admission;
mod budget;
mod direct_apply;
mod direct_batch;
mod fences;
mod ledger;
mod proof;
mod recovery;
mod retained_bytes;
mod revocations;
mod validate;
pub(super) use ledger::Ledger;
pub use proof::NativeProof;

#[cfg(test)]
mod tests;

mod desktop;
mod graph;
mod liveness;
mod output_work;
mod publish;
mod scripts;
