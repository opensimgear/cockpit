//! Wire contracts only. User code never executes inside the pure reducer.
use crate::{
    engine::{Counter, ProducerToken},
    profile::{identifier, Input, Kind, Output, Profile, Value},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
pub const IPC_LIMIT: usize = 32_768;
pub const STATE_LIMIT: usize = 16_384;
pub const QUEUE_LIMIT: usize = 32;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutputBinding {
    pub id: String,
    pub output: Output,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Script {
    pub id: String,
    pub source: String,
    pub inputs: Vec<Input>,
    pub outputs: Vec<OutputBinding>,
    #[serde(default = "empty")]
    pub parameters: serde_json::Value,
}
pub fn empty() -> serde_json::Value {
    serde_json::json!({})
}
impl Script {
    pub fn owner(&self, output: &str) -> String {
        format!("{}:{output}", self.id)
    }
    pub fn validate(&self, profile: &Profile) -> Result<BTreeSet<String>, String> {
        if !identifier(&self.id)
            || self.source.is_empty()
            || self.source.len() > 16_384
            || self.inputs.is_empty()
            || self.inputs.len() > 32
            || self.outputs.is_empty()
            || self.outputs.len() > 128
            || !self.parameters.is_object()
            || self.parameters.to_string().len() > 4096
        {
            return Err("A script needs an ID, at most 16 KiB of code, 1–32 declared inputs, 1–128 outputs and at most 4 KiB of parameters.".into());
        }
        let mut roles = BTreeSet::new();
        let mut inputs = BTreeSet::new();
        for input in &self.inputs {
            let Input::Source { role, control } = input else {
                return Err("Scripts declare source role/control inputs explicitly.".into());
            };
            if !profile
                .roles
                .iter()
                .any(|r| r.id == *role && r.controls.iter().any(|c| c.id == *control))
                || !inputs.insert((role, control))
            {
                return Err("Unknown or duplicate script input.".into());
            }
            roles.insert(role.clone());
        }
        let mut ids = BTreeSet::new();
        for output in &self.outputs {
            if !identifier(&output.id)
                || !ids.insert(&output.id)
                || self.output_kind(profile, &output.id).is_none()
            {
                return Err("Unknown or duplicate script output.".into());
            }
        }
        Ok(roles)
    }
    pub fn output_kind(&self, profile: &Profile, id: &str) -> Option<Kind> {
        let output = &self.outputs.iter().find(|o| o.id == id)?.output;
        profile
            .targets
            .iter()
            .find(|t| t.id == output.target)?
            .layout
            .kind(&output.control)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ticket {
    pub producer: ProducerToken,
    pub epoch: Counter,
    pub sequence: Counter,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Invocation {
    pub ticket: Ticket,
    pub callback: String,
    pub at_us: Counter,
    pub inputs: BTreeMap<String, BTreeMap<String, Value>>,
    pub event: serde_json::Value,
    pub parameters: serde_json::Value,
    pub state: serde_json::Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Intent {
    pub output: String,
    pub value: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Timer {
    pub id: String,
    pub after_us: u64,
}
/// Every callback replaces its whole state, output contribution and timer set atomically.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Transaction {
    pub state: serde_json::Value,
    pub outputs: Vec<Intent>,
    pub timers: Vec<Timer>,
    #[serde(default)]
    pub logs: Vec<String>,
}
impl Transaction {
    pub fn validate(&self, script: &Script, profile: &Profile) -> Result<(), String> {
        if !self.state.is_object()
            || self.state.to_string().len() > STATE_LIMIT
            || self.outputs.len() > 128
            || self.timers.len() > 64
            || self.logs.len() > 4
            || self.logs.iter().any(|s| s.len() > 256)
            || serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > IPC_LIMIT
        {
            return Err("Script transaction exceeds a state/output/timer/log budget.".into());
        }
        let mut outputs = BTreeSet::new();
        let mut timers = BTreeSet::new();
        if self.outputs.iter().any(|i| {
            !outputs.insert(&i.output)
                || !script
                    .output_kind(profile, &i.output)
                    .is_some_and(|k| k.accepts(i.value))
        }) || self.timers.iter().any(|t| {
            !identifier(&t.id)
                || !timers.insert(&t.id)
                || !(1_000..=60_000_000).contains(&t.after_us)
        }) {
            return Err("Invalid or duplicate script intent or timer.".into());
        }
        Ok(())
    }
}
#[derive(Default)]
pub(crate) struct Instance {
    pub epoch: u64,
    pub sequence: u64,
    pub started: bool,
    pub fault: Option<String>,
    pub state: serde_json::Value,
    pub pending: VecDeque<Invocation>,
    pub inflight: Option<Invocation>,
    pub timers: BTreeMap<(u64, String), ()>,
    pub logs: VecDeque<String>,
    pub log_window: u64,
    pub log_count: usize,
}
