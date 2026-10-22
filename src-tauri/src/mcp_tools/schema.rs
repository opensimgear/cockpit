//! Wire contracts for portable authoring, independent of native ownership.
//! Native serde parsing and compiler validation remain the final authority.
use serde_json::{json, Value};

fn text() -> Value {
    json!({"type":"string"})
}
fn number() -> Value {
    json!({"type":"number"})
}
fn integer() -> Value {
    json!({"type":"integer"})
}
fn boolean() -> Value {
    json!({"type":"boolean"})
}
fn enumeration(values: &[&str]) -> Value {
    json!({"type":"string","enum":values})
}
fn list(item: Value, max: usize) -> Value {
    json!({"type":"array","items":item,"maxItems":max})
}
pub(super) fn reference(name: &str) -> Value {
    json!({"$ref":format!("#/$defs/{name}")})
}
fn nullable(value: Value) -> Value {
    json!({"anyOf":[value,{"type":"null"}]})
}
fn object(properties: Vec<(&str, Value)>, optional: &[&str]) -> Value {
    let required = properties
        .iter()
        .filter(|(name, _)| !optional.contains(name))
        .map(|(name, _)| *name)
        .collect::<Vec<_>>();
    json!({"type":"object","additionalProperties":false,"properties":properties.iter().map(|(name,schema)|(name.to_string(),schema.clone())).collect::<serde_json::Map<_,_>>(),"required":required})
}
fn variant<'a>(
    tag: &'a str,
    name: &str,
    mut properties: Vec<(&'a str, Value)>,
    optional: &[&str],
) -> Value {
    properties.insert(0, (tag, json!({"const":name})));
    object(properties, optional)
}
fn union(variants: Vec<Value>) -> Value {
    json!({"oneOf":variants})
}
fn rounding() -> Value {
    enumeration(&["none", "nearest", "floor", "ceil", "truncate"])
}
fn comparison() -> Value {
    enumeration(&[
        "greater",
        "greater-equal",
        "less",
        "less-equal",
        "equal",
        "not-equal",
    ])
}
fn range() -> Value {
    enumeration(&["unipolar", "bipolar"])
}
fn encoding() -> Value {
    enumeration(&["utf8", "ascii"])
}
fn semantics() -> Value {
    enumeration(&["setter", "one-shot"])
}

pub(super) fn definitions() -> Value {
    let mut defs = serde_json::Map::new();
    defs.insert(
        "PortValue".into(),
        json!({"type":["boolean","number","string"]}),
    );
    defs.insert(
        "ScalarSchema".into(),
        union(vec![
            variant("type", "boolean", vec![], &[]),
            variant(
                "type",
                "integer",
                vec![
                    ("min", integer()),
                    ("max", integer()),
                    ("unit", nullable(text())),
                ],
                &[],
            ),
            variant(
                "type",
                "number",
                vec![
                    ("min", number()),
                    ("max", number()),
                    ("unit", nullable(text())),
                ],
                &[],
            ),
            variant(
                "type",
                "text",
                vec![("maxBytes", integer()), ("encoding", encoding())],
                &[],
            ),
            variant("type", "enum", vec![("values", list(text(), 256))], &[]),
            variant("type", "rgb", vec![], &[]),
        ]),
    );
    defs.insert(
        "ActionParameter".into(),
        object(
            vec![
                ("id", text()),
                ("name", text()),
                ("schema", reference("ScalarSchema")),
            ],
            &[],
        ),
    );
    defs.insert(
        "DisplayConstraints".into(),
        object(
            vec![
                ("columns", integer()),
                ("rows", integer()),
                ("encoding", encoding()),
            ],
            &[],
        ),
    );
    defs.insert(
        "OwnershipCapability".into(),
        object(
            vec![
                ("scope", text()),
                ("maxLeaseMs", integer()),
                ("restorePolicy", enumeration(&["if-unchanged"])),
                ("fallbackPolicy", enumeration(&["capture-at-claim"])),
            ],
            &["restorePolicy", "fallbackPolicy"],
        ),
    );
    defs.insert(
        "PortContract".into(),
        union(vec![
            variant(
                "delivery",
                "state",
                vec![
                    ("schema", reference("ScalarSchema")),
                    ("freshnessMs", integer()),
                    ("fallback", nullable(reference("PortValue"))),
                ],
                &[],
            ),
            variant(
                "delivery",
                "event",
                vec![("schema", reference("ScalarSchema"))],
                &[],
            ),
            variant(
                "delivery",
                "invocation",
                vec![
                    ("parameters", list(reference("ActionParameter"), 4)),
                    ("semantics", semantics()),
                ],
                &[],
            ),
            variant(
                "delivery",
                "action",
                vec![
                    ("parameters", list(reference("ActionParameter"), 4)),
                    ("semantics", semantics()),
                    ("endpoint", text()),
                    ("readable", boolean()),
                    ("display", nullable(reference("DisplayConstraints"))),
                    ("ownership", nullable(reference("OwnershipCapability"))),
                ],
                &[],
            ),
        ]),
    );
    defs.insert(
        "Port".into(),
        object(
            vec![
                ("id", text()),
                ("name", text()),
                ("direction", enumeration(&["input", "output"])),
                ("contract", reference("PortContract")),
            ],
            &[],
        ),
    );
    defs.insert(
        "Resource".into(),
        object(
            vec![
                ("id", text()),
                ("name", text()),
                ("kind", enumeration(&["device", "companion", "game"])),
                ("required", boolean()),
                ("ports", list(reference("Port"), 1024)),
            ],
            &[],
        ),
    );
    defs.insert(
        "Endpoint".into(),
        object(vec![("nodeId", text()), ("portId", text())], &[]),
    );
    defs.insert(
        "Binding".into(),
        object(
            vec![
                ("id", text()),
                ("from", reference("Endpoint")),
                ("to", reference("Endpoint")),
            ],
            &[],
        ),
    );
    defs.insert(
        "GameEligibility".into(),
        object(vec![("gameIds", list(text(), 32))], &[]),
    );
    defs.insert(
        "Point".into(),
        object(vec![("input", number()), ("output", number())], &[]),
    );
    defs.insert(
        "Rule".into(),
        union(vec![
            variant(
                "kind",
                "axis",
                vec![
                    ("deadZone", integer()),
                    ("invert", boolean()),
                    ("scale", integer()),
                    (
                        "curve",
                        list(
                            object(vec![("input", integer()), ("output", integer())], &[]),
                            32,
                        ),
                    ),
                ],
                &[],
            ),
            variant("kind", "merge", vec![], &[]),
            variant("kind", "split", vec![("positive", boolean())], &[]),
            variant(
                "kind",
                "button-axis",
                vec![
                    ("range", range()),
                    ("pressed", integer()),
                    ("released", integer()),
                ],
                &[],
            ),
            variant(
                "kind",
                "threshold",
                vec![("press", integer()), ("release", integer())],
                &[],
            ),
            variant("kind", "chord", vec![], &[]),
            variant("kind", "gate", vec![], &[]),
            variant("kind", "toggle", vec![], &[]),
            variant(
                "kind",
                "gesture",
                vec![
                    ("gesture", enumeration(&["tap", "hold", "double-tap"])),
                    ("holdUs", integer()),
                    ("doubleUs", integer()),
                    ("pulseUs", integer()),
                ],
                &[],
            ),
            variant(
                "kind",
                "pulse",
                vec![("durationUs", integer()), ("repeatUs", integer())],
                &[],
            ),
            variant(
                "kind",
                "macro",
                vec![(
                    "steps",
                    list(
                        object(vec![("afterUs", integer()), ("pressed", boolean())], &[]),
                        16,
                    ),
                )],
                &[],
            ),
        ]),
    );
    defs.insert(
        "Operation".into(),
        union(vec![
            variant("kind", "identity", vec![], &[]),
            variant(
                "kind",
                "declare-quantity",
                vec![("quantity", text()), ("unit", text())],
                &[],
            ),
            variant(
                "kind",
                "constant",
                vec![("value", reference("PortValue"))],
                &[],
            ),
            variant("kind", "not", vec![], &[]),
            variant(
                "kind",
                "scale",
                vec![("factor", number()), ("offset", number())],
                &[],
            ),
            variant(
                "kind",
                "clamp",
                vec![("min", number()), ("max", number())],
                &[],
            ),
            variant(
                "kind",
                "range-map",
                vec![
                    ("inputMin", number()),
                    ("inputMax", number()),
                    ("outputMin", number()),
                    ("outputMax", number()),
                    ("rounding", rounding()),
                ],
                &[],
            ),
            variant(
                "kind",
                "unit-convert",
                vec![
                    ("from", text()),
                    ("to", text()),
                    ("rounding", rounding()),
                    ("clamp", boolean()),
                ],
                &["clamp"],
            ),
            variant(
                "kind",
                "deadzone",
                vec![
                    ("center", number()),
                    ("radius", number()),
                    ("rounding", rounding()),
                ],
                &[],
            ),
            variant("kind", "invert", vec![], &[]),
            variant(
                "kind",
                "curve",
                vec![
                    ("points", list(reference("Point"), 32)),
                    ("rounding", rounding()),
                ],
                &[],
            ),
            variant(
                "kind",
                "compare",
                vec![("comparison", comparison()), ("threshold", number())],
                &[],
            ),
            variant(
                "kind",
                "hysteresis",
                vec![
                    ("press", number()),
                    ("release", number()),
                    ("comparison", comparison()),
                ],
                &[],
            ),
            variant(
                "kind",
                "hat-direction",
                vec![
                    ("direction", enumeration(&["up", "right", "down", "left"])),
                    ("diagonals", boolean()),
                ],
                &[],
            ),
            variant("kind", "gate", vec![], &[]),
            variant("kind", "any", vec![], &[]),
            variant("kind", "all", vec![], &[]),
            variant("kind", "priority", vec![("order", list(text(), 1024))], &[]),
            variant(
                "kind",
                "event-edge",
                vec![("edge", enumeration(&["rising", "falling", "change"]))],
                &[],
            ),
            variant("kind", "event-to-state", vec![], &[]),
            variant(
                "kind",
                "select",
                vec![
                    ("whenTrue", reference("PortValue")),
                    ("whenFalse", reference("PortValue")),
                ],
                &[],
            ),
            variant(
                "kind",
                "format",
                vec![
                    ("decimals", integer()),
                    ("prefix", text()),
                    ("suffix", text()),
                    ("rounding", rounding()),
                    ("overflow", enumeration(&["reject", "truncate"])),
                ],
                &[],
            ),
            variant(
                "kind",
                "fallback",
                vec![
                    ("unknown", reference("PortValue")),
                    ("stale", reference("PortValue")),
                    ("unavailable", reference("PortValue")),
                ],
                &[],
            ),
            variant(
                "kind",
                "action-pack",
                vec![("arguments", list(text(), 4))],
                &[],
            ),
            variant(
                "kind",
                "set-on-change",
                vec![("arguments", list(text(), 4))],
                &[],
            ),
            variant(
                "kind",
                "builtin",
                vec![("rule", reference("Rule")), ("inputs", list(text(), 1024))],
                &[],
            ),
            variant(
                "kind",
                "script",
                vec![
                    ("source", text()),
                    (
                        "parameters",
                        json!({"type":"object","additionalProperties":reference("PortValue")}),
                    ),
                ],
                &[],
            ),
        ]),
    );
    defs.insert(
        "ProcessingDraft".into(),
        object(
            vec![("preset", text()), ("configured", list(text(), 1024))],
            &["configured"],
        ),
    );
    defs.insert(
        "ProcessingNode".into(),
        object(
            vec![
                ("id", text()),
                ("name", text()),
                ("operation", reference("Operation")),
                ("inputs", list(reference("Port"), 1024)),
                ("outputs", list(reference("Port"), 1024)),
                ("draft", reference("ProcessingDraft")),
            ],
            &["draft"],
        ),
    );
    defs.insert(
        "ControllerLayout".into(),
        object(
            vec![
                ("buttons", list(text(), 128)),
                (
                    "axes",
                    list(
                        object(
                            vec![
                                ("portId", text()),
                                ("range", range()),
                                ("fallback", integer()),
                            ],
                            &[],
                        ),
                        8,
                    ),
                ),
                ("hats", list(text(), 4)),
            ],
            &[],
        ),
    );
    defs.insert(
        "VirtualOutput".into(),
        object(
            vec![
                ("id", text()),
                ("name", text()),
                ("required", boolean()),
                ("layout", reference("ControllerLayout")),
                ("ports", list(reference("Port"), 1024)),
            ],
            &[],
        ),
    );
    let button = || enumeration(&["left", "right", "middle", "x1", "x2"]);
    defs.insert(
        "DesktopAction".into(),
        union(vec![
            variant("kind", "key-hold", vec![("key", integer())], &[]),
            variant(
                "kind",
                "key-tap",
                vec![
                    ("key", integer()),
                    ("downMs", integer()),
                    ("upMs", integer()),
                ],
                &[],
            ),
            variant("kind", "chord", vec![("keys", list(integer(), 8))], &[]),
            variant("kind", "mouse-hold", vec![("button", button())], &[]),
            variant(
                "kind",
                "mouse-click",
                vec![
                    ("button", button()),
                    ("downMs", integer()),
                    ("upMs", integer()),
                ],
                &[],
            ),
            variant(
                "kind",
                "wheel",
                vec![("horizontal", integer()), ("vertical", integer())],
                &[],
            ),
            variant(
                "kind",
                "pointer",
                vec![
                    ("axis", enumeration(&["x", "y"])),
                    ("speed", integer()),
                    ("curve", number()),
                ],
                &[],
            ),
            variant(
                "kind",
                "repeat",
                vec![
                    ("key", integer()),
                    ("delayMs", integer()),
                    ("intervalMs", integer()),
                ],
                &[],
            ),
        ]),
    );
    defs.insert(
        "DesktopScope".into(),
        union(vec![
            variant("kind", "selected-game", vec![], &[]),
            variant(
                "kind",
                "application",
                vec![("applicationReference", text())],
                &["applicationReference"],
            ),
            variant("kind", "desktop-wide", vec![], &[]),
        ]),
    );
    defs.insert(
        "DesktopOutput".into(),
        object(
            vec![
                ("id", text()),
                ("name", text()),
                ("required", boolean()),
                ("scope", reference("DesktopScope")),
                ("ports", list(reference("Port"), 1024)),
                (
                    "actions",
                    list(
                        object(
                            vec![("portId", text()), ("action", reference("DesktopAction"))],
                            &[],
                        ),
                        128,
                    ),
                ),
            ],
            &["scope"],
        ),
    );
    defs.insert(
        "VisibilityIntent".into(),
        object(
            vec![
                ("resourceId", text()),
                ("mode", enumeration(&["visible", "hidden-while-active"])),
            ],
            &[],
        ),
    );
    defs.insert(
        "RuntimeOverride".into(),
        object(vec![("resourceId", text()), ("enabled", boolean())], &[]),
    );
    defs.insert(
        "CompanionAssignment".into(),
        object(
            vec![
                ("resourceId", text()),
                ("panelId", nullable(text())),
                ("publicationHash", text()),
            ],
            &["panelId"],
        ),
    );
    defs.insert(
        "VehicleTab".into(),
        object(
            vec![
                ("id", text()),
                ("gameId", text()),
                ("vehicleId", text()),
                ("vehicleName", text()),
                ("nodeIds", list(text(), 337)),
            ],
            &[],
        ),
    );
    defs.insert(
        "Position".into(),
        object(
            vec![("nodeId", text()), ("x", number()), ("y", number())],
            &[],
        ),
    );
    defs.insert(
        "WirePoint".into(),
        object(vec![("x", number()), ("y", number())], &[]),
    );
    defs.insert(
        "EditorLayout".into(),
        object(
            vec![
                ("positions", list(reference("Position"), 336)),
                (
                    "viewport",
                    object(
                        vec![("x", number()), ("y", number()), ("zoom", number())],
                        &[],
                    ),
                ),
                (
                    "groups",
                    list(
                        object(
                            vec![
                                ("id", text()),
                                ("name", text()),
                                ("members", list(text(), 336)),
                                (
                                    "bounds",
                                    object(
                                        vec![
                                            ("x", number()),
                                            ("y", number()),
                                            ("width", number()),
                                            ("height", number()),
                                        ],
                                        &[],
                                    ),
                                ),
                            ],
                            &["bounds"],
                        ),
                        32,
                    ),
                ),
                (
                    "collapsedPortGroups",
                    list(
                        object(
                            vec![
                                ("nodeId", text()),
                                ("group", enumeration(&["outputs", "inputs", "actions"])),
                            ],
                            &[],
                        ),
                        1008,
                    ),
                ),
                ("arrowStyle", enumeration(&["smooth", "orthogonal"])),
                (
                    "wireRoutes",
                    list(
                        object(
                            vec![
                                ("bindingId", text()),
                                ("points", list(reference("WirePoint"), 128)),
                                ("automatic", boolean()),
                                ("handles", list(reference("WirePoint"), 32)),
                            ],
                            &["automatic", "handles"],
                        ),
                        512,
                    ),
                ),
            ],
            &["groups", "collapsedPortGroups", "arrowStyle", "wireRoutes"],
        ),
    );
    defs.insert(
        "LocalAssignment".into(),
        object(
            vec![
                ("resourceId", text()),
                ("sourceId", text()),
                (
                    "ports",
                    list(
                        object(
                            vec![
                                ("profilePortId", text()),
                                ("observedPortId", text()),
                                ("expectedContractHash", text()),
                            ],
                            &[],
                        ),
                        1024,
                    ),
                ),
            ],
            &[],
        ),
    );
    defs.insert(
        "ProfileDocument".into(),
        object(
            vec![
                (
                    "schemaVersion",
                    json!({"const":osg_profiles::PROFILE_SCHEMA_VERSION}),
                ),
                ("id", text()),
                ("name", text()),
                (
                    "revision",
                    json!({"type":"integer","minimum":1,"maximum":osg_profiles::MAX_REVISION}),
                ),
                ("eligibility", reference("GameEligibility")),
                ("resources", list(reference("Resource"), 64)),
                ("bindings", list(reference("Binding"), 512)),
                ("processingNodes", list(reference("ProcessingNode"), 256)),
                ("virtualOutputs", list(reference("VirtualOutput"), 16)),
                ("desktopOutputs", list(reference("DesktopOutput"), 33)),
                ("visibility", list(reference("VisibilityIntent"), 64)),
                ("overrides", list(reference("RuntimeOverride"), 64)),
                (
                    "companionAssignments",
                    list(reference("CompanionAssignment"), 64),
                ),
                ("vehicleTabs", list(reference("VehicleTab"), 32)),
                ("editor", reference("EditorLayout")),
            ],
            &["desktopOutputs", "vehicleTabs"],
        ),
    );
    defs.insert(
        "Edit".into(),
        union(vec![
            variant("kind", "set_name", vec![("name", text())], &[]),
            variant(
                "kind",
                "set_eligibility",
                vec![("eligibility", reference("GameEligibility"))],
                &[],
            ),
            variant(
                "kind",
                "upsert_resource",
                vec![("resource", reference("Resource"))],
                &[],
            ),
            variant("kind", "remove_resource", vec![("id", text())], &[]),
            variant(
                "kind",
                "upsert_binding",
                vec![("binding", reference("Binding"))],
                &[],
            ),
            variant("kind", "remove_binding", vec![("id", text())], &[]),
            variant(
                "kind",
                "upsert_processing_node",
                vec![("node", reference("ProcessingNode"))],
                &[],
            ),
            variant("kind", "remove_processing_node", vec![("id", text())], &[]),
            variant(
                "kind",
                "replace_virtual_outputs",
                vec![("outputs", list(reference("VirtualOutput"), 16))],
                &[],
            ),
            variant(
                "kind",
                "replace_desktop_outputs",
                vec![("outputs", list(reference("DesktopOutput"), 33))],
                &[],
            ),
            variant(
                "kind",
                "replace_visibility",
                vec![("visibility", list(reference("VisibilityIntent"), 64))],
                &[],
            ),
            variant(
                "kind",
                "replace_overrides",
                vec![("overrides", list(reference("RuntimeOverride"), 64))],
                &[],
            ),
            variant(
                "kind",
                "replace_companion_assignments",
                vec![("assignments", list(reference("CompanionAssignment"), 64))],
                &[],
            ),
            variant(
                "kind",
                "replace_vehicle_tabs",
                vec![("tabs", list(reference("VehicleTab"), 32))],
                &[],
            ),
            variant(
                "kind",
                "replace_editor",
                vec![("editor", reference("EditorLayout"))],
                &[],
            ),
        ]),
    );
    Value::Object(defs)
}

pub(super) fn document() -> Value {
    let mut schema = reference("ProfileDocument");
    schema["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
    schema["$defs"] = definitions();
    schema
}

pub(super) fn examples() -> Value {
    let state =
        json!({"delivery":"state","schema":{"type":"boolean"},"freshnessMs":1000,"fallback":false});
    let port = |id: &str, direction: &str| json!({"id":id,"name":id,"direction":direction,"contract":state});
    json!({
        "resource":{"id":"button-source","name":"Observed button","kind":"device","required":true,"ports":[port("pressed","output")]},
        "binding":{"id":"button-to-not","from":{"nodeId":"button-source","portId":"pressed"},"to":{"nodeId":"invert-button","portId":"in"}},
        "processingNode":{"id":"invert-button","name":"Invert button","operation":{"kind":"not"},"inputs":[port("in","input")],"outputs":[port("out","output")]},
        "virtualOutput":{"id":"virtual-pad","name":"Virtual controller","required":true,"layout":{"buttons":["button-1"],"axes":[],"hats":[]},"ports":[port("button-1","input")]},
        "desktopOutput":{"id":"key-output","name":"Physical A key","required":true,"scope":{"kind":"selected-game"},"ports":[port("hold-a","input")],"actions":[{"portId":"hold-a","action":{"kind":"key-hold","key":4}}]},
        "visibility":{"resourceId":"button-source","mode":"hidden-while-active"},"override":{"resourceId":"button-source","enabled":false},
        "companionAssignment":{"resourceId":"logical-screen","panelId":"11111111-1111-4111-8111-111111111111","publicationHash":format!("sha256:{}","a".repeat(64))},
        "vehicleTab":{"id":"aircraft-tab","gameId":"dcs","vehicleId":"FA-18C_hornet","vehicleName":"F/A-18C","nodeIds":["button-source","invert-button"]},
        "guidance":"Examples demonstrate complete native shapes. Replace source ports/contracts and companion hashes with exact discovered native metadata. Native graph/quantity/contract/compiler checks remain authoritative beyond this structural schema."
    })
}

pub(super) fn runtime_config() -> Value {
    let tuple = |bounds: &[(u64, u64)]| json!({"type":"array","minItems":bounds.len(),"maxItems":bounds.len(),"items":false,"prefixItems":bounds.iter().map(|(minimum,maximum)|json!({"type":"integer","minimum":minimum,"maximum":maximum})).collect::<Vec<_>>()});
    let mut schema = object(
        vec![
            (
                "name",
                json!({"type":"string","minLength":1,"maxLength":63,"description":"1–63 UTF-8 bytes without control characters; native validation checks bytes."}),
            ),
            (
                "expanderMask",
                json!({"type":"integer","minimum":0,"maximum":255}),
            ),
            ("buttons", list(tuple(&[(0, 151), (0, 1), (0, 255)]), 128)),
            (
                "axes",
                list(
                    tuple(&[
                        (0, 7),
                        (48, 56),
                        (0, 4094),
                        (1, 4095),
                        (0, 65535),
                        (0, 1),
                        (0, 32767),
                    ]),
                    8,
                ),
            ),
            ("hats", list(tuple(&[(0, 151); 4]), 4)),
            ("encoders", list(tuple(&[(0, 23); 2]), 8)),
            ("selectors", list(tuple(&[(0, 151); 2]), 8)),
        ],
        &[],
    );
    schema["description"]=json!("Full typed module configuration. Buttons=[contact,maintained,exclusiveGroup]; axes=[adcChannel,hidUsage,rawMin,rawMax,neutral,invert,deadzone]; hats=[up,right,down,left]; encoders=[phaseA,phaseB]; selectors=[low,high]. Pure native encoder validation checks counts and calibration; firmware validates physical contact/layout semantics at explicit Apply.");
    schema
}

pub(super) fn operation_catalogue(args: &Value) -> Value {
    let definitions = definitions();
    let all=definitions["Operation"]["oneOf"].as_array().expect("native operation schema").iter().map(|schema| {
        let kind=schema["properties"]["kind"]["const"].as_str().expect("tagged operation");
        let (category,description)=match kind {
            "declare-quantity"=>("quantity","Interpret a unitless raw value as an explicitly named physical quantity without changing its number."),
            "unit-convert"=>("quantity","Convert between supported declared units with explicit rounding and optional clamp."),
            "scale"|"clamp"|"range-map"|"deadzone"|"invert"|"curve"=>("numeric","Transform explicitly declared numeric values; input/output ranges, units and rounding are checked by the native compiler."),
            "compare"|"hysteresis"=>("logic","Produce a boolean condition from a numeric value with explicit thresholds and comparison."),
            "not"|"gate"|"any"|"all"|"priority"|"select"=>("logic","Combine or select explicit declared inputs. Native compilation checks exact input names, count, order, delivery and compatible contracts."),
            "event-edge"|"event-to-state"=>("event","Convert delivery explicitly between state and event; edge/baseline behavior is provided by the native runtime."),
            "hat-direction"=>("logic","Extract a declared cardinal hat direction with explicit diagonal handling."),
            "format"=>("text","Format a numeric input as bounded text with explicit precision, rounding, prefix, suffix and overflow behavior."),
            "action-pack"|"set-on-change"=>("action","Pack ordered declared arguments for an exact action invocation; one-shot and setter semantics remain explicit."),
            "script"=>("script","Run bounded portable mapping script logic over explicitly declared ports and scalar parameters only after native compilation and explicit profile activation."),
            "builtin"=>("logic","Use a bounded native timing/logic Rule such as toggle, pulse, gesture, macro or controller-axis transformation with explicit input names."),
            "identity"|"constant"|"fallback"=>("logic","Provide explicit pass-through, constant, or unknown/stale/unavailable fallback behavior using typed scalar contracts."),
            _=>("other","Native typed processing operation; inspect its parameter schema and compile the proposed declared ports."),
        };
        let parameters=expand(schema,&definitions);
        serde_json::json!({"kind":kind,"name":kind.replace('-'," "),"category":category,"description":description,"parametersSchema":parameters,"schemaHash":super::digest(&parameters)})
    }).collect::<Vec<_>>();
    let search = args["search"].as_str().unwrap_or("").trim().to_lowercase();
    let category = args["category"].as_str().unwrap_or("");
    let offset = args["offset"].as_u64().unwrap_or(0) as usize;
    let limit = args["limit"].as_u64().unwrap_or(32) as usize;
    let filtered = all
        .iter()
        .filter(|item| {
            (category.is_empty() || item["category"] == category)
                && (search.is_empty() || item.to_string().to_lowercase().contains(&search))
        })
        .collect::<Vec<_>>();
    let total = filtered.len();
    let items = filtered
        .into_iter()
        .skip(offset)
        .take(limit)
        .cloned()
        .collect::<Vec<_>>();
    let next = offset + items.len();
    json!({"schemaVersion":1,"profileSchemaVersion":osg_profiles::PROFILE_SCHEMA_VERSION,"schemaHash":super::digest(&expand(&definitions["Operation"],&definitions)),"offset":offset,"total":total,"operations":items,"nextOffset":if next<total{Some(next)}else{None},"guidance":"Copy an exact operation kind/parameters and explicitly declare the ProcessingNode inputs/outputs. The native compiler checks contract compatibility; this catalogue never grants execution or output ownership."})
}
fn expand(value: &Value, definitions: &Value) -> Value {
    if let Some(name) = value["$ref"]
        .as_str()
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
    {
        return expand(&definitions[name], definitions);
    }
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| (key.clone(), expand(value, definitions)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|value| expand(value, definitions))
                .collect(),
        ),
        _ => value.clone(),
    }
}
