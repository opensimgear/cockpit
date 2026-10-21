//! Host admission for the renderer-neutral widget source contract.
//! Keep limits and semantic projection aligned with companion-designer/widget-model.ts.
use super::*;

const STATES: &[&str] = &[
    "normal",
    "active",
    "pressed",
    "warning",
    "stale",
    "unknown",
    "disabled",
    "disconnected",
];
const PAINT: &[&str] = &["fill", "stroke", "strokeWidth", "opacity", "visible"];

fn string(v: &Value) -> Result<&str, String> {
    let s = v.as_str().ok_or("Expected widget text.")?;
    ensure(
        s.encode_utf16().count() <= 120 && !s.chars().any(|c| c <= '\u{1f}'),
        "Invalid widget text.",
    )?;
    Ok(s)
}
fn color(v: &Value) -> Result<(), String> {
    let s = v.as_str().ok_or("Expected widget color.")?;
    ensure(
        s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid widget color.",
    )
}
fn property_value(p: &Value, v: &Value, assets: &[Value]) -> Result<(), String> {
    match p["type"].as_str() {
        Some("number") => {
            number(
                v,
                p["min"].as_f64().ok_or("Missing minimum.")?,
                p["max"].as_f64().ok_or("Missing maximum.")?,
            )?;
        }
        Some("boolean") => ensure(v.is_boolean(), "Expected widget boolean.")?,
        Some("color") => color(v)?,
        Some("asset") => {
            let id = string(v)?;
            ensure(
                id.is_empty() || assets.iter().any(|a| a["id"] == id),
                "Missing widget asset.",
            )?;
        }
        Some("text") => {
            string(v)?;
        }
        _ => return Err("Unsupported widget property type.".into()),
    }
    Ok(())
}
fn layer_value(
    v: &Value,
    kind: &str,
    properties: &[Value],
    assets: &[Value],
    min: f64,
    max: f64,
) -> Result<(), String> {
    if v.is_object() {
        object(v, &["property"], &[])?;
        let p = properties
            .iter()
            .find(|p| p["name"] == v["property"])
            .ok_or("Unknown widget property.")?;
        ensure(p["type"] == kind, "Incompatible widget property reference.")?;
        if kind == "number" {
            ensure(
                number(&p["min"], -1e9, 1e9)? >= min && number(&p["max"], -1e9, 1e9)? <= max,
                "Widget property range exceeds layer bounds.",
            )?;
        }
    } else if kind == "number" {
        number(v, min, max)?;
    } else {
        property_value(&json!({"type":kind}), v, assets)?;
    }
    Ok(())
}
fn paint(v: &Value, properties: &[Value], assets: &[Value]) -> Result<(), String> {
    for (key, kind, min, max) in [
        ("fill", "color", 0., 1.),
        ("stroke", "color", 0., 1.),
        ("strokeWidth", "number", 0., 16.),
        ("opacity", "number", 0., 1.),
        ("visible", "boolean", 0., 1.),
    ] {
        if let Some(value) = v.get(key) {
            layer_value(value, kind, properties, assets, min, max)?;
        }
    }
    Ok(())
}
fn resolved<'a>(v: &'a Value, properties: &'a Value) -> &'a Value {
    v.get("property")
        .and_then(Value::as_str)
        .and_then(|name| properties.get(name))
        .unwrap_or(v)
}
fn properties(definition: &Value, overrides: &Value, assets: &[Value]) -> Result<Value, String> {
    let definitions = definition["properties"]
        .as_array()
        .ok_or("Missing widget properties.")?;
    let names: Vec<_> = definitions
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    object(overrides, &[], &names)?;
    let mut values = serde_json::Map::new();
    for p in definitions {
        let name = p["name"].as_str().unwrap();
        let value = overrides.get(name).unwrap_or(&p["default"]);
        property_value(p, value, assets)?;
        values.insert(name.to_owned(), value.clone());
    }
    Ok(Value::Object(values))
}

pub(super) fn definitions<'a>(
    value: &'a Value,
    assets: &[Value],
) -> Result<Vec<&'a Value>, String> {
    let mut revisions = BTreeSet::new();
    let mut result = Vec::new();
    for d in list(value, 0, 64)? {
        object(
            d,
            &[
                "formatVersion",
                "id",
                "version",
                "title",
                "size",
                "properties",
                "inputs",
                "outputs",
                "interaction",
                "hitArea",
                "layers",
                "state",
            ],
            &[],
        )?;
        ensure(
            serde_json::to_vec(d).map_err(|e| e.to_string())?.len() <= 65536,
            "Widget definition exceeds budget.",
        )?;
        ensure(
            d["formatVersion"] == 1,
            "Unsupported widget definition version.",
        )?;
        let id = stable(&d["id"])?;
        let version = integer(&d["version"], 1, 65535)?;
        ensure(
            revisions.insert((id, version)),
            "Duplicate widget definition revision.",
        )?;
        text(&d["title"])?;
        object(&d["size"], &["width", "height"], &[])?;
        integer(&d["size"]["width"], 32, 1024)?;
        integer(&d["size"]["height"], 32, 1024)?;
        bounds(&d["hitArea"], "Widget hit area outside component.")?;
        let props = list(&d["properties"], 0, 24)?;
        let mut names = BTreeSet::new();
        for p in props {
            object(
                p,
                &["name", "label", "type", "default"],
                if p["type"] == "number" {
                    &["min", "max"]
                } else {
                    &[]
                },
            )?;
            ensure(
                names.insert(stable(&p["name"])?),
                "Duplicate widget property.",
            )?;
            string(&p["label"])?;
            if p["type"] == "number" {
                ensure(
                    number(&p["min"], -1e9, 1e9)? < number(&p["max"], -1e9, 1e9)?,
                    "Invalid widget property range.",
                )?;
            }
            property_value(p, &p["default"], assets)?;
        }
        let inputs = list(&d["inputs"], 0, 8)?;
        let outputs = list(&d["outputs"], 0, 8)?;
        let mut ports = BTreeSet::new();
        for p in inputs.iter().chain(outputs) {
            ensure(ports.insert(stable(&p["name"])?), "Duplicate widget port.")?;
            string(&p["label"])?;
        }
        for p in inputs {
            object(
                p,
                &["name", "label", "kind"],
                match p["kind"].as_str() {
                    Some("axis") => &["axisMode", "fallback"],
                    Some("hat") => &["fallback"],
                    _ => &[],
                },
            )?;
            match p["kind"].as_str() {
                Some("button") => (),
                Some("hat") => ensure(p["fallback"] == -1, "Invalid widget hat fallback.")?,
                Some("axis") => {
                    ensure(
                        matches!(p["axisMode"].as_str(), Some("bipolar" | "unipolar")),
                        "Invalid widget axis mode.",
                    )?;
                    number(
                        &p["fallback"],
                        if p["axisMode"] == "bipolar" { -1. } else { 0. },
                        1.,
                    )?;
                }
                _ => return Err("Unsupported widget input port.".into()),
            }
        }
        for p in outputs {
            object(p, &["name", "label", "kind"], &["unit", "range"])?;
            ensure(
                matches!(p["kind"].as_str(), Some("indicator" | "text" | "gauge")),
                "Unsupported widget output port.",
            )?;
            if let Some(unit) = p.get("unit") {
                string(unit)?;
            }
            if let Some(range) = p.get("range") {
                object(range, &["min", "max"], &[])?;
                ensure(
                    p["kind"] == "gauge"
                        && number(&range["min"], -1e9, 1e9)? < number(&range["max"], -1e9, 1e9)?,
                    "Invalid widget output range.",
                )?;
            }
        }
        let interaction = &d["interaction"];
        object(
            interaction,
            &["kind"],
            match interaction["kind"].as_str() {
                Some("momentary" | "toggle") => &["port"],
                Some("selector") => &["port", "options"],
                Some("navigate") => &["pageProperty"],
                _ => &[],
            },
        )?;
        match interaction["kind"].as_str() {
            Some("momentary" | "toggle" | "selector") => {
                let p = inputs
                    .iter()
                    .find(|p| p["name"] == interaction["port"])
                    .ok_or("Unknown interaction port.")?;
                ensure(
                    inputs.len() == 1
                        && p["kind"]
                            == if interaction["kind"] == "selector" {
                                "axis"
                            } else {
                                "button"
                            },
                    "Incompatible widget interaction port.",
                )?;
                if interaction["kind"] == "selector" {
                    let mut values = Vec::new();
                    for o in list(&interaction["options"], 2, 16)? {
                        object(o, &["label", "value"], &[])?;
                        string(&o["label"])?;
                        let n = number(
                            &o["value"],
                            if p["axisMode"] == "bipolar" { -1. } else { 0. },
                            1.,
                        )?;
                        ensure(!values.contains(&n), "Duplicate selector value.")?;
                        values.push(n);
                    }
                    ensure(
                        values.contains(&p["fallback"].as_f64().unwrap()),
                        "Selector fallback needs an option.",
                    )?;
                }
            }
            Some("none" | "navigate") => {
                ensure(
                    inputs.is_empty(),
                    "Presentation widgets cannot own gameplay inputs.",
                )?;
                if interaction["kind"] == "navigate" {
                    ensure(
                        props.iter().any(|p| {
                            p["name"] == interaction["pageProperty"] && p["type"] == "text"
                        }),
                        "Navigation needs a page property.",
                    )?;
                }
            }
            _ => return Err("Unsupported widget interaction.".into()),
        }
        for p in object(
            &d["state"],
            &[],
            &["confirmedPort", "warningPort", "enabledPort"],
        )?
        .values()
        {
            ensure(
                outputs
                    .iter()
                    .any(|o| o["name"] == *p && o["kind"] == "indicator"),
                "State requires an indicator output.",
            )?;
        }
        let mut layer_ids = BTreeSet::new();
        for layer in list(&d["layers"], 1, 32)? {
            let kind = layer["kind"].as_str().ok_or("Missing layer kind.")?;
            let mut optional = vec![
                "fill",
                "stroke",
                "strokeWidth",
                "opacity",
                "visible",
                "states",
            ];
            optional.extend_from_slice(match kind {
                "rectangle" => &["radius"][..],
                "ellipse" | "line" => &[],
                "text" => &[
                    "text",
                    "font",
                    "fontSize",
                    "align",
                    "outputPort",
                    "precision",
                    "unit",
                ],
                "image" => &["assetId"],
                "indicator" => &["outputPort"],
                "gauge" => &["outputPort", "min", "max", "orientation"],
                _ => return Err("Unsupported widget layer.".into()),
            });
            object(layer, &["id", "label", "kind", "bounds"], &optional)?;
            ensure(
                layer_ids.insert(stable(&layer["id"])?),
                "Duplicate widget layer.",
            )?;
            string(&layer["label"])?;
            bounds(&layer["bounds"], "Widget layer outside component.")?;
            paint(layer, props, assets)?;
            if kind == "rectangle" {
                if let Some(radius) = layer.get("radius") {
                    layer_value(radius, "number", props, assets, 0., 64.)?;
                }
            }
            if kind == "image" {
                layer_value(&layer["assetId"], "asset", props, assets, 0., 1.)?;
            }
            if kind == "text" {
                layer_value(&layer["text"], "text", props, assets, 0., 1.)?;
                ensure(
                    matches!(layer["font"].as_str(), Some("sans" | "mono"))
                        && matches!(layer["align"].as_str(), Some("left" | "center" | "right")),
                    "Unsupported widget typography.",
                )?;
                integer(&layer["fontSize"], 8, 128)?;
                if let Some(precision) = layer.get("precision") {
                    integer(precision, 0, 4)?;
                }
                if let Some(unit) = layer.get("unit") {
                    string(unit)?;
                }
                if let Some(port) = layer.get("outputPort") {
                    ensure(
                        outputs.iter().any(|o| o["name"] == *port),
                        "Unknown widget text output.",
                    )?;
                }
            }
            if kind == "indicator" || kind == "gauge" {
                ensure(
                    outputs.iter().any(|o| {
                        o["name"] == layer["outputPort"]
                            && o["kind"]
                                == if kind == "indicator" {
                                    "indicator"
                                } else {
                                    "gauge"
                                }
                    }),
                    "Incompatible widget layer output.",
                )?;
            }
            if kind == "gauge" {
                layer_value(&layer["min"], "number", props, assets, -1e9, 1e9)?;
                layer_value(&layer["max"], "number", props, assets, -1e9, 1e9)?;
                ensure(
                    matches!(
                        layer["orientation"].as_str(),
                        Some("horizontal" | "vertical" | "radial")
                    ),
                    "Invalid gauge orientation.",
                )?;
            }
            if let Some(states) = layer.get("states") {
                for paint_value in object(states, &[], STATES)?.values() {
                    let mut allowed = PAINT.to_vec();
                    if kind == "text" {
                        allowed.push("text");
                    }
                    object(paint_value, &[], &allowed)?;
                    paint(paint_value, props, assets)?;
                    if let Some(text) = paint_value.get("text") {
                        layer_value(text, "text", props, assets, 0., 1.)?;
                    }
                }
            }
        }
        let defaults = properties(d, &json!({}), assets)?;
        validate_ranges(d, &defaults)?;
        result.push(d);
    }
    Ok(result)
}
fn validate_ranges(d: &Value, properties: &Value) -> Result<(), String> {
    for layer in d["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["kind"] == "gauge")
    {
        ensure(
            number(resolved(&layer["min"], properties), -1e9, 1e9)?
                < number(resolved(&layer["max"], properties), -1e9, 1e9)?,
            "Invalid instance gauge range.",
        )?;
    }
    Ok(())
}
pub(super) fn instance<'a>(
    w: &Value,
    definitions: &[&'a Value],
    ui: &Value,
    assets: &[Value],
) -> Result<(&'a Value, Value), String> {
    let config = &w["config"];
    object(
        config,
        &["definitionId", "definitionVersion", "properties"],
        &[],
    )?;
    let d = definitions
        .iter()
        .find(|d| d["id"] == config["definitionId"] && d["version"] == config["definitionVersion"])
        .ok_or("Unknown widget definition revision.")?;
    let props = properties(d, &config["properties"], assets)?;
    validate_ranges(d, &props)?;
    if d["interaction"]["kind"] == "navigate" {
        let name = d["interaction"]["pageProperty"].as_str().unwrap();
        ensure(
            ui["pages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|page| page["id"] == props[name]),
            "Unknown navigation page.",
        )?;
    }
    let strip_labels = |ports: &Value| -> Value {
        Value::Array(
            ports
                .as_array()
                .unwrap()
                .iter()
                .map(|p| {
                    let mut p = p.clone();
                    p.as_object_mut().unwrap().remove("label");
                    p
                })
                .collect(),
        )
    };
    let mut semantics = json!({"inputs":strip_labels(&d["inputs"]),"outputs":strip_labels(&d["outputs"]),"interaction":d["interaction"]});
    if d["interaction"]["kind"] == "navigate" {
        semantics["page"] = props[d["interaction"]["pageProperty"].as_str().unwrap()].clone();
    }
    Ok((d, semantics))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> PublicationUpload {
        serde_json::from_str(match name {
            "styling" => include_str!("../../tests/fixtures/widgets-styling.json"),
            "behavior" => include_str!("../../tests/fixtures/widgets-behavior.json"),
            _ => include_str!("../../tests/fixtures/widgets-base.json"),
        })
        .unwrap()
    }
    fn mutate(change: impl FnOnce(&mut Value)) -> Result<Publication, String> {
        let mut upload = fixture("base");
        let mut doc: Value = serde_json::from_str(&upload.canonical_document).unwrap();
        change(&mut doc);
        upload.canonical_document = serde_json_canonicalizer::to_string(&doc).unwrap();
        upload.publication_hash = format!("sha256:{}", hash(upload.canonical_document.as_bytes()));
        upload.capability_fingerprint = None;
        Publication::validate(upload)
    }
    #[test]
    fn shared_widget_native_admission_matches_actual_typescript_publications() {
        let base = Publication::validate(fixture("base")).unwrap();
        let styling = Publication::validate(fixture("styling")).unwrap();
        let behavior = Publication::validate(fixture("behavior")).unwrap();
        assert_eq!(base.ui_id, "shared-systems");
        assert_eq!(base.controls.len(), 3);
        assert_eq!(base.capability_fingerprint, styling.capability_fingerprint);
        assert_ne!(base.publication_hash, styling.publication_hash);
        assert_ne!(
            styling.capability_fingerprint,
            behavior.capability_fingerprint
        );
    }
    #[test]
    fn shared_widget_host_rejects_malformed_definitions_before_admission() {
        let cases: &[(&str, Value)] = &[
            ("/presentation/definitions/0/formatVersion", json!(2)),
            ("/presentation/definitions/0/version", json!(0)),
            ("/presentation/definitions/0/id", json!("bad/id")),
            ("/presentation/definitions/0/title", json!("")),
            ("/presentation/definitions/0/size/width", json!(0)),
            ("/presentation/definitions/0/hitArea/width", json!(2)),
            (
                "/presentation/definitions/0/properties/0/type",
                json!("script"),
            ),
            (
                "/presentation/definitions/0/properties/1/default",
                json!("red"),
            ),
            (
                "/presentation/definitions/0/inputs/0/kind",
                json!("keyboard"),
            ),
            (
                "/presentation/definitions/0/outputs/0/kind",
                json!("javascript"),
            ),
            (
                "/presentation/definitions/0/interaction/port",
                json!("missing"),
            ),
            (
                "/presentation/definitions/0/interaction/kind",
                json!("none"),
            ),
            (
                "/presentation/definitions/0/state/confirmedPort",
                json!("missing"),
            ),
            (
                "/presentation/definitions/0/layers/0/kind",
                json!("webview"),
            ),
            (
                "/presentation/definitions/0/layers/0/stroke/property",
                json!("missing"),
            ),
            ("/presentation/definitions/0/layers/0/radius", json!(100)),
            ("/presentation/definitions/0/layers/1/font", json!("remote")),
            ("/presentation/definitions/0/layers/1/fontSize", json!(0)),
            (
                "/presentation/definitions/0/layers/2/outputPort",
                json!("missing"),
            ),
            (
                "/presentation/definitions/2/inputs/0/axisMode",
                json!("other"),
            ),
            (
                "/presentation/definitions/2/interaction/options/1/value",
                json!(0),
            ),
            (
                "/presentation/definitions/5/layers/3/max/property",
                json!("missing"),
            ),
            (
                "/presentation/widgets/0/config/definitionVersion",
                json!(99),
            ),
            ("/presentation/widgets/0/controlIds", json!([])),
            ("/presentation/widgets/0/feedbackIds", json!([])),
            (
                "/presentation/widgets/9/config/properties/page",
                json!("missing"),
            ),
            ("/ui/controls/2/fallback", json!(0.5)),
        ];
        for (path, value) in cases {
            let result = mutate(|doc| *doc.pointer_mut(path).unwrap() = value.clone());
            assert!(result.is_err(), "accepted malformed widget field {path}");
        }
        assert!(
            mutate(|doc| doc["presentation"]["widgets"][0]["config"]["extra"] = json!(true))
                .is_err()
        );
        assert!(mutate(
            |doc| doc["presentation"]["definitions"][0]["layers"][0]["states"]["active"]
                ["bounds"] = json!({"x":0})
        )
        .is_err());
        assert!(mutate(|doc| {
            let duplicate = doc["presentation"]["definitions"][0].clone();
            doc["presentation"]["definitions"]
                .as_array_mut()
                .unwrap()
                .push(duplicate);
        })
        .is_err());
        assert!(mutate(
            |doc| doc["presentation"]["widgets"][5]["config"]["properties"] =
                json!({"minimum":100,"maximum":10})
        )
        .is_err());
    }
}
