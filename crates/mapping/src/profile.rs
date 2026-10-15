use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub const DOCUMENT_LIMIT: usize = 65_536;
pub const MAX_ROLES: usize = 16;
pub const MAX_TARGETS: usize = 8;
pub const MAX_NODES: usize = 256;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Origin {
    HostController,
    OsgPanelEvents,
    Synthetic,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AxisRange {
    Bipolar,
    Unipolar,
}
impl AxisRange {
    pub fn min(self) -> i32 {
        if self == Self::Bipolar {
            -32768
        } else {
            0
        }
    }
    pub fn contains(self, value: i32) -> bool {
        (self.min()..=32767).contains(&value)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Kind {
    Button,
    Axis { range: AxisRange },
    Hat,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum Value {
    Button(bool),
    Axis(i32),
    Hat(Option<u8>),
}
impl Kind {
    pub fn accepts(self, value: Value) -> bool {
        match (self, value) {
            (Self::Button, Value::Button(_)) => true,
            (Self::Axis { range }, Value::Axis(value)) => range.contains(value),
            (Self::Hat, Value::Hat(value)) => value.is_none_or(|v| v < 8),
            _ => false,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Control {
    pub id: String,
    pub kind: Kind,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Role {
    pub id: String,
    pub name: String,
    pub origin: Origin,
    pub controls: Vec<Control>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Axis {
    pub range: AxisRange,
    pub fallback: i32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Layout {
    pub buttons: u16,
    pub axes: Vec<Axis>,
    pub hats: u8,
}
impl Layout {
    pub fn validate(&self) -> Result<(), String> {
        if self.buttons > 128
            || self.axes.len() > 16
            || self.hats > 4
            || (self.buttons == 0 && self.axes.is_empty() && self.hats == 0)
        {
            return Err("Target layout must contain 1–128 buttons, up to 16 axes and up to four hats, with at least one control.".into());
        }
        if self.axes.iter().any(|a| !a.range.contains(a.fallback)) {
            return Err("An axis fallback is outside its declared range.".into());
        }
        Ok(())
    }
    pub fn kind(&self, control: &OutputControl) -> Option<Kind> {
        match *control {
            OutputControl::Button { index } if index < self.buttons => Some(Kind::Button),
            OutputControl::Axis { index } => self
                .axes
                .get(index as usize)
                .map(|a| Kind::Axis { range: a.range }),
            OutputControl::Hat { index } if index < self.hats as u16 => Some(Kind::Hat),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Target {
    pub id: String,
    pub name: String,
    pub layout: Layout,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum OutputControl {
    Button { index: u16 },
    Axis { index: u16 },
    Hat { index: u16 },
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Output {
    pub target: String,
    pub control: OutputControl,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Input {
    Source { role: String, control: String },
    Node { node: String },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Activation {
    Momentary,
    Level,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Operation {
    Direct { activation: Activation },
    Transform { rule: crate::rules::Rule },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub inputs: Vec<Input>,
    pub output: Option<Output>,
    pub operation: Operation,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Arbitration {
    pub output: Output,
    pub priority: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Visibility {
    Visible { role: String },
    HideAll { role: String },
    HideFromProcess { role: String, application: String },
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Requirements {
    #[serde(default)]
    pub virtual_devices: bool,
    #[serde(default)]
    pub device_hiding: bool,
    #[serde(default)]
    pub visibility: Vec<Visibility>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Profile {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub roles: Vec<Role>,
    pub targets: Vec<Target>,
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub arbitration: Vec<Arbitration>,
    #[serde(default)]
    pub requirements: Requirements,
}
#[derive(Clone, Debug)]
pub struct Compiled {
    pub timer_reserve: usize,
    pub profile: Profile,
    pub order: Vec<usize>,
    pub kinds: BTreeMap<String, Kind>,
    pub dependencies: BTreeMap<String, BTreeSet<String>>,
    pub writers: BTreeMap<Output, Vec<String>>,
}
pub fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}
pub fn label(value: &str) -> bool {
    !value.trim().is_empty() && value.chars().count() <= 80 && !value.chars().any(char::is_control)
}
impl Profile {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > DOCUMENT_LIMIT {
            return Err("Mapping document exceeds 65536 bytes.".into());
        }
        let profile: Self =
            serde_json::from_slice(bytes).map_err(|e| format!("Invalid mapping document: {e}"))?;
        profile.compile()?;
        Ok(profile)
    }
    pub fn export(&self) -> Result<Vec<u8>, String> {
        self.compile()?;
        serde_json::to_vec(self).map_err(|e| e.to_string())
    }
    pub fn compile(&self) -> Result<Compiled, String> {
        if self.schema != 1
            || uuid::Uuid::parse_str(&self.id)
                .map(|id| id.to_string())
                .ok()
                .as_deref()
                != Some(self.id.as_str())
            || !label(&self.name)
        {
            return Err("Mapping schema, UUID or name is invalid.".into());
        }
        if serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > DOCUMENT_LIMIT {
            return Err("Mapping document exceeds 65536 bytes.".into());
        }
        if self.roles.is_empty()
            || self.roles.len() > MAX_ROLES
            || self.targets.is_empty()
            || self.targets.len() > MAX_TARGETS
            || self.nodes.is_empty()
            || self.nodes.len() > MAX_NODES
        {
            return Err("A mapping needs 1–16 roles, 1–8 targets and 1–256 nodes.".into());
        }
        let mut roles = BTreeMap::new();
        for role in &self.roles {
            if !identifier(&role.id)
                || !label(&role.name)
                || role.controls.is_empty()
                || role.controls.len() > 256
                || roles.insert(role.id.clone(), role).is_some()
            {
                return Err("Invalid or duplicate mapping role.".into());
            }
            let mut controls = BTreeSet::new();
            for control in &role.controls {
                if !identifier(&control.id) || !controls.insert(&control.id) {
                    return Err("Invalid or duplicate logical input control.".into());
                }
            }
        }
        let mut targets = BTreeMap::new();
        for target in &self.targets {
            if !identifier(&target.id)
                || !label(&target.name)
                || targets.insert(target.id.clone(), target).is_some()
            {
                return Err("Invalid or duplicate logical target.".into());
            }
            target.layout.validate()?;
        }
        let mut nodes = BTreeMap::new();
        for (index, node) in self.nodes.iter().enumerate() {
            if !identifier(&node.id)
                || nodes.insert(node.id.clone(), index).is_some()
                || node.inputs.is_empty()
                || node.inputs.len() > 8
            {
                return Err("Nodes require a unique ID and one to eight inputs.".into());
            }
        }
        fn visit(
            index: usize,
            profile: &Profile,
            nodes: &BTreeMap<String, usize>,
            marks: &mut [u8],
            order: &mut Vec<usize>,
        ) -> Result<(), String> {
            if marks[index] == 1 {
                return Err("Mapping graph contains a cycle.".into());
            }
            if marks[index] == 2 {
                return Ok(());
            }
            marks[index] = 1;
            for input in &profile.nodes[index].inputs {
                if let Input::Node { node } = input {
                    visit(
                        *nodes.get(node).ok_or("Unknown input node.")?,
                        profile,
                        nodes,
                        marks,
                        order,
                    )?;
                }
            }
            marks[index] = 2;
            order.push(index);
            Ok(())
        }
        let mut order = Vec::new();
        let mut marks = vec![0; self.nodes.len()];
        for index in nodes.values() {
            visit(*index, self, &nodes, &mut marks, &mut order)?;
        }
        let mut kinds = BTreeMap::new();
        let mut dependencies: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut writers: BTreeMap<Output, Vec<String>> = BTreeMap::new();
        for &index in &order {
            let node = &self.nodes[index];
            let mut input_kinds = Vec::new();
            let mut deps = BTreeSet::new();
            for input in &node.inputs {
                let (kind, input_deps) = match input {
                    Input::Source { role, control } => {
                        let role_definition = roles.get(role).ok_or("Unknown source role.")?;
                        let kind = role_definition
                            .controls
                            .iter()
                            .find(|c| c.id == *control)
                            .ok_or("Unknown logical input control.")?
                            .kind;
                        (kind, BTreeSet::from([role.clone()]))
                    }
                    Input::Node { node } => (
                        *kinds.get(node).ok_or("Unresolved input type.")?,
                        dependencies
                            .get(node)
                            .ok_or("Unresolved input dependency.")?
                            .clone(),
                    ),
                };
                input_kinds.push(kind);
                deps.extend(input_deps);
            }
            let kind = match &node.operation {
                Operation::Direct { .. } if input_kinds.len() == 1 => input_kinds[0],
                Operation::Direct { .. } => return Err("Direct mapping requires one input.".into()),
                Operation::Transform { rule } => rule.validate(&input_kinds)?,
            };
            if let Some(output) = &node.output {
                let target = targets
                    .get(&output.target)
                    .ok_or("Unknown output target.")?;
                if target.layout.kind(&output.control) != Some(kind) {
                    return Err(
                        "Direct input/output kinds or axis ranges do not match the target layout."
                            .into(),
                    );
                }
                writers
                    .entry(output.clone())
                    .or_default()
                    .push(node.id.clone());
            }
            kinds.insert(node.id.clone(), kind);
            dependencies.insert(node.id.clone(), deps);
        }
        let timer_reserve = self
            .nodes
            .iter()
            .map(|n| match &n.operation {
                Operation::Transform {
                    rule: crate::rules::Rule::Gesture { .. },
                } => 3,
                Operation::Transform {
                    rule: crate::rules::Rule::Pulse { .. },
                } => 2,
                Operation::Transform {
                    rule: crate::rules::Rule::Macro { steps },
                } => steps.len(),
                _ => 0,
            })
            .sum::<usize>();
        if timer_reserve > 256 {
            return Err("Built-in transformations exceed the shared timer budget.".into());
        }
        let mut explicit = BTreeSet::new();
        for arbitration in &self.arbitration {
            if !explicit.insert(arbitration.output.clone()) {
                return Err("Duplicate output arbitration.".into());
            }
            let actual = writers
                .get_mut(&arbitration.output)
                .ok_or("Arbitration has no writers.")?;
            let proposed: BTreeSet<_> = arbitration.priority.iter().cloned().collect();
            if matches!(arbitration.output.control, OutputControl::Button { .. })
                || proposed.len() != arbitration.priority.len()
                || proposed != actual.iter().cloned().collect()
            {
                return Err(
                    "Axis/hat priority must name each actual writer exactly once; buttons use OR."
                        .into(),
                );
            }
            *actual = arbitration.priority.clone();
        }
        for (output, owners) in &writers {
            if owners.len() > 1
                && !matches!(output.control, OutputControl::Button { .. })
                && !explicit.contains(output)
            {
                return Err("Ambiguous axis/hat writers require explicit priority.".into());
            }
        }
        if self.requirements.visibility.len() > MAX_ROLES {
            return Err("Too many visibility selections.".into());
        }
        let mut selected = BTreeSet::new();
        for visibility in &self.requirements.visibility {
            let (role, hidden) = match visibility {
                Visibility::Visible { role } => (role, false),
                Visibility::HideAll { role } => (role, true),
                Visibility::HideFromProcess { role, application } => {
                    if !identifier(application) {
                        return Err(
                            "Visibility uses a logical application ID, not an executable path."
                                .into(),
                        );
                    }
                    (role, true)
                }
            };
            let definition = roles.get(role).ok_or("Unknown visibility role.")?;
            if !selected.insert(role)
                || definition.origin != Origin::HostController
                || (hidden && !self.requirements.device_hiding)
            {
                return Err("Visibility requires a unique physical controller role and declared optional hiding.".into());
            }
        }
        Ok(Compiled {
            timer_reserve,
            profile: self.clone(),
            order,
            kinds,
            dependencies,
            writers,
        })
    }
}
#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;
