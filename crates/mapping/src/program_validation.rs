use super::*;
use crate::signal::{ActionSemantics, Direction, ScalarSchema};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug)]
pub struct CompiledProgram {
    pub program: Program,
    pub order: Vec<usize>,
    pub contracts: BTreeMap<Endpoint, PortContract>,
    pub dependencies: BTreeMap<String, BTreeSet<Endpoint>>,
    pub hash: String,
    pub timer_reserve: usize,
}
pub fn compatible(from: &PortContract, to: &PortContract) -> bool {
    from.can_bind_to(to)
}
pub(crate) fn schema(contract: &PortContract) -> Option<&ScalarSchema> {
    match contract {
        PortContract::State { schema, .. } | PortContract::Event { schema } => Some(schema),
        _ => None,
    }
}
pub(crate) fn numeric(schema: &ScalarSchema) -> Option<(f64, f64, Option<&str>)> {
    match schema {
        ScalarSchema::Integer { min, max, unit } => {
            Some((*min as f64, *max as f64, unit.as_deref()))
        }
        ScalarSchema::Number { min, max, unit } => Some((*min, *max, unit.as_deref())),
        _ => None,
    }
}
fn port_id(value: &str) -> Result<(), String> {
    crate::signal::text(value, 256)
}
impl ControllerLayout {
    pub fn validate(&self) -> Result<(), String> {
        if self.buttons.len() > 128
            || self.axes.len() > 16
            || self.hats.len() > 4
            || self.buttons.len() + self.axes.len() + self.hats.len() == 0
        {
            return Err(
                "A virtual layout needs controls within 128 buttons, 16 axes and four hats.".into(),
            );
        }
        let mut ids = BTreeSet::new();
        for id in self
            .buttons
            .iter()
            .chain(self.axes.iter().map(|axis| &axis.port_id))
            .chain(&self.hats)
        {
            port_id(id)?;
            if !ids.insert(id) {
                return Err("Duplicate virtual layout port.".into());
            }
        }
        if self
            .axes
            .iter()
            .any(|axis| !axis.range.contains(axis.fallback))
        {
            return Err("Virtual axis fallback exceeds its declared range.".into());
        }
        Ok(())
    }
    pub fn virtual_port_contract(&self, id: &str) -> Option<PortContract> {
        virtual_port_contract(self, id)
    }
}
pub fn virtual_port_contract(layout: &ControllerLayout, id: &str) -> Option<PortContract> {
    let (schema, fallback) = if layout.buttons.iter().any(|port| port == id) {
        (ScalarSchema::Boolean {}, PortValue::Boolean(false))
    } else if let Some(axis) = layout.axes.iter().find(|axis| axis.port_id == id) {
        (
            ScalarSchema::Integer {
                min: axis.range.min() as i64,
                max: 32767,
                unit: None,
            },
            PortValue::Number(axis.fallback as f64),
        )
    } else if layout.hats.iter().any(|port| port == id) {
        (
            ScalarSchema::Integer {
                min: -1,
                max: 7,
                unit: None,
            },
            PortValue::Number(-1.0),
        )
    } else {
        return None;
    };
    Some(PortContract::State {
        schema,
        freshness_ms: 1000,
        fallback: Some(fallback),
    })
}
fn kind(schema: &ScalarSchema) -> Option<crate::profile::Kind> {
    use crate::profile::Kind;
    match schema {
        ScalarSchema::Boolean {} => Some(Kind::Button),
        ScalarSchema::Integer {
            min: 0,
            max: 32767,
            unit: None,
        } => Some(Kind::Axis {
            range: AxisRange::Unipolar,
        }),
        ScalarSchema::Integer {
            min: -32768,
            max: 32767,
            unit: None,
        } => Some(Kind::Axis {
            range: AxisRange::Bipolar,
        }),
        ScalarSchema::Integer {
            min: -1,
            max: 7,
            unit: None,
        } => Some(Kind::Hat),
        _ => None,
    }
}
pub(crate) fn controller_kind(contract: &PortContract) -> Option<crate::profile::Kind> {
    kind(schema(contract)?)
}
pub(crate) fn conversion(from: &str, to: &str) -> Option<(f64, f64)> {
    match (from, to) {
        ("m", "ft") => Some((1250.0 / 381.0, 0.0)),
        ("ft", "m") => Some((381.0 / 1250.0, 0.0)),
        ("m", "km") => Some((0.001, 0.0)),
        ("km", "m") => Some((1000.0, 0.0)),
        ("s", "ms") => Some((1000.0, 0.0)),
        ("ms", "s") => Some((0.001, 0.0)),
        ("m/s", "kn") => Some((900.0 / 463.0, 0.0)),
        ("kn", "m/s") => Some((463.0 / 900.0, 0.0)),
        ("deg", "rad") => Some((std::f64::consts::PI / 180.0, 0.0)),
        ("rad", "deg") => Some((180.0 / std::f64::consts::PI, 0.0)),
        ("degC", "degF") => Some((1.8, 32.0)),
        ("degF", "degC") => Some((1.0 / 1.8, -32.0 / 1.8)),
        _ if from == to => Some((1.0, 0.0)),
        _ => None,
    }
}
fn finite(values: &[f64]) -> bool {
    values
        .iter()
        .all(|value| value.is_finite() && value.abs() <= crate::signal::MAX_SAFE_INTEGER as f64)
}
pub fn validate_operation(
    operation: &Operation,
    inputs: &[PortDescriptor],
    outputs: &[PortDescriptor],
) -> Result<(), String> {
    let fail =
        || Err("Invalid explicit operation ports, types, units, ranges or parameters.".into());
    if inputs.len() > 128
        || outputs.is_empty()
        || outputs.len() > 16
        || inputs.iter().any(|p| p.direction != Direction::Input)
        || outputs.iter().any(|p| p.direction != Direction::Output)
    {
        return fail();
    }
    let mut port_ids = BTreeSet::new();
    for port in inputs.iter().chain(outputs) {
        port_id(&port.id)?;
        crate::signal::text(&port.name, 320)?;
        if !port_ids.insert(&port.id) {
            return fail();
        }
        port.contract.validate_graph(port.direction)?;
    }
    let input = inputs.first().and_then(|p| schema(&p.contract));
    let output = outputs.first().and_then(|p| schema(&p.contract));
    let one = inputs.len() == 1 && outputs.len() == 1;
    let same_delivery = one
        && std::mem::discriminant(&inputs[0].contract)
            == std::mem::discriminant(&outputs[0].contract);
    let same = one
        && compatible(&inputs[0].contract, &outputs[0].contract)
        && compatible(&outputs[0].contract, &inputs[0].contract);
    let bool_inputs = inputs
        .iter()
        .all(|p| matches!(schema(&p.contract), Some(ScalarSchema::Boolean {})));
    match operation {
        Operation::Constant { value }
            if inputs.is_empty()
                && outputs.len() == 1
                && output.is_some_and(|s| s.accepts(value))
                && matches!(outputs[0].contract, PortContract::State { .. }) =>
        {
            Ok(())
        }
        Operation::Identity {} if same => Ok(()),
        Operation::DeclareQuantity { quantity, unit } if same_delivery => {
            crate::signal::text(quantity, 128)?;
            crate::signal::text(unit, 128)?;
            if quantity.trim() != quantity || unit.trim() != unit {
                return fail();
            }
            let Some(mut declared) = input.cloned() else {
                return fail();
            };
            match &mut declared {
                ScalarSchema::Integer { unit: source, .. }
                | ScalarSchema::Number { unit: source, .. }
                    if source.is_none() =>
                {
                    *source = Some(unit.clone());
                }
                _ => return fail(),
            }
            if output == Some(&declared) {
                Ok(())
            } else {
                fail()
            }
        }
        Operation::Not {} if same && bool_inputs => Ok(()),
        Operation::HatDirection { .. }
            if one && same_delivery
                && input == Some(&ScalarSchema::Integer { min: -1, max: 7, unit: None })
                && matches!(output, Some(ScalarSchema::Boolean {})) => Ok(()),
        Operation::EventToState {}
            if one
                && input == output
                && matches!(inputs[0].contract, PortContract::Event { .. })
                && matches!(outputs[0].contract, PortContract::State { .. }) =>
        {
            Ok(())
        }
        Operation::EventEdge { edge }
            if one
                && input == output
                && matches!(inputs[0].contract, PortContract::State { .. })
                && matches!(outputs[0].contract, PortContract::Event { .. })
                && (*edge == Edge::Change || bool_inputs) =>
        {
            Ok(())
        }
        Operation::Any {} | Operation::All {}
            if (2..=8).contains(&inputs.len())
                && outputs.len() == 1
                && bool_inputs
                && inputs
                    .iter()
                    .all(|p| matches!(p.contract, PortContract::State { .. }))
                && matches!(outputs[0].contract, PortContract::State { .. })
                && matches!(output, Some(ScalarSchema::Boolean {})) =>
        {
            Ok(())
        }
        Operation::Priority { order }
            if (2..=8).contains(&inputs.len())
                && outputs.len() == 1
                && inputs.iter().all(|port| {
                    schema(&port.contract) == output
                        && matches!(port.contract, PortContract::State { .. })
                })
                && matches!(outputs[0].contract, PortContract::State { .. })
                && explicit_order(order, inputs) =>
        {
            Ok(())
        }
        Operation::Gate {}
            if inputs.len() == 2
                && outputs.len() == 1
                && inputs.iter().any(|p| {
                    p.id == "gate"
                        && matches!(
                            p.contract,
                            PortContract::State {
                                schema: ScalarSchema::Boolean {},
                                ..
                            }
                        )
                })
                && inputs.iter().any(|p| {
                    p.id == "value"
                        && compatible(&p.contract, &outputs[0].contract)
                        && compatible(&outputs[0].contract, &p.contract)
                }) =>
        {
            Ok(())
        }
        Operation::Fallback {
            unknown,
            stale,
            unavailable,
        } if same
            && output.is_some_and(|s| {
                [unknown, stale, unavailable]
                    .into_iter()
                    .all(|v| s.accepts(v))
            }) =>
        {
            Ok(())
        }
        Operation::Select {
            when_true,
            when_false,
        } if same_delivery
            && bool_inputs
            && output.is_some_and(|s| s.accepts(when_true) && s.accepts(when_false)) =>
        {
            Ok(())
        }
        Operation::Scale { factor, offset } if same_delivery && finite(&[*factor, *offset]) => {
            numeric_operation(input, output, *factor, *offset, Rounding::None, None)
        }
        Operation::RangeMap {
            input_min,
            input_max,
            output_min,
            output_max,
            rounding,
        } if same_delivery
            && finite(&[*input_min, *input_max, *output_min, *output_max])
            && input_min < input_max =>
        {
            let Some((min, max, _)) = input.and_then(numeric) else {
                return fail();
            };
            if min < *input_min || max > *input_max {
                return fail();
            }
            let factor = (output_max - output_min) / (input_max - input_min);
            numeric_operation(
                input,
                output,
                factor,
                output_min - input_min * factor,
                *rounding,
                None,
            )
        }
        Operation::UnitConvert { from, to, rounding } if same_delivery => {
            let Some((factor, offset)) = conversion(from, to) else {
                return fail();
            };
            numeric_operation(input, output, factor, offset, *rounding, Some((from, to)))
        }
        Operation::Invert {} if same && input.and_then(numeric).is_some() => Ok(()),
        Operation::Deadzone {
            center,
            radius,
            rounding,
        } if same
            && finite(&[*center, *radius])
            && *radius >= 0.0
            && input.and_then(numeric).is_some_and(|(min, max, _)| {
                *center >= min && *center <= max && *radius <= (max - center).max(center - min)
            })
            && (!matches!(output, Some(ScalarSchema::Integer { .. }))
                || *rounding != Rounding::None
                || *radius == 0.0) =>
        {
            Ok(())
        }
        Operation::Curve { points, rounding }
            if same_delivery
                && (2..=32).contains(&points.len())
                && points.iter().all(|p| finite(&[p.input, p.output]))
                && points.windows(2).all(|p| p[0].input < p[1].input) =>
        {
            let (Some((min, max, unit)), Some((out_min, out_max, out_unit))) =
                (input.and_then(numeric), output.and_then(numeric))
            else {
                return fail();
            };
            if min < points[0].input
                || max > points.last().unwrap().input
                || unit != out_unit
                || points
                    .iter()
                    .any(|p| p.output < out_min || p.output > out_max)
                || matches!(output, Some(ScalarSchema::Integer { .. }))
                    && *rounding == Rounding::None
            {
                return fail();
            }
            Ok(())
        }
        Operation::Compare { threshold, .. }
            if same_delivery
                && finite(&[*threshold])
                && input.and_then(numeric).is_some()
                && matches!(output, Some(ScalarSchema::Boolean {})) =>
        {
            Ok(())
        }
        Operation::Hysteresis {
            press,
            release,
            comparison,
        } if same_delivery
            && finite(&[*press, *release])
            && input.and_then(numeric).is_some()
            && matches!(output, Some(ScalarSchema::Boolean {}))
            && ((*comparison == Comparison::GreaterEqual && release < press)
                || (*comparison == Comparison::LessEqual && release > press)) =>
        {
            Ok(())
        }
        Operation::Format {
            decimals,
            prefix,
            suffix,
            ..
        } if same_delivery
            && *decimals <= 6
            && prefix.len() <= 64
            && suffix.len() <= 64
            && !prefix.chars().chain(suffix.chars()).any(char::is_control)
            && input.and_then(numeric).is_some()
            && matches!(output, Some(ScalarSchema::Text { .. })) =>
        {
            Ok(())
        }
        Operation::ActionPack { arguments } | Operation::SetOnChange { arguments }
            if outputs.len() == 1 =>
        {
            let PortContract::Invocation {
                parameters,
                semantics,
            } = &outputs[0].contract
            else {
                return fail();
            };
            let triggered = matches!(operation, Operation::ActionPack { .. });
            if arguments.len() > 4
                || parameters.len() != arguments.len()
                || inputs.len() != arguments.len() + usize::from(triggered)
                || (!triggered && *semantics != ActionSemantics::Setter)
            {
                return fail();
            }
            let mut names = BTreeSet::new();
            for (argument, parameter) in arguments.iter().zip(parameters) {
                let Some(port) = inputs.iter().find(|port| port.id == *argument) else {
                    return fail();
                };
                if !names.insert(argument)
                    || parameter.id != *argument
                    || schema(&port.contract) != Some(&parameter.schema)
                    || !matches!(port.contract, PortContract::State { .. })
                {
                    return fail();
                }
            }
            if triggered
                && !inputs.iter().any(|port| {
                    !arguments.contains(&port.id)
                        && matches!(port.contract, PortContract::Event { .. })
                })
            {
                return fail();
            }
            Ok(())
        }
        Operation::Builtin {
            rule,
            inputs: order,
        } if outputs.len() == 1
            && inputs
                .iter()
                .all(|port| matches!(port.contract, PortContract::State { .. }))
            && matches!(outputs[0].contract, PortContract::State { .. })
            && explicit_order(order, inputs) =>
        {
            let kinds = order
                .iter()
                .map(|id| {
                    controller_kind(&inputs.iter().find(|port| &port.id == id).unwrap().contract)
                        .ok_or(
                        "A legacy built-in requires its exact controller-compatible native schema.",
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            if Some(rule.validate(&kinds)?) != controller_kind(&outputs[0].contract) {
                return fail();
            }
            Ok(())
        }
        Operation::Script { source, parameters }
            if !source.is_empty()
                && source.len() <= 16_384
                && parameters.len() <= 64
                && (1..=32).contains(&inputs.len())
                && serde_json::to_vec(parameters).is_ok_and(|bytes| bytes.len() <= 4096) =>
        {
            for (key, value) in parameters {
                crate::signal::text(key, 80)?;
                if matches!(value,PortValue::Number(n) if !finite(&[*n])) {
                    return fail();
                }
            }
            Ok(())
        }
        _ => fail(),
    }
}
fn numeric_operation(
    input: Option<&ScalarSchema>,
    output: Option<&ScalarSchema>,
    factor: f64,
    offset: f64,
    rounding: Rounding,
    units: Option<(&String, &String)>,
) -> Result<(), String> {
    let (Some((min, max, unit)), Some((out_min, out_max, out_unit))) =
        (input.and_then(numeric), output.and_then(numeric))
    else {
        return Err("Numeric conversion needs numeric ports.".into());
    };
    let valid_units = units.map_or(unit == out_unit, |(from, to)| {
        unit == Some(from.as_str()) && out_unit == Some(to.as_str())
    });
    let (a, b) = (
        rounded(min * factor + offset, rounding),
        rounded(max * factor + offset, rounding),
    );
    if !valid_units
        || !finite(&[factor, offset, a, b])
        || a.min(b) < out_min - 1e-9
        || a.max(b) > out_max + 1e-9
        || matches!(output, Some(ScalarSchema::Integer { .. }))
            && (rounding == Rounding::None
                && (!matches!(input, Some(ScalarSchema::Integer { .. }))
                    || factor.fract() != 0.0
                    || offset.fract() != 0.0))
    {
        return Err(
            "Conversion output range, rounding or units do not fit its declared port.".into(),
        );
    }
    Ok(())
}
fn explicit_order(order: &[String], inputs: &[PortDescriptor]) -> bool {
    order.len() == inputs.len()
        && order.iter().collect::<BTreeSet<_>>() == inputs.iter().map(|port| &port.id).collect()
}
pub(crate) fn rounded(value: f64, rounding: Rounding) -> f64 {
    match rounding {
        Rounding::None => value,
        Rounding::Nearest => value.round(),
        Rounding::Floor => value.floor(),
        Rounding::Ceil => value.ceil(),
        Rounding::Truncate => value.trunc(),
    }
}
impl Program {
    pub fn compile(&self) -> Result<CompiledProgram, String> {
        let desktop_fragment=self.sinks.iter().filter(|sink|matches!(sink.kind,SinkKind::Desktop{..})).collect::<Vec<_>>();
        if serde_json::to_vec(&desktop_fragment).map_err(|error|error.to_string())?.len()>crate::profile::DOCUMENT_LIMIT {
            return Err("The desktop backend fragment exceeds its 64 KiB budget.".into());
        }
        let port_count = self.sources.len()
            + self
                .nodes
                .iter()
                .map(|n| n.inputs.len() + n.outputs.len())
                .sum::<usize>()
            + self
                .controllers
                .iter()
                .map(|c| c.layout.buttons.len() + c.layout.axes.len() + c.layout.hats.len())
                .sum::<usize>()
            + self
                .sinks
                .iter()
                .filter(|s| matches!(s.kind, SinkKind::Resource { .. } | SinkKind::Desktop { .. }))
                .count();
        if serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > MAX_PROGRAM_BYTES
            || self.nodes.len() + self.sinks.iter().filter(|sink|matches!(sink.kind,SinkKind::Desktop { .. })).count() > MAX_NODES
            || self.controllers.len() > MAX_TARGETS
            || port_count > MAX_PORTS
            || self.nodes.iter().map(|n| n.inputs.len()).sum::<usize>() + self.sinks.len()
                > MAX_BINDINGS
        {
            return Err("Compiled graph exceeds its bounded program budget.".into());
        }
        let mut contracts = BTreeMap::new();
        let mut owners = BTreeSet::new();
        for source in &self.sources {
            port_id(&source.endpoint.node_id)?;
            port_id(&source.endpoint.port_id)?;
            source.contract.validate_graph(Direction::Output)?;
            if !matches!(
                source.contract,
                PortContract::State { .. } | PortContract::Event { .. }
            ) || contracts
                .insert(source.endpoint.clone(), source.contract.clone())
                .is_some()
            {
                return Err("Duplicate or non-scalar source port.".into());
            }
            owners.insert(&source.endpoint.node_id);
        }
        if owners.len() > MAX_SOURCES {
            return Err("An execution program may use at most 16 source resources.".into());
        }
        let mut nodes = BTreeMap::new();
        for (index, node) in self.nodes.iter().enumerate() {
            port_id(&node.id)?;
            if owners.contains(&node.id) || nodes.insert(node.id.clone(), index).is_some() {
                return Err("Duplicate program node identity.".into());
            }
            let inputs = node
                .inputs
                .iter()
                .map(|input| PortDescriptor {
                    id: input.port_id.clone(),
                    name: input.port_id.clone(),
                    direction: Direction::Input,
                    contract: input.contract.clone(),
                })
                .collect::<Vec<_>>();
            validate_operation(&node.operation, &inputs, &node.outputs)?;
            let mut inputs_seen = BTreeSet::new();
            for input in &node.inputs {
                port_id(&input.port_id)?;
                port_id(&input.edge_id)?;
                if !inputs_seen.insert(&input.port_id) {
                    return Err("Duplicate named node input.".into());
                }
            }
            for output in &node.outputs {
                port_id(&output.id)?;
                if contracts
                    .insert(
                        Endpoint {
                            node_id: node.id.clone(),
                            port_id: output.id.clone(),
                        },
                        output.contract.clone(),
                    )
                    .is_some()
                {
                    return Err("Duplicate node output port.".into());
                }
            }
        }
        let mut order = Vec::new();
        let mut marks = vec![0u8; self.nodes.len()];
        fn visit(
            index: usize,
            program: &Program,
            nodes: &BTreeMap<String, usize>,
            marks: &mut [u8],
            order: &mut Vec<usize>,
        ) -> Result<(), String> {
            if marks[index] == 1 {
                return Err("Internal processing cycle.".into());
            }
            if marks[index] == 2 {
                return Ok(());
            }
            marks[index] = 1;
            let next = program.nodes[index]
                .inputs
                .iter()
                .filter_map(|input| {
                    nodes
                        .get(&input.from.node_id)
                        .map(|index| (input.from.node_id.clone(), *index))
                })
                .collect::<BTreeMap<_, _>>();
            for next in next.values() {
                visit(*next, program, nodes, marks, order)?
            }
            marks[index] = 2;
            order.push(index);
            Ok(())
        }
        for index in nodes.values() {
            visit(*index, self, &nodes, &mut marks, &mut order)?
        }
        let mut dependencies = BTreeMap::<String, BTreeSet<Endpoint>>::new();
        let mut edge_ids = BTreeSet::new();
        let mut timer_reserve = 0;
        for index in &order {
            let node = &self.nodes[*index];
            let mut deps = BTreeSet::new();
            for input in &node.inputs {
                let from = contracts
                    .get(&input.from)
                    .ok_or("Missing exact input endpoint.")?;
                if !compatible(from, &input.contract) || !edge_ids.insert(&input.edge_id) {
                    return Err("Input contract differs or binding ID is duplicated.".into());
                }
                if let Some(inputs) = dependencies.get(&input.from.node_id) {
                    deps.extend(inputs.clone())
                } else {
                    deps.insert(input.from.clone());
                }
            }
            if let Operation::Builtin { rule, .. } = &node.operation {
                timer_reserve += match rule {
                    crate::rules::Rule::Gesture { .. } => 3,
                    crate::rules::Rule::Pulse { .. } => 2,
                    crate::rules::Rule::Macro { steps } => steps.len(),
                    _ => 0,
                };
            }
            deps.extend(node.inputs.iter().map(|input| input.from.clone()));
            dependencies.insert(node.id.clone(), deps);
        }
        if timer_reserve > crate::engine::MAX_TIMERS {
            return Err("Built-in timers exceed the shared timer budget.".into());
        }
        let mut controllers = BTreeMap::new();
        for controller in &self.controllers {
            port_id(&controller.id)?;
            controller.layout.validate()?;
            if owners.contains(&controller.id)
                || nodes.contains_key(&controller.id)
                || controllers
                    .insert(controller.id.clone(), &controller.layout)
                    .is_some()
            {
                return Err("Duplicate controller identity.".into());
            }
        }
        let mut targets = BTreeSet::new();
        let mut desktop_endpoints = BTreeSet::new();
        for sink in &self.sinks {
            port_id(&sink.id)?;
            port_id(&sink.endpoint.node_id)?;
            port_id(&sink.endpoint.port_id)?;
            sink.contract.validate_graph(Direction::Input)?;
            let from = contracts
                .get(&sink.from)
                .ok_or("Missing sink source port.")?;
            if !compatible(from, &sink.contract)
                || !edge_ids.insert(&sink.id)
                || !targets.insert(sink.endpoint.clone())
            {
                return Err("Sink contract differs or has ambiguous writers; use an explicit arbitration node.".into());
            }
            if let SinkKind::Desktop { action } = &sink.kind {
                action.validate()?;
                if sink.contract != action.contract() {
                    return Err("Desktop sink differs from its exact typed action port.".into());
                }
                desktop_endpoints.insert(&sink.endpoint.node_id);
                if desktop_endpoints.len()>1 {
                    return Err("An execution program supports one logical desktop endpoint.".into());
                }
            }
            if let SinkKind::Virtual {
                controller_id,
                control,
            } = &sink.kind
            {
                let layout = controllers
                    .get(controller_id)
                    .ok_or("Missing virtual controller layout.")?;
                let expected = virtual_port_contract(layout, &sink.endpoint.port_id)
                    .ok_or("Missing virtual controller port.")?;
                if sink.endpoint.node_id != *controller_id
                    || schema(&expected) != schema(&sink.contract)
                {
                    return Err("Virtual sink contract differs from its explicit layout.".into());
                }
                let actual = match control {
                    OutputControl::Button { index } => {
                        layout.buttons.get(*index as usize).map(String::as_str)
                    }
                    OutputControl::Axis { index } => {
                        layout.axes.get(*index as usize).map(|a| a.port_id.as_str())
                    }
                    OutputControl::Hat { index } => {
                        layout.hats.get(*index as usize).map(String::as_str)
                    }
                };
                if actual != Some(sink.endpoint.port_id.as_str()) {
                    return Err("Virtual control index differs from its named layout port.".into());
                }
            }
        }
        let mut normalized = self.clone();
        normalized
            .sources
            .sort_by(|a, b| a.endpoint.cmp(&b.endpoint));
        normalized.nodes.sort_by(|a, b| a.id.cmp(&b.id));
        for node in &mut normalized.nodes {
            node.inputs.sort_by(|a, b| a.port_id.cmp(&b.port_id));
            node.outputs.sort_by(|a, b| a.id.cmp(&b.id));
        }
        normalized.sinks.sort_by(|a, b| a.id.cmp(&b.id));
        normalized.controllers.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(CompiledProgram {
            program: self.clone(),
            order,
            contracts,
            dependencies,
            hash: crate::signal::digest(&normalized)?,
            timer_reserve,
        })
    }
}
