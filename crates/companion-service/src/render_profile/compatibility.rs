use super::validate;
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::BTreeSet, sync::OnceLock};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RenderCompatibilityIssue {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub widget: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    pub field: String,
    pub required: Value,
    pub available: Value,
    pub message: String,
}
#[derive(Default)]
struct Check {
    issues: Vec<RenderCompatibilityIssue>,
    widget: Option<String>,
    definition: Option<String>,
    layer: Option<String>,
    target: String,
}
impl Check {
    fn add(&mut self, field: &str, required: Value, available: Value) {
        let owner = self
            .widget
            .as_ref()
            .map(|id| format!("Widget {id}"))
            .or_else(|| {
                self.definition
                    .as_ref()
                    .map(|id| format!("Definition {id}"))
            })
            .unwrap_or("Panel".into());
        let display = |v: &Value| {
            v.as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| v.to_string())
        };
        let layer = self
            .layer
            .as_ref()
            .map(|id| format!(" layer {id}"))
            .unwrap_or_default();
        let issue = RenderCompatibilityIssue {
            widget: self.widget.clone(),
            definition: self.definition.clone(),
            layer: self.layer.clone(),
            field: field.into(),
            message: format!(
                "{owner}{layer}: {field} requires {}; {} provides {}.",
                display(&required),
                self.target,
                display(&available)
            ),
            required,
            available,
        };
        if !self.issues.contains(&issue) {
            self.issues.push(issue);
        }
    }
    fn over(&mut self, field: &str, required: usize, available: usize) {
        if required > available {
            self.add(field, json!(required), json!(available));
        }
    }
    fn feature(&mut self, profile: &Value, name: &str) {
        if !items(&profile["features"])
            .iter()
            .any(|value| value == name)
        {
            self.add(name, json!("supported"), json!("unavailable"));
        }
    }
    fn font(&mut self, text: &str, face: &str, field: &str) {
        static BANK: OnceLock<Value> = OnceLock::new();
        let bank = BANK.get_or_init(|| {
            serde_json::from_str(include_str!(
                "../../../../packages/companion-ui/assets/fonts/metrics.json"
            ))
            .expect("pinned font coverage")
        });
        let advances = items(&bank["fonts"][face]["advances"]);
        let mut seen = BTreeSet::new();
        let missing = text
            .chars()
            .filter(|ch| {
                !advances
                    .iter()
                    .any(|pair| pair[0].as_u64() == Some(*ch as u64))
                    && seen.insert(*ch)
            })
            .take(8)
            .map(|ch| format!("U+{:04X}", ch as u32))
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            self.add(
                field,
                json!(missing.join(", ")),
                json!(format!("{face} glyph set")),
            );
        }
    }
}
fn items(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn string(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}
fn number(value: &Value) -> f64 {
    value.as_f64().unwrap_or(0.0)
}
fn count_nodes(value: &Value) -> usize {
    1 + match value {
        Value::Array(values) => values.iter().map(count_nodes).sum::<usize>(),
        Value::Object(values) => values.values().map(count_nodes).sum(),
        _ => 0,
    }
}
fn resolve(value: &Value, properties: &Value) -> Value {
    value["property"]
        .as_str()
        .and_then(|name| properties.get(name))
        .unwrap_or(value)
        .clone()
}
fn layers(definition: &Value, overrides: &Value, state: &str) -> Vec<Value> {
    let mut properties = json!({});
    for property in items(&definition["properties"]) {
        let name = string(&property["name"]);
        properties[name] = overrides.get(name).unwrap_or(&property["default"]).clone();
    }
    items(&definition["layers"])
        .iter()
        .map(|original| {
            let mut layer = original.clone();
            layer.as_object_mut().unwrap().remove("states");
            for key in ["normal", state] {
                if let Some(paint) = original["states"][key].as_object() {
                    for (field, value) in paint {
                        layer[field] = value.clone();
                    }
                }
            }
            for value in layer.as_object_mut().unwrap().values_mut() {
                *value = resolve(value, &properties);
            }
            layer
        })
        .collect()
}
/// Called only with an admitted immutable publication. Static checks cannot
/// promise current free heap/cache space; device preflight remains authoritative.
pub(crate) fn check_document(
    document: &Value,
    report: &Value,
) -> Result<Vec<RenderCompatibilityIssue>, String> {
    let profile = validate(report)?;
    let limits = &profile["limits"];
    let limit = |key: &str| limits[key].as_u64().unwrap() as usize;
    let mut check = Check {
        target: string(&profile["id"]).into(),
        ..Default::default()
    };
    let widgets = items(&document["presentation"]["widgets"]);
    let definitions = items(&document["presentation"]["definitions"]);
    check.feature(&profile, &format!("panel.v{}", document["schemaVersion"]));
    check.over(
        "documentBytes",
        serde_json_canonicalizer::to_vec(document)
            .map_err(|e| e.to_string())?
            .len(),
        limit("documentBytes"),
    );
    check.over("json.nodes", count_nodes(document), limit("jsonNodes"));
    for (field, values, available) in [
        ("pages", &document["ui"]["pages"], limit("pages")),
        (
            "widgets",
            &document["presentation"]["widgets"],
            limit("widgets"),
        ),
        ("controls", &document["ui"]["controls"], limit("widgets")),
        (
            "outputs",
            &document["ui"]["feedbackWidgets"],
            limit("outputs"),
        ),
        (
            "definitions",
            &document["presentation"]["definitions"],
            limit("definitions"),
        ),
    ] {
        check.over(field, items(values).len(), available);
    }
    let controls = items(&document["ui"]["controls"]).len();
    let bindings = items(&document["bindings"]).len();
    if controls != bindings {
        check.add("bindings", json!(controls), json!(bindings));
    }
    for page in items(&document["ui"]["pages"]) {
        if !widgets.iter().any(|widget| widget["pageId"] == page["id"]) {
            check.add(
                &format!("page.{}.widgets", string(&page["id"])),
                json!(1),
                json!(0),
            );
        }
    }
    for definition in definitions {
        check.definition = Some(string(&definition["id"]).into());
        check.over(
            "layers",
            items(&definition["layers"]).len(),
            limit("layersPerWidget").min(limit("totalLayers")),
        );
        check.over(
            "inputPorts",
            items(&definition["inputs"]).len(),
            limit("portsPerWidget"),
        );
        check.over(
            "outputPorts",
            items(&definition["outputs"]).len(),
            limit("portsPerWidget"),
        );
    }
    check.definition = None;
    for viewport in items(&document["presentation"]["viewports"]) {
        if viewport.get("backgroundAssetId").is_some() {
            check.add(
                "viewport.backgroundAssetId",
                json!("supported"),
                json!("unavailable"),
            );
        }
    }
    let groups = items(&document["presentation"]["groups"]);
    if !groups.is_empty() {
        check.feature(&profile, "group.v1");
    }
    for group in groups {
        if widgets
            .iter()
            .any(|widget| widget["groupId"] == group["id"])
        {
            check.widget = Some(string(&group["id"]).into());
            check.font(string(&group["label"]), "sans", "group.label");
        }
    }
    let (mut components, mut total_layers, mut references, mut image_bytes) = (0, 0, 0, 0);
    let mut rasters = BTreeSet::new();
    let mut image = |check: &mut Check, asset: &str, width: usize, height: usize| {
        references += 1;
        check.over("imageReferences", references, limit("imageReferences"));
        if width < 1 || height < 1 {
            check.add("image.minimumPixels", json!(1), json!(width.min(height)));
        }
        check.over("image.width", width, limit("imageWidth"));
        check.over("image.height", height, limit("imageHeight"));
        check.over("image.pixels", width * height, limit("imagePixels"));
        let hash = items(&document["ui"]["assets"])
            .iter()
            .find(|item| item["id"] == asset)
            .map(|item| string(&item["sha256"]))
            .unwrap_or("");
        if rasters.insert(format!("{hash}:{width}:{height}")) {
            image_bytes += width * height * 3;
            check.over("imageBytes", image_bytes, limit("imageBytes"));
        }
    };
    let mut touch_areas: Vec<(&Value, [f64; 4], bool)> = Vec::new();
    for widget in widgets {
        check.widget = Some(string(&widget["id"]).into());
        check.definition = None;
        let viewport = items(&document["presentation"]["viewports"])
            .iter()
            .find(|viewport| viewport["pageId"] == widget["pageId"])
            .ok_or("Missing admitted viewport.")?;
        let inset = &viewport["safeInsets"];
        let scale = (number(&profile["screen"]["width"]) / number(&viewport["width"]))
            .min(number(&profile["screen"]["height"]) / number(&viewport["height"]));
        let mut width = number(&widget["bounds"]["width"])
            * (1.0 - number(&inset["left"]) - number(&inset["right"]))
            * number(&viewport["width"])
            * scale;
        let mut height = number(&widget["bounds"]["height"])
            * (1.0 - number(&inset["top"]) - number(&inset["bottom"]))
            * number(&viewport["height"])
            * scale;
        let (mut pw, mut ph) = (width.floor() as usize, height.floor() as usize);
        let mut artwork = [0.0, 0.0, 1.0, 1.0];
        let config = &widget["config"];
        let mut hit = [0.0, 0.0, 1.0, 1.0];
        let mut interactive = widget["kind"] == "button";
        if widget["kind"] == "component" {
            let definition = definitions
                .iter()
                .find(|definition| {
                    definition["id"] == config["definitionId"]
                        && definition["version"] == config["definitionVersion"]
                })
                .ok_or("Missing admitted definition.")?;
            let fit = (width / number(&definition["size"]["width"]))
                .min(height / number(&definition["size"]["height"]));
            let w = number(&definition["size"]["width"]) * fit;
            let h = number(&definition["size"]["height"]) * fit;
            if width > 0.0 && height > 0.0 {
                artwork = [
                    (1.0 - w / width) / 2.0,
                    (1.0 - h / height) / 2.0,
                    w / width,
                    h / height,
                ];
            }
            width = w;
            height = h;
            pw = w.floor() as usize;
            ph = h.floor() as usize;
            hit = ["x", "y", "width", "height"].map(|field| number(&definition["hitArea"][field]));
            interactive = definition["interaction"]["kind"] != "none";
            check.definition = Some(string(&definition["id"]).into());
            components += 1;
            total_layers += items(&definition["layers"]).len();
            check.over("components", components, limit("components"));
            check.over("layers", total_layers, limit("totalLayers"));
            let mut features = BTreeSet::from([
                "widget.v1".to_owned(),
                format!("interaction.{}", string(&definition["interaction"]["kind"])),
            ]);
            for layer in items(&definition["layers"]) {
                features.insert(format!(
                    "layer.{}",
                    if layer["kind"] == "gauge" {
                        format!("gauge.{}", string(&layer["orientation"]))
                    } else {
                        string(&layer["kind"]).into()
                    }
                ));
                if layer["kind"] == "text" {
                    features.insert(format!("font.{}", string(&layer["font"])));
                }
            }
            for name in features {
                check.feature(&profile, &name);
            }
            if pw < 1 || ph < 1 {
                check.add("geometry.minimumPixels", json!(1), json!(pw.min(ph)));
            }
            check.over(
                "scale",
                ((pw as f64 / number(&definition["size"]["width"]))
                    .max(ph as f64 / number(&definition["size"]["height"])))
                .ceil() as usize,
                limit("scale"),
            );
            if definition["interaction"]["kind"] != "none" {
                let touch = (width * number(&definition["hitArea"]["width"]))
                    .floor()
                    .min((height * number(&definition["hitArea"]["height"])).floor())
                    as usize;
                if touch < limit("touchPixels") {
                    check.add(
                        "hitArea.minimumPixels",
                        json!(limit("touchPixels")),
                        json!(touch),
                    );
                }
            }
            let states = [
                "normal",
                "active",
                "pressed",
                "warning",
                "stale",
                "unknown",
                "disabled",
                "disconnected",
            ]
            .map(|state| layers(definition, &config["properties"], state));
            for (index, layer) in states[0].iter().enumerate() {
                check.layer = Some(string(&layer["id"]).into());
                if layer["kind"] == "image" && !string(&layer["assetId"]).is_empty() {
                    image(
                        &mut check,
                        string(&layer["assetId"]),
                        (pw as f64 * number(&layer["bounds"]["width"])).round() as usize,
                        (ph as f64 * number(&layer["bounds"]["height"])).round() as usize,
                    );
                }
                for state in &states {
                    let layer = &state[index];
                    if layer["visible"] == false || layer["opacity"] == 0 || layer["kind"] != "text"
                    {
                        continue;
                    }
                    check.over(
                        "fontSize",
                        number(&layer["fontSize"]) as usize,
                        limit("fontSize"),
                    );
                    let face = string(&layer["font"]);
                    if let Some(port) = layer.get("outputPort") {
                        check.font("—STALE", face, "text.glyphs");
                        if let Some(output) = items(&definition["outputs"])
                            .iter()
                            .find(|output| output["name"] == *port)
                        {
                            if output["kind"] == "gauge" {
                                check.font(
                                    &format!("0123456789.- {}", string(&layer["unit"])),
                                    face,
                                    "text.glyphs",
                                );
                            }
                            if output["kind"] == "indicator" {
                                check.font("truefalse", face, "text.glyphs");
                            }
                        }
                    } else {
                        check.font(string(&layer["text"]), face, "text.glyphs");
                    }
                }
                check.layer = None;
            }
        } else {
            check.feature(&profile, &format!("legacy.{}", string(&widget["kind"])));
            for (key, value) in [
                ("label", &widget["label"]),
                ("prefix", &config["prefix"]),
                ("suffix", &config["suffix"]),
                ("unit", &config["unit"]),
            ] {
                if let Some(text) = value.as_str() {
                    if !text.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || b" .,:;-_+=|/%!()?".contains(&byte)
                    }) {
                        check.add(
                            &format!("legacy.{key}.glyphs"),
                            json!("supported text"),
                            json!("basic device glyph set"),
                        );
                    }
                    if key == "unit" {
                        check.over("legacy.unitBytes", text.len(), 32);
                    }
                }
            }
            if config.get("iconAssetId").is_some() {
                check.add(
                    "button.iconAssetId",
                    json!("supported"),
                    json!("unavailable"),
                );
            }
            if widget["kind"] == "button" {
                if config["buttonMode"] != "momentary"
                    || config["pulseMs"] != 0
                    || config["repeatMs"] != 0
                {
                    check.add(
                        "button.interaction",
                        json!("momentary without pulse/repeat"),
                        config["buttonMode"].clone(),
                    );
                }
                let touch = pw.min(ph);
                if touch < limit("touchPixels") {
                    check.add(
                        "hitArea.minimumPixels",
                        json!(limit("touchPixels")),
                        json!(touch),
                    );
                }
            }
            if widget["kind"] == "numeric" || widget["kind"] == "gauge" {
                check.over(
                    "legacy.precision",
                    number(&config["precision"]) as usize,
                    profile["textFormat"]["fractionDigits"].as_u64().unwrap() as usize,
                );
                check.over(
                    "legacy.range",
                    number(&config["min"])
                        .abs()
                        .max(number(&config["max"]).abs())
                        .ceil() as usize,
                    1_000_000_000,
                );
            }
            if widget["kind"] == "image" {
                if !string(&config["assetId"]).is_empty() {
                    image(&mut check, string(&config["assetId"]), pw, ph);
                } else {
                    check.add(
                        "image.assetId",
                        json!("selected asset"),
                        json!("unavailable"),
                    );
                }
            }
        }
        // Keep the firmware's single-pointer overlap rule, using local hit
        // areas after its safe-inset transform. Decorative components may overlap.
        hit = [
            artwork[0] + hit[0] * artwork[2],
            artwork[1] + hit[1] * artwork[3],
            hit[2] * artwork[2],
            hit[3] * artwork[3],
        ];
        let usable_x = 1.0 - number(&inset["left"]) - number(&inset["right"]);
        let usable_y = 1.0 - number(&inset["top"]) - number(&inset["bottom"]);
        let bounds = [
            number(&inset["left"])
                + number(&widget["bounds"]["x"]) * usable_x
                + number(&widget["bounds"]["width"]) * usable_x * hit[0],
            number(&inset["top"])
                + number(&widget["bounds"]["y"]) * usable_y
                + number(&widget["bounds"]["height"]) * usable_y * hit[1],
            number(&widget["bounds"]["width"]) * usable_x * hit[2],
            number(&widget["bounds"]["height"]) * usable_y * hit[3],
        ];
        if let Some((other, _, _)) = touch_areas.iter().find(|(other, a, other_interactive)| {
            let portable = widget["kind"] == "component" || other["kind"] == "component";
            let constrained = if portable {
                interactive && *other_interactive
            } else {
                widget["kind"] != "image"
                    && other["kind"] != "image"
                    && (widget["kind"] == "button" || other["kind"] == "button")
            };
            widget["pageId"] == other["pageId"]
                && constrained
                && bounds[0] < a[0] + a[2]
                && a[0] < bounds[0] + bounds[2]
                && bounds[1] < a[1] + a[3]
                && a[1] < bounds[1] + bounds[3]
        }) {
            check.add(
                "hitArea.overlap",
                json!("non-overlapping touch targets"),
                other["id"].clone(),
            );
        }
        touch_areas.push((widget, bounds, interactive));
    }
    Ok(check.issues)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_target_diagnostics_match_shared_typescript_oracle_without_source_changes() {
        let fixtures: Value = serde_json::from_str(include_str!(
            "../../../../apps/companion-esp32/test/widget-compatibility.json"
        ))
        .unwrap();
        let profiles: Value = serde_json::from_str(include_str!(
            "../../../../apps/companion-esp32/test/render-profiles.json"
        ))
        .unwrap();
        for item in items(&fixtures["cases"]) {
            let document: Value = serde_json::from_str(string(&item["canonicalDocument"])).unwrap();
            let before = document.to_string();
            let result = check_document(
                &document,
                &profiles["valid"][item["profile"].as_u64().unwrap() as usize],
            )
            .unwrap();
            assert_eq!(
                serde_json::to_value(result).unwrap(),
                item["issues"],
                "{}",
                item["name"]
            );
            assert_eq!(document.to_string(), before);
        }
    }
}
