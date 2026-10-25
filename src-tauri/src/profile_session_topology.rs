//! Immutable applied graph metadata. This projection cannot advance execution,
//! drain effects, expose script code/tokens or select the current editor draft.
use super::{Coordinator, Host, OwnedRun, RunOwner};
use osg_mapping::{engine::typed::Phase, program::Operation};
use osg_profiles::{session::SnapshotSummary, Binding, ProfileDocument, ResourceKind};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Section {
    Nodes,
    Edges,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Request {
    command: String,
    pub owner: RunOwner,
    section: Section,
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_limit")]
    limit: usize,
}
fn default_limit() -> usize {
    32
}
impl Request {
    pub(crate) fn parse(value: &serde_json::Value) -> Result<Self, String> {
        let request: Self = serde_json::from_value(value.clone())
            .map_err(|error| format!("Invalid applied topology request: {error}"))?;
        let uuid =
            |value: &str| uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value);
        if request.command != "profile_runtime_topology"
            || request.offset > 512
            || !(1..=64).contains(&request.limit)
            || !uuid(&request.owner.session)
            || !uuid(&request.owner.profile_id)
            || request.owner.generation == 0
            || request.owner.generation > osg_profiles::MAX_REVISION
            || request.owner.profile_revision == 0
            || request.owner.profile_revision > osg_profiles::MAX_REVISION
            || request.owner.game_id.as_deref().is_some_and(|id| !osg_profiles::activation::valid_game_id(id))
        {
            return Err("Choose an exact applied owner and bounded topology page.".into());
        }
        Ok(request)
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum NodeKind {
    Desktop,
    Device,
    Companion,
    Game,
    Processing,
    Virtual,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Node {
    id: String,
    name: String,
    kind: NodeKind,
    summary: String,
}
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum Items {
    Nodes(Vec<Node>),
    Edges(Vec<Binding>),
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Page {
    schema_version: u32,
    owner: RunOwner,
    applied_snapshot: SnapshotSummary,
    program_hash: String,
    captured_us: String,
    section: Section,
    offset: usize,
    total: usize,
    items: Items,
}
impl<H: Host> Coordinator<H> {
    pub(crate) fn inspect_topology(&self, request: &Request) -> Result<Page, String> {
        if !self.validates(&request.owner) {
            return Err(
                "The applied topology owner expired; inspect the current native run.".into(),
            );
        }
        let run = self
            .manifest
            .active
            .as_ref()
            .filter(|run| run.owner == request.owner)
            .ok_or("The applied topology no longer has a matching native manifest.")?;
        self.host.inspect_topology(run, request)
    }
}
fn number(value: f64) -> String {
    if value != 0.0 && (value.abs() >= 1e9 || value.abs() < 1e-6) {
        format!("{value:.6e}")
    } else {
        value.to_string()
    }
}
fn scalar(value: &osg_profiles::PortValue) -> String {
    match value {
        osg_profiles::PortValue::Boolean(value) => value.to_string(),
        osg_profiles::PortValue::Number(value) => number(*value),
        osg_profiles::PortValue::Text(value) => format!("text ({} bytes)", value.len()),
    }
}
fn bounded_summary(mut value: String) -> String {
    if value.len() > 255 {
        let mut end = 252;
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        value.truncate(end);
        value.push('…');
    }
    value
}
fn builtin_summary(rule: &osg_mapping::rules::Rule, inputs: &[String]) -> String {
    use osg_mapping::rules::{Gesture, Rule};
    let time = |value: u32| {
        if value >= 1000 {
            format!("{} ms", number(f64::from(value) / 1000.0))
        } else {
            format!("{value} µs")
        }
    };
    let parameters = match rule {
        Rule::Axis {
            dead_zone,
            invert,
            scale,
            curve,
        } => {
            let curve = if curve.is_empty() {
                "linear".into()
            } else {
                format!(
                    "{} points [{}]",
                    curve.len(),
                    curve
                        .iter()
                        .map(|p| format!("{}→{}", p.input, p.output))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            format!(
                "axis · deadzone {dead_zone} · {} · scale {scale} · curve {curve}",
                if *invert { "inverted" } else { "normal" }
            )
        }
        Rule::Split { positive } => format!(
            "split · {} half",
            if *positive { "positive" } else { "negative" }
        ),
        Rule::ButtonAxis {
            range,
            pressed,
            released,
        } => format!(
            "button-axis · {} · pressed {pressed}, released {released}",
            format!("{range:?}").to_lowercase()
        ),
        Rule::Threshold { press, release } => {
            format!("threshold · press {press}, release {release}")
        }
        Rule::Gesture {
            gesture,
            hold_us,
            double_us,
            pulse_us,
        } => {
            let gesture = match gesture {
                Gesture::Tap => "tap",
                Gesture::Hold => "hold",
                Gesture::DoubleTap => "double-tap",
            };
            format!(
                "{gesture} · hold {} · double {} · pulse {}",
                time(*hold_us),
                time(*double_us),
                time(*pulse_us)
            )
        }
        Rule::Pulse {
            duration_us,
            repeat_us,
        } => format!(
            "pulse · duration {} · {}",
            time(*duration_us),
            if *repeat_us > 0 {
                format!("repeat {}", time(*repeat_us))
            } else {
                "no repeat".into()
            }
        ),
        Rule::Macro { steps } => format!(
            "macro · {} steps · duration {} · {}",
            steps.len(),
            time(steps.last().map_or(0, |step| step.after_us)),
            steps
                .iter()
                .map(|step| format!(
                    "{} {}",
                    time(step.after_us),
                    if step.pressed { "on" } else { "off" }
                ))
                .collect::<Vec<_>>()
                .join(" → ")
        ),
        Rule::Merge => "merge".into(),
        Rule::Chord => "chord".into(),
        Rule::Gate => "gate".into(),
        Rule::Toggle => "toggle".into(),
    };
    bounded_summary(format!(
        "{parameters} · explicit inputs: {}",
        inputs.join(", ")
    ))
}
fn summary(operation: &Operation) -> String {
    match operation {
        Operation::Identity{}=>"Identity".into(),Operation::Not{}=>"Boolean NOT".into(),Operation::Invert{}=>"Invert numeric value".into(),
        Operation::DeclareQuantity{quantity,unit}=>bounded_summary(format!("Quantity {quantity} [{unit}]")),
        Operation::Gate{}=>"Explicit gate".into(),Operation::Any{}=>"Boolean OR".into(),Operation::All{}=>"Boolean AND".into(),
        Operation::EventToState{}=>"Event to state".into(),Operation::Constant{value}=>format!("Constant {}",scalar(value)),
        Operation::HatDirection{direction,diagonals}=>format!("Hat {direction:?} · {diagonals:?}"),
        Operation::Scale{factor,offset}=>format!("Scale × {} + {}",number(*factor),number(*offset)),
        Operation::RangeMap{input_min,input_max,output_min,output_max,rounding}=>format!("Map {}…{} → {}…{} · {rounding:?}",number(*input_min),number(*input_max),number(*output_min),number(*output_max)),
        Operation::UnitConvert{from,to,rounding,clamp}=>format!("Units {from} → {to} · {rounding:?}{}", if *clamp { " · clamp to output range" } else { "" }),
        Operation::Deadzone{center_percent,deadzone_percent,rounding}=>format!("Deadzone center {center_percent}%, width {deadzone_percent}% · {rounding:?}"),
        Operation::Curve{points,rounding}=>format!("Curve · {} explicit points · {rounding:?}",points.len()),
        Operation::Compare{comparison,threshold}=>format!("Compare {comparison:?} {}",number(*threshold)),
        Operation::Hysteresis{press,release,comparison}=>format!("Hysteresis {comparison:?} · press {}, release {}",number(*press),number(*release)),
        Operation::Priority{order}=>format!("Priority · {} ordered inputs",order.len()),
        Operation::EventEdge{edge}=>format!("State to {edge:?} event"),
        Operation::Select{when_true,when_false}=>format!("Choose {} / {}",scalar(when_true),scalar(when_false)),
        Operation::Format{decimals,prefix,suffix,rounding,overflow}=>format!("Format {decimals} decimals · {rounding:?} · {overflow:?} · prefix/suffix {} / {} bytes",prefix.len(),suffix.len()),
        Operation::Fallback{..}=>"Explicit unknown / stale / unavailable fallback".into(),
        Operation::ActionPack{arguments}=>format!("Event-triggered action pack · {} ordered arguments",arguments.len()),
        Operation::SetOnChange{arguments}=>format!("Setter on argument change · {} ordered arguments",arguments.len()),
        Operation::Builtin{rule,inputs}=>builtin_summary(rule,inputs),
        Operation::Clamp{min,max}=>format!("Clamp {}…{}",number(*min),number(*max)),
        Operation::Script{..}=>"Isolated script · declared ports only; code is not exposed".into(),
    }
}
pub(crate) fn capture(
    document: &ProfileDocument,
    phase: &Phase,
    run: &OwnedRun,
    request: &Request,
) -> Result<Page, String> {
    capture_variant(document,document,phase,run,request)
}
pub(crate) fn capture_variant(
    document: &ProfileDocument,
    source: &ProfileDocument,
    phase: &Phase,
    run: &OwnedRun,
    request: &Request,
) -> Result<Page, String> {
    let token = phase.run_token();
    if request.owner != run.owner
        || token.session != run.owner.session
        || token.generation.0 != run.owner.generation
        || document.id != run.owner.profile_id
        || document.revision != run.owner.profile_revision
        || run.snapshot.profile_id != document.id
        || run.snapshot.revision != document.revision
        || source.id != document.id || source.revision != document.revision
        || osg_profiles::semantic_hash(source)? != run.snapshot.semantic_hash
        || !phase.active()
    {
        return Err("The topology does not belong to this exact immutable applied reducer.".into());
    }
    let (total, items) = match request.section {
        Section::Nodes => {
            let mut nodes = document
                .resources
                .iter()
                .map(|node| Node {
                    id: node.id.clone(),
                    name: node.name.clone(),
                    kind: match node.kind {
                        ResourceKind::Device => NodeKind::Device,
                        ResourceKind::Companion => NodeKind::Companion,
                        ResourceKind::Game => NodeKind::Game,
                    },
                    summary: if node.required {
                        "Required resource"
                    } else {
                        "Optional resource"
                    }
                    .into(),
                })
                .chain(document.processing_nodes.iter().map(|node| Node {
                    id: node.id.clone(),
                    name: node.name.clone(),
                    kind: NodeKind::Processing,
                    summary: summary(&node.operation),
                }))
                .chain(document.virtual_outputs.iter().map(|node| Node {
                    id: node.id.clone(),
                    name: node.name.clone(),
                    kind: NodeKind::Virtual,
                    summary: format!(
                        "{} buttons · {} axes · {} hats",
                        node.layout.buttons.len(),
                        node.layout.axes.len(),
                        node.layout.hats.len()
                    ),
                }))
                .chain(document.desktop_outputs.iter().map(|node| Node {
                    id: node.id.clone(), name: node.name.clone(), kind: NodeKind::Desktop,
                    summary: format!("{} exact keyboard and mouse actions", node.actions.len()),
                }))
                .collect::<Vec<_>>();
            nodes.sort_by(|a, b| a.id.cmp(&b.id));
            if nodes.len() > 336 || nodes.iter().any(|node| node.summary.len() > 256) {
                return Err("The applied topology exceeds its bounded metadata budget.".into());
            }
            (
                nodes.len(),
                Items::Nodes(
                    nodes
                        .into_iter()
                        .skip(request.offset)
                        .take(request.limit)
                        .collect(),
                ),
            )
        }
        Section::Edges => {
            let mut edges = document.bindings.clone();
            edges.sort_by(|a, b| a.id.cmp(&b.id));
            if edges.len() > 512 {
                return Err("The applied topology exceeds its bounded edge budget.".into());
            }
            (
                edges.len(),
                Items::Edges(
                    edges
                        .into_iter()
                        .skip(request.offset)
                        .take(request.limit)
                        .collect(),
                ),
            )
        }
    };
    if request.offset > total {
        return Err("Choose an available applied topology page.".into());
    }
    Ok(Page {
        schema_version: 1,
        owner: run.owner.clone(),
        applied_snapshot: run.snapshot.clone(),
        program_hash: run.program_hash.clone(),
        captured_us: phase.at_us().0.to_string(),
        section: request.section,
        offset: request.offset,
        total,
        items,
    })
}

#[cfg(test)]
#[path = "profile_session_topology_tests.rs"]
mod tests;
