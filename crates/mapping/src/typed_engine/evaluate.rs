//! Dependency-scoped evaluation and explicit baseline seeding.
use super::*;

impl Phase {
    pub(super) fn evaluate(
        &mut self,
        dirty: BTreeSet<Endpoint>,
        events: BTreeSet<Endpoint>,
        baseline: bool,
    ) -> Result<BTreeSet<Endpoint>, String> {
        self.evaluate_scoped(dirty, events, baseline, BTreeSet::new())
    }
    pub(super) fn evaluate_baseline(
        &mut self,
        dirty: BTreeSet<Endpoint>,
        events: BTreeSet<Endpoint>,
    ) -> Result<BTreeSet<Endpoint>, String> {
        let rebased = dirty.clone();
        self.evaluate_scoped(dirty, events, true, rebased)
    }
    fn evaluate_scoped(
        &mut self,
        mut dirty: BTreeSet<Endpoint>,
        mut events: BTreeSet<Endpoint>,
        baseline: bool,
        mut rebased: BTreeSet<Endpoint>,
    ) -> Result<BTreeSet<Endpoint>, String> {
        for index in self.compiled.order.clone() {
            let node = self.compiled.program.nodes[index].clone();
            let due = self.states[&node.id]
                .rule
                .next()
                .is_some_and(|at| at <= self.clock);
            if !due
                && !node.inputs.iter().any(|input| dirty.contains(&input.from))
                && !matches!(node.operation, Operation::Constant { .. })
            {
                continue;
            }
            let seed = node
                .inputs
                .iter()
                .any(|input| rebased.contains(&input.from));
            let inputs = self.inputs(&node);
            let event_inputs = node
                .inputs
                .iter()
                .filter(|input| events.contains(&input.from))
                .map(|input| input.port_id.clone())
                .collect::<BTreeSet<_>>();
            let token = self.token(&node.id)?;
            let input_proof = self.native_inputs(&node);
            self.causal = if self.states[&node.id].rule.next().is_some() && !seed {
                self.states[&node.id].rule_proof.clone()
            } else {
                input_proof.clone()
            };
            let state = self.states.get_mut(&node.id).unwrap();
            if state
                .validity
                .as_ref()
                .is_some_and(|previous| previous != &token)
            {
                state.rule = crate::rules::State::default();
                state.previous = None;
                state.boolean = false;
                state.pending.clear();
                state.inflight = None;
                state.script_state.clear();
                state.script_started = false;
            }
            state.validity = Some(token);
            if matches!(node.operation, Operation::Script { .. }) {
                if let Some(status) = missing(&inputs) {
                    for output in &node.outputs {
                        let endpoint = Endpoint {
                            node_id: node.id.clone(),
                            port_id: output.id.clone(),
                        };
                        self.put(&endpoint, status.clone());
                        dirty.insert(endpoint);
                    }
                }
                self.enqueue_script(&node, inputs, !baseline && !event_inputs.is_empty())?;
                continue;
            }
            self.states.get_mut(&node.id).unwrap().rule_proof = self.causal.clone();
            let outputs = match self.evaluate_node(&node, &inputs, &event_inputs, baseline, seed) {
                Ok(outputs) => outputs,
                Err(error) => {
                    self.fault = Some(error.clone());
                    self.stop();
                    return Err(error);
                }
            };
            for (id, sample, fire) in outputs {
                let endpoint = Endpoint {
                    node_id: node.id.clone(),
                    port_id: id,
                };
                // Constants have no new observation when another branch runs.
                // Rewriting an unchanged value would manufacture dirty input
                // and callbacks, including when a script's own output expires.
                if matches!(node.operation, Operation::Constant { .. })
                    && self.values[&endpoint].sample == sample
                {
                    continue;
                }
                self.put(&endpoint, sample);
                dirty.insert(endpoint.clone());
                if seed {
                    rebased.insert(endpoint.clone());
                }
                if fire {
                    events.insert(endpoint);
                }
            }
        }
        self.causal = native::NativeProof::default();
        Ok(events)
    }
    fn evaluate_node(
        &mut self,
        node: &Node,
        inputs: &BTreeMap<String, SignalSample>,
        events: &BTreeSet<String>,
        baseline: bool,
        seed: bool,
    ) -> Result<Vec<(String, SignalSample, bool)>, String> {
        let output = &node.outputs[0];
        let first = inputs
            .values()
            .next()
            .cloned()
            .unwrap_or(SignalSample::Unknown);
        if let Some(boundary) = self.desktop_rule_boundary.filter(|_| {
            matches!(
                node.operation,
                Operation::Builtin { .. }
                    | Operation::Hysteresis { .. }
                    | Operation::EventEdge { .. }
                    | Operation::SetOnChange { .. }
            )
        }) {
            let observations = self.source_observations(&self.token(&node.id)?);
            if observations.is_empty()
                || observations
                    .iter()
                    .any(|observation| observation.at_us <= boundary)
            {
                let state = self.states.get_mut(&node.id).unwrap();
                state.rule = crate::rules::State::default();
                state.boolean = false;
                state.previous = None;
                return Ok(vec![(output.id.clone(), SignalSample::Unknown, false)]);
            }
        }
        let missing = missing(inputs);
        let mut fire = false;
        let sample = match &node.operation {
            Operation::Constant { value } => SignalSample::scalar(value.clone()),
            Operation::Fallback {
                unknown,
                stale,
                unavailable,
            } => match first {
                SignalSample::Unknown => SignalSample::scalar(unknown.clone()),
                SignalSample::Stale => SignalSample::scalar(stale.clone()),
                SignalSample::Unavailable => SignalSample::scalar(unavailable.clone()),
                known => known,
            },
            Operation::Priority { order } => order
                .iter()
                .find_map(|id| {
                    matches!(inputs[id], SignalSample::Known(_)).then(|| inputs[id].clone())
                })
                .unwrap_or_else(|| missing.clone().unwrap_or(SignalSample::Unknown)),
            Operation::Any {} => {
                if inputs.values().any(|v| boolean(v) == Some(true)) {
                    SignalSample::scalar(PortValue::Boolean(true))
                } else {
                    missing
                        .clone()
                        .unwrap_or_else(|| SignalSample::scalar(PortValue::Boolean(false)))
                }
            }
            Operation::All {} => {
                if inputs.values().any(|v| boolean(v) == Some(false)) {
                    SignalSample::scalar(PortValue::Boolean(false))
                } else {
                    missing
                        .clone()
                        .unwrap_or_else(|| SignalSample::scalar(PortValue::Boolean(true)))
                }
            }
            Operation::ActionPack { arguments } | Operation::SetOnChange { arguments } => {
                if let Some(missing) = missing {
                    self.states.get_mut(&node.id).unwrap().previous = None;
                    missing
                } else {
                    let value = SignalValue::ActionPack(ActionPack {
                        arguments: arguments
                            .iter()
                            .map(|id| NamedArgument {
                                id: id.clone(),
                                value: scalar(&inputs[id]).unwrap().clone(),
                            })
                            .collect(),
                    });
                    let state = self.states.get_mut(&node.id).unwrap();
                    fire = match node.operation {
                        Operation::ActionPack { .. } => {
                            !baseline && events.iter().any(|event| !arguments.contains(event))
                        }
                        _ => state.previous.as_ref() != Some(&value),
                    };
                    state.previous = Some(value.clone());
                    SignalSample::Known(value)
                }
            }
            _ if missing.is_some() => {
                let state = self.states.get_mut(&node.id).unwrap();
                state.previous = None;
                state.boolean = false;
                state.rule = crate::rules::State::default();
                missing.unwrap()
            }
            Operation::Identity {}
            | Operation::DeclareQuantity { .. }
            | Operation::EventToState {} => {
                fire = !baseline
                    && !events.is_empty()
                    && matches!(
                        output.contract,
                        PortContract::Event { .. } | PortContract::Invocation { .. }
                    );
                first
            }
            Operation::Not {} => {
                SignalSample::scalar(PortValue::Boolean(!boolean(&first).unwrap()))
            }
            Operation::HatDirection {
                direction,
                diagonals,
            } => {
                if let Some(value) = number(&first) {
                    SignalSample::scalar(PortValue::Boolean(crate::desktop::hat_direction(
                        (value >= 0.0).then_some(value as u8),
                        *direction,
                        if *diagonals {
                            crate::desktop::HatDiagonal::IncludeAdjacent
                        } else {
                            crate::desktop::HatDiagonal::CardinalOnly
                        },
                    )))
                } else {
                    missing.clone().unwrap_or(SignalSample::Unknown)
                }
            }
            Operation::Gate {} => {
                if boolean(&inputs["gate"]) == Some(true) {
                    fire = !baseline && events.contains("value");
                    inputs["value"].clone()
                } else {
                    SignalSample::Unknown
                }
            }
            Operation::Select {
                when_true,
                when_false,
            } => SignalSample::scalar(if boolean(&first).unwrap() {
                when_true.clone()
            } else {
                when_false.clone()
            }),
            Operation::EventEdge { edge } => {
                let value = if let SignalSample::Known(value) = &first {
                    value.clone()
                } else {
                    unreachable!()
                };
                let state = self.states.get_mut(&node.id).unwrap();
                let previous = state.previous.replace(value.clone());
                fire = !baseline
                    && previous.as_ref().is_some_and(|previous| match edge {
                        Edge::Change => *previous != value,
                        Edge::Rising => {
                            matches!(previous, SignalValue::Scalar(PortValue::Boolean(false)))
                                && boolean(&first) == Some(true)
                        }
                        Edge::Falling => {
                            matches!(previous, SignalValue::Scalar(PortValue::Boolean(true)))
                                && boolean(&first) == Some(false)
                        }
                    });
                SignalSample::Known(value)
            }
            Operation::Compare {
                comparison,
                threshold,
            } => SignalSample::scalar(PortValue::Boolean(compare(
                number(&first).unwrap(),
                *threshold,
                *comparison,
            ))),
            Operation::Hysteresis {
                press,
                release,
                comparison,
            } => {
                let state = self.states.get_mut(&node.id).unwrap();
                let threshold = if state.boolean { *release } else { *press };
                let comparison = if state.boolean {
                    if *comparison == Comparison::GreaterEqual {
                        Comparison::Greater
                    } else {
                        Comparison::Less
                    }
                } else {
                    *comparison
                };
                state.boolean = compare(number(&first).unwrap(), threshold, comparison);
                SignalSample::scalar(PortValue::Boolean(state.boolean))
            }
            Operation::Builtin {
                rule,
                inputs: order,
            } => {
                let values = order
                    .iter()
                    .map(|id| {
                        legacy_value(
                            &inputs[id],
                            controller_kind(
                                &node
                                    .inputs
                                    .iter()
                                    .find(|input| &input.port_id == id)
                                    .unwrap()
                                    .contract,
                            )
                            .unwrap(),
                        )
                    })
                    .collect::<Vec<_>>();
                let kind = controller_kind(&output.contract).unwrap();
                let state = self.states.get_mut(&node.id).unwrap();
                let value = if seed {
                    state.rule.seed(rule, &values, kind, self.clock)
                } else {
                    state.rule.apply(rule, &values, kind, self.clock)
                };
                value
                    .map(from_legacy)
                    .map(SignalSample::scalar)
                    .unwrap_or(SignalSample::Unknown)
            }
            Operation::Format {
                decimals,
                prefix,
                suffix,
                rounding,
                overflow,
            } => {
                let factor = 10f64.powi(*decimals as i32);
                let value = rounded(number(&first).unwrap() * factor, *rounding) / factor;
                let mut text = format!(
                    "{prefix}{value:.precision$}{suffix}",
                    precision = *decimals as usize
                );
                let ScalarSchema::Text {
                    max_bytes,
                    encoding,
                } = schema(&output.contract).unwrap()
                else {
                    unreachable!()
                };
                if *encoding == crate::signal::TextEncoding::Ascii && !text.is_ascii() {
                    return Err("Formatted value is not ASCII.".into());
                }
                if text.len() > *max_bytes as usize {
                    if *overflow == TextOverflow::Reject {
                        return Err("Formatted value exceeds its explicit text budget.".into());
                    }
                    let mut end = (*max_bytes as usize).min(text.len());
                    while !text.is_char_boundary(end) {
                        end -= 1
                    }
                    text.truncate(end)
                }
                SignalSample::scalar(PortValue::Text(text))
            }
            operation => {
                let value =
                    number(&first).ok_or("Numeric operation received a nonnumeric scalar.")?;
                let input = &node.inputs[0].contract;
                let (min, max, _) = schema(input).and_then(numeric).unwrap();
                let result = match operation {
                    Operation::Scale { factor, offset } => value * factor + offset,
                    Operation::Clamp { min, max } => value.clamp(*min, *max),
                    Operation::RangeMap {
                        input_min,
                        input_max,
                        output_min,
                        output_max,
                        rounding,
                    } => rounded(
                        (value - input_min) * (output_max - output_min) / (input_max - input_min)
                            + output_min,
                        *rounding,
                    ),
                    Operation::UnitConvert {
                        from,
                        to,
                        rounding,
                        clamp,
                    } => {
                        let (factor, offset) = conversion(from, to).unwrap();
                        let converted = value * factor + offset;
                        let converted = if *clamp {
                            let (min, max, _) = schema(&output.contract).and_then(numeric).unwrap();
                            converted.clamp(min, max)
                        } else {
                            converted
                        };
                        rounded(converted, *rounding)
                    }
                    Operation::Invert {} => min + max - value,
                    Operation::Deadzone {
                        center_percent,
                        deadzone_percent,
                        rounding,
                    } => {
                        let span = max - min;
                        let center = min + span * (center_percent / 100.0);
                        let radius = span * (deadzone_percent / 200.0);
                        rounded(
                            if (value - center).abs() <= radius {
                                center
                            } else if value > center {
                                center
                                    + (value - center - radius) * (max - center)
                                        / (max - center - radius)
                            } else {
                                center
                                    - (center - value - radius) * (center - min)
                                        / (center - min - radius)
                            },
                            *rounding,
                        )
                    }
                    Operation::Curve { points, rounding } => {
                        let pair = points
                            .windows(2)
                            .find(|pair| value <= pair[1].input)
                            .ok_or("Curve input exceeds its declared domain.")?;
                        rounded(
                            pair[0].output
                                + (value - pair[0].input) * (pair[1].output - pair[0].output)
                                    / (pair[1].input - pair[0].input),
                            *rounding,
                        )
                    }
                    _ => return Err("Unsupported typed operation.".into()),
                };
                SignalSample::scalar(PortValue::Number(result))
            }
        };
        if let SignalSample::Known(value) = &sample {
            if !accepts(&output.contract, value) {
                return Err(format!(
                    "Operation {} exceeded its declared output contract.",
                    node.id
                ));
            }
        }
        if matches!(output.contract, PortContract::Event { .. })
            && !matches!(node.operation, Operation::EventEdge { .. })
        {
            fire = !baseline && (!events.is_empty() || fire);
        }
        Ok(vec![(output.id.clone(), sample, fire)])
    }
}
