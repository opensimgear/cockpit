//! Independent admission of the bounded behavior language. Runtime code never crosses this boundary.
use super::*;

#[derive(Clone)]
pub(super) struct Inference {
    kind: &'static str,
    min: f64,
    max: f64,
    values: Vec<String>,
}
fn behavior_id(value: &Value) -> Result<&str, String> {
    let value = stable(value)?;
    ensure(
        value.as_bytes()[0].is_ascii_alphabetic(),
        "Invalid behavior ID.",
    )?;
    Ok(value)
}
fn unique(items: &[Value]) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    for item in items {
        ensure(
            ids.insert(behavior_id(&item["id"])?),
            "Duplicate behavior ID.",
        )?;
    }
    Ok(())
}
fn scalar(value: &Value) -> Result<(), String> {
    ensure(
        value.is_boolean()
            || value
                .as_f64()
                .is_some_and(|n| n.is_finite() && n.abs() <= 1e9)
            || value.as_str().is_some_and(|s| {
                s.encode_utf16().count() <= 120 && !s.chars().any(|c| c <= '\u{1f}')
            }),
        "Invalid behavior scalar.",
    )
}
pub(super) fn accepts(domain: &Value, value: &Value) -> bool {
    match domain["type"].as_str() {
        Some("boolean") => value.is_boolean(),
        Some("number") => value.as_f64().is_some_and(|n| {
            n.is_finite()
                && domain["min"].as_f64().is_some_and(|min| n >= min)
                && domain["max"].as_f64().is_some_and(|max| n <= max)
        }),
        Some("enum") => domain["values"]
            .as_array()
            .is_some_and(|values| values.contains(value)),
        _ => false,
    }
}
fn domain(value: &Value, initial: &Value) -> Result<(), String> {
    match value["type"].as_str() {
        Some("boolean") => {}
        Some("number") => {
            let min = number(&value["min"], -1e9, 1e9)?;
            let max = number(&value["max"], -1e9, 1e9)?;
            ensure(min < max, "Invalid behavior numeric domain.")?;
        }
        Some("enum") => {
            let mut seen = BTreeSet::new();
            for v in list(&value["values"], 1, 32)? {
                ensure(
                    seen.insert(behavior_id(v)?),
                    "Repeated behavior enum value.",
                )?;
            }
        }
        _ => return Err("Invalid behavior signal type.".into()),
    };
    ensure(
        accepts(value, initial),
        "Behavior initial/neutral outside domain.",
    )
}
fn domain_type(value: &Value) -> Inference {
    match value["type"].as_str() {
        Some("boolean") => Inference {
            kind: "boolean",
            min: 0.,
            max: 0.,
            values: vec![],
        },
        Some("number") => Inference {
            kind: "number",
            min: value["min"].as_f64().unwrap(),
            max: value["max"].as_f64().unwrap(),
            values: vec![],
        },
        _ => Inference {
            kind: "string",
            min: 0.,
            max: 0.,
            values: value["values"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect(),
        },
    }
}
fn boolean() -> Inference {
    Inference {
        kind: "boolean",
        min: 0.,
        max: 0.,
        values: vec![],
    }
}
fn infer(
    expression: &Value,
    d: &Value,
    nodes: &mut usize,
    depth: usize,
) -> Result<Inference, String> {
    *nodes += 1;
    ensure(
        *nodes <= 32 && depth < 8,
        "Behavior expression exceeds work bounds.",
    )?;
    let e = expression
        .as_object()
        .ok_or("Invalid behavior expression.")?;
    if e.contains_key("literal") {
        object(expression, &["literal"], &[])?;
        let v = &expression["literal"];
        scalar(v)?;
        return Ok(if let Some(n) = v.as_f64() {
            Inference {
                kind: "number",
                min: n,
                max: n,
                values: vec![],
            }
        } else if let Some(s) = v.as_str() {
            Inference {
                kind: "string",
                min: 0.,
                max: 0.,
                values: vec![s.into()],
            }
        } else {
            boolean()
        });
    }
    if e.contains_key("ref") {
        object(expression, &["ref"], &[])?;
        let signal = d["signals"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == expression["ref"])
            .ok_or("Unknown behavior signal reference.")?;
        return Ok(domain_type(signal));
    }
    if e.contains_key("state") {
        object(expression, &["state"], &[])?;
        let region = d["regions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == expression["state"])
            .ok_or("Unknown behavior region reference.")?;
        return Ok(Inference {
            kind: "string",
            min: 0.,
            max: 0.,
            values: region["states"]
                .as_array()
                .ok_or("Invalid behavior states.")?
                .iter()
                .map(|v| v.as_str().unwrap_or("").into())
                .collect(),
        });
    }
    let mut child = |v: &Value| infer(v, d, nodes, depth + 1);
    let op = expression["op"]
        .as_str()
        .ok_or("Missing behavior operator.")?;
    match op {
        "not" => {
            object(expression, &["op", "value"], &[])?;
            ensure(
                child(&expression["value"])?.kind == "boolean",
                "Boolean expression required.",
            )?;
            Ok(boolean())
        }
        "and" | "or" => {
            object(expression, &["op", "values"], &[])?;
            for v in list(&expression["values"], 1, 16)? {
                ensure(child(v)?.kind == "boolean", "Boolean expression required.")?;
            }
            Ok(boolean())
        }
        "if" => {
            object(expression, &["op", "condition", "then", "else"], &[])?;
            ensure(
                child(&expression["condition"])?.kind == "boolean",
                "Boolean expression required.",
            )?;
            let a = child(&expression["then"])?;
            let b = child(&expression["else"])?;
            ensure(
                a.kind == b.kind,
                "Conditional branches have different types.",
            )?;
            Ok(Inference {
                kind: a.kind,
                min: a.min.min(b.min),
                max: a.max.max(b.max),
                values: a.values.into_iter().chain(b.values).collect(),
            })
        }
        "clamp" => {
            object(expression, &["op", "value", "min", "max"], &[])?;
            let v = child(&expression["value"])?;
            let low = child(&expression["min"])?;
            let high = child(&expression["max"])?;
            ensure(
                v.kind == "number"
                    && low.kind == "number"
                    && high.kind == "number"
                    && low.max <= high.min,
                "Invalid numeric clamp bounds.",
            )?;
            Ok(Inference {
                kind: "number",
                min: low.min,
                max: high.max,
                values: vec![],
            })
        }
        _ => {
            object(expression, &["op", "left", "right"], &[])?;
            ensure(
                [
                    "eq", "ne", "lt", "lte", "gt", "gte", "add", "sub", "mul", "div",
                ]
                .contains(&op),
                "Unsupported behavior operator.",
            )?;
            let a = child(&expression["left"])?;
            let b = child(&expression["right"])?;
            if op == "eq" || op == "ne" {
                ensure(a.kind == b.kind, "Comparison types differ.")?;
                return Ok(boolean());
            }
            ensure(
                a.kind == "number" && b.kind == "number",
                "Numeric expression required.",
            )?;
            if ["lt", "lte", "gt", "gte"].contains(&op) {
                return Ok(boolean());
            }
            let (min, max) = if op == "add" {
                (a.min + b.min, a.max + b.max)
            } else if op == "sub" {
                (a.min - b.max, a.max - b.min)
            } else {
                ensure(
                    op != "div" || b.min > 0. || b.max < 0.,
                    "Division domain includes zero.",
                )?;
                let values = [a.min, a.max]
                    .into_iter()
                    .flat_map(|x| [b.min, b.max].map(|y| if op == "mul" { x * y } else { x / y }))
                    .collect::<Vec<_>>();
                (
                    values.iter().copied().fold(f64::INFINITY, f64::min),
                    values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                )
            };
            ensure(
                min.is_finite() && max.is_finite() && min >= -1e9 && max <= 1e9,
                "Behavior number outside bounds.",
            )?;
            Ok(Inference {
                kind: "number",
                min,
                max,
                values: vec![],
            })
        }
    }
}
fn assignable(kind: &Inference, target: &Value) -> bool {
    match target["type"].as_str() {
        Some("boolean") => kind.kind == "boolean",
        Some("number") => {
            kind.kind == "number"
                && kind.min >= target["min"].as_f64().unwrap()
                && kind.max <= target["max"].as_f64().unwrap()
        }
        Some("enum") => {
            kind.kind == "string"
                && kind
                    .values
                    .iter()
                    .all(|v| target["values"].as_array().unwrap().contains(&json!(v)))
        }
        _ => false,
    }
}
pub(super) fn validate(d: &Value) -> Result<(), String> {
    ensure(
        canonical(d)?.len() <= 131072,
        "Behavior exceeds byte budget.",
    )?;
    object(
        d,
        &["version", "signals", "events", "regions"],
        &["outputs", "timers"],
    )?;
    ensure(d["version"] == 1, "Unsupported behavior version.")?;
    let signals = list(&d["signals"], 0, 32)?;
    let events = list(&d["events"], 0, 32)?;
    let regions = list(&d["regions"], 0, 8)?;
    let empty = vec![];
    let outputs = d
        .get("outputs")
        .map(|v| list(v, 0, 16))
        .transpose()?
        .unwrap_or(&empty);
    let timers = d
        .get("timers")
        .map(|v| list(v, 0, 16))
        .transpose()?
        .unwrap_or(&empty);
    for items in [signals, events, regions, outputs, timers] {
        unique(items)?;
    }
    for signal in signals {
        let mut keys = vec!["id", "source", "type", "initial"];
        match signal["type"].as_str() {
            Some("number") => keys.extend(["min", "max"]),
            Some("enum") => keys.push("values"),
            _ => {}
        }
        object(
            signal,
            &keys,
            &["freshnessMs", "qualitySignal", "requireGameConfirmed"],
        )?;
        domain(signal, &signal["initial"])?;
        ensure(
            matches!(
                signal["source"].as_str(),
                Some("local" | "input" | "feedback")
            ),
            "Invalid behavior signal source.",
        )?;
        if signal["source"] != "feedback" {
            ensure(
                ["freshnessMs", "qualitySignal", "requireGameConfirmed"]
                    .iter()
                    .all(|key| signal.get(key).is_none()),
                "Feedback options require a feedback signal.",
            )?;
        } else {
            if let Some(v) = signal.get("freshnessMs") {
                integer(v, 1, 86400000)?;
            }
            if let Some(v) = signal.get("requireGameConfirmed") {
                ensure(v.is_boolean(), "Invalid confirmation requirement.")?;
            }
            if let Some(quality) = signal.get("qualitySignal") {
                let q = signals
                    .iter()
                    .find(|s| s["id"] == *quality)
                    .ok_or("Invalid feedback quality signal.")?;
                ensure(
                    q["source"] == "input"
                        && q["type"] == "enum"
                        && q["initial"] == "unknown"
                        && ["fresh", "unknown", "stale", "disconnected"]
                            .iter()
                            .all(|v| {
                                q["values"]
                                    .as_array()
                                    .is_some_and(|values| values.contains(&json!(v)))
                            })
                        && signals
                            .iter()
                            .filter(|s| s.get("qualitySignal") == Some(quality))
                            .count()
                            == 1,
                    "Invalid or shared feedback quality signal.",
                )?;
            }
        }
    }
    for output in outputs {
        let mut keys = vec!["id", "type", "neutral"];
        match output["type"].as_str() {
            Some("number") => keys.extend(["min", "max"]),
            Some("enum") => keys.push("values"),
            _ => {}
        }
        object(output, &keys, &[])?;
        domain(output, &output["neutral"])?;
    }
    for event in events {
        object(event, &["id"], &["updates"])?;
        let mut seen = BTreeSet::new();
        if let Some(updates) = event.get("updates") {
            for update in list(updates, 0, 32)? {
                let name = behavior_id(update)?;
                ensure(
                    seen.insert(name)
                        && signals.iter().any(|s| {
                            s["id"] == name
                                && s["source"] != "local"
                                && !signals.iter().any(|f| f["qualitySignal"] == name)
                        }),
                    "Event updates require an external signal.",
                )?;
            }
        }
    }
    for timer in timers {
        object(timer, &["id", "event", "delayMs"], &["repeat"])?;
        ensure(
            events.iter().any(|e| e["id"] == timer["event"]),
            "Unknown timer event.",
        )?;
        integer(&timer["delayMs"], 1, 86400000)?;
        if let Some(v) = timer.get("repeat") {
            ensure(v.is_boolean(), "Invalid timer repetition.")?;
        }
    }
    let mut count = 0;
    for region in regions {
        object(region, &["id", "initial", "states", "transitions"], &[])?;
        let states = list(&region["states"], 1, 32)?;
        let mut names = BTreeSet::new();
        for state in states {
            ensure(names.insert(behavior_id(state)?), "Duplicate region state.")?;
        }
        ensure(
            states.contains(&region["initial"]),
            "Invalid initial region state.",
        )?;
        let transitions = list(&region["transitions"], 0, 128)?;
        count += transitions.len();
        unique(transitions)?;
        let mut priorities: Vec<(&Value, i64)> = vec![];
        for transition in transitions {
            object(
                transition,
                &["id", "from", "event"],
                &["priority", "guard", "to", "actions"],
            )?;
            ensure(
                transition["from"] == "*" || states.contains(&transition["from"]),
                "Unknown transition source.",
            )?;
            ensure(
                events.iter().any(|e| e["id"] == transition["event"]),
                "Unknown transition event.",
            )?;
            if let Some(to) = transition.get("to") {
                ensure(states.contains(to), "Unknown transition destination.")?;
            }
            let priority = transition
                .get("priority")
                .map(|v| {
                    v.as_i64()
                        .filter(|n| (-32768..=32767).contains(n))
                        .ok_or("Invalid transition priority.")
                })
                .transpose()?
                .unwrap_or(0);
            ensure(
                !priorities.iter().any(|(other, p)| {
                    *p == priority
                        && other["event"] == transition["event"]
                        && (other["from"] == transition["from"]
                            || other["from"] == "*"
                            || transition["from"] == "*")
                }),
                "Ambiguous transition priority.",
            )?;
            priorities.push((transition, priority));
            if let Some(guard) = transition.get("guard") {
                ensure(
                    infer(guard, d, &mut 0, 0)?.kind == "boolean",
                    "Transition guard must be boolean.",
                )?;
            }
            let mut writes = BTreeSet::new();
            if let Some(actions) = transition.get("actions") {
                for action in list(actions, 0, 8)? {
                    match action["type"].as_str() {
                        Some("set" | "emit") => {
                            let set = action["type"] == "set";
                            object(
                                action,
                                if set {
                                    &["type", "signal", "value"]
                                } else {
                                    &["type", "port", "value"]
                                },
                                &[],
                            )?;
                            let target = if set {
                                signals
                                    .iter()
                                    .find(|s| s["id"] == action["signal"] && s["source"] == "local")
                            } else {
                                outputs.iter().find(|o| o["id"] == action["port"])
                            }
                            .ok_or("Unknown or external action target.")?;
                            ensure(
                                assignable(&infer(&action["value"], d, &mut 0, 0)?, target),
                                "Action expression outside target type/range.",
                            )?;
                            ensure(
                                writes.insert(format!(
                                    "{}:{}",
                                    action["type"],
                                    if set {
                                        &action["signal"]
                                    } else {
                                        &action["port"]
                                    }
                                )),
                                "Repeated transition write.",
                            )?;
                        }
                        Some("cue") => {
                            object(action, &["type", "cue"], &[])?;
                            behavior_id(&action["cue"])?;
                        }
                        Some("start-timer" | "cancel-timer") => {
                            object(action, &["type", "timer"], &[])?;
                            ensure(
                                timers.iter().any(|t| t["id"] == action["timer"])
                                    && writes.insert(format!("timer:{}", action["timer"])),
                                "Unknown or repeated timer action.",
                            )?;
                        }
                        _ => return Err("Unsupported behavior action.".into()),
                    }
                }
            }
        }
    }
    ensure(count <= 128, "Behavior transition budget exceeded.")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn definition() -> Value {
        json!({"version":1,
            "signals":[{"id":"count","source":"local","type":"number","min":0,"max":100,"initial":0},
                {"id":"flag","source":"local","type":"boolean","initial":false},
                {"id":"mode","source":"local","type":"enum","values":["idle","active"],"initial":"idle"},
                {"id":"raw","source":"input","type":"number","min":0,"max":100,"initial":0}],
            "events":[{"id":"change","updates":["raw"]},{"id":"expire"}],
            "outputs":[{"id":"count","type":"number","min":0,"max":100,"neutral":0},{"id":"mode","type":"enum","values":["idle","active"],"neutral":"idle"}],
            "timers":[{"id":"timer","event":"expire","delayMs":10,"repeat":true}],
            "regions":[{"id":"main","initial":"idle","states":["idle","active"],"transitions":[
                {"id":"change","from":"*","event":"change","to":"active","guard":{"literal":true},"actions":[
                    {"type":"set","signal":"count","value":{"ref":"raw"}},
                    {"type":"emit","port":"mode","value":{"state":"main"}},
                    {"type":"cue","cue":"click"},{"type":"start-timer","timer":"timer"}]},
                {"id":"expire","from":"*","event":"expire","actions":[{"type":"cancel-timer","timer":"timer"}]}]}]})
    }
    #[test]
    fn behavior_admission_covers_numeric_enum_boolean_expressions_and_explicit_timer_actions() {
        let d = definition();
        validate(&d).unwrap();
        for op in ["add", "sub", "mul", "div"] {
            let mut d = definition();
            d["regions"][0]["transitions"][0]["actions"][0]["value"] =
                json!({"op":op,"left":{"literal":6},"right":{"literal":2}});
            validate(&d).unwrap();
        }
        for guard in [
            json!({"op":"not","value":{"ref":"flag"}}),
            json!({"op":"and","values":[{"literal":true},{"ref":"flag"}]}),
            json!({"op":"or","values":[{"literal":false},{"ref":"flag"}]}),
            json!({"op":"eq","left":{"state":"main"},"right":{"literal":"idle"}}),
            json!({"op":"ne","left":{"ref":"mode"},"right":{"literal":"active"}}),
            json!({"op":"lt","left":{"ref":"raw"},"right":{"literal":50}}),
            json!({"op":"lte","left":{"ref":"raw"},"right":{"literal":50}}),
            json!({"op":"gt","left":{"ref":"raw"},"right":{"literal":50}}),
            json!({"op":"gte","left":{"ref":"raw"},"right":{"literal":50}}),
        ] {
            let mut d = definition();
            d["regions"][0]["transitions"][0]["guard"] = guard;
            validate(&d).unwrap();
        }
        let mut d = definition();
        d["regions"][0]["transitions"][0]["actions"][0]["value"] = json!({"op":"clamp","value":{"op":"add","left":{"ref":"raw"},"right":{"literal":1}},"min":{"literal":0},"max":{"literal":100}});
        validate(&d).unwrap();
        let mut d = definition();
        d["regions"][0]["transitions"][0]["actions"][1]["value"] = json!({"op":"if","condition":{"ref":"flag"},"then":{"literal":"idle"},"else":{"literal":"active"}});
        validate(&d).unwrap();
    }
    #[test]
    fn behavior_admission_rejects_ambiguous_or_unbounded_imported_machine_data() {
        let changes: Vec<Box<dyn Fn(&mut Value)>> = vec![
            Box::new(|d| d["signals"][0]["min"] = json!(100)),
            Box::new(|d| d["signals"][0]["initial"] = json!(-1)),
            Box::new(|d| d["events"][0]["updates"] = json!(["count"])),
            Box::new(|d| d["timers"][0]["delayMs"] = json!(0)),
            Box::new(|d| d["timers"][0]["repeat"] = json!(1)),
            Box::new(|d| d["timers"][0]["event"] = json!("absent")),
            Box::new(|d| d["regions"][0]["initial"] = json!("absent")),
            Box::new(|d| {
                let transition = d["regions"][0]["transitions"][0].clone();
                let mut other = transition.clone();
                other["id"] = json!("other");
                d["regions"][0]["transitions"]
                    .as_array_mut()
                    .unwrap()
                    .push(other);
            }),
            Box::new(|d| {
                d["regions"][0]["transitions"][0]["actions"][0]["value"] =
                    json!({"op":"add","left":{"ref":"raw"},"right":{"literal":1}})
            }),
            Box::new(|d| {
                d["regions"][0]["transitions"][0]["actions"][0]["value"] =
                    json!({"op":"div","left":{"literal":1},"right":{"literal":0}})
            }),
            Box::new(|d| {
                d["regions"][0]["transitions"][0]["actions"][0]["value"] =
                    json!({"op":"mul","left":{"literal":1e9},"right":{"literal":2}})
            }),
            Box::new(|d| {
                d["regions"][0]["transitions"][0]["guard"] =
                    json!({"op":"eval","left":{"literal":1},"right":{"literal":2}})
            }),
            Box::new(|d| {
                d["regions"][0]["transitions"][0]["guard"] =
                    json!({"op":"eq","left":{"literal":1},"right":{"literal":true}})
            }),
            Box::new(
                |d| d["regions"][0]["transitions"][0]["actions"][1]["value"] = json!({"op":"if","condition":{"ref":"flag"},"then":{"literal":"idle"},"else":{"literal":1}}),
            ),
            Box::new(|d| {
                let mut value = json!({"literal":true});
                for _ in 0..8 {
                    value = json!({"op":"not","value":value});
                }
                d["regions"][0]["transitions"][0]["guard"] = value;
            }),
        ];
        for (index, change) in changes.into_iter().enumerate() {
            let mut d = definition();
            change(&mut d);
            assert!(validate(&d).is_err(), "change {index}");
        }
    }
}
