//! Native admission for editable generic widget sources. These never deploy to a player.
use super::*;

fn string(value: &Value, max: usize) -> Result<&str, String> {
    let s = value.as_str().ok_or("Expected widget source text.")?;
    ensure(
        s.encode_utf16().count() <= max && !s.chars().any(|c| c <= '\u{1f}'),
        "Invalid widget source text.",
    )?;
    Ok(s)
}
fn color(value: &Value) -> Result<(), String> {
    let value = string(value, 9)?;
    ensure(
        (value.len() == 7 || value.len() == 9)
            && value.starts_with('#')
            && value[1..].bytes().all(|c| c.is_ascii_hexdigit()),
        "Invalid source color.",
    )
}
fn optional_number(
    value: &Value,
    key: &str,
    min: f64,
    max: f64,
    whole: bool,
) -> Result<(), String> {
    if let Some(v) = value.get(key) {
        let n = number(v, min, max)?;
        ensure(!whole || n.fract() == 0., "Expected source integer.")?;
    }
    Ok(())
}
fn accepts(signal: &Value, value: &Value) -> bool {
    match signal["type"].as_str() {
        Some("boolean") => value.is_boolean(),
        Some("number") => value.as_f64().is_some_and(|v| {
            v.is_finite()
                && signal["min"].as_f64().is_some_and(|min| v >= min)
                && signal["max"].as_f64().is_some_and(|max| v <= max)
        }),
        Some("enum") => signal["values"]
            .as_array()
            .is_some_and(|values| values.contains(value)),
        _ => false,
    }
}

fn expression(
    value: &Value,
    parameters: &BTreeSet<String>,
    signals: &BTreeSet<String>,
    regions: &BTreeSet<String>,
    depth: usize,
    nodes: &mut usize,
) -> Result<(), String> {
    if depth == 0 {
        if value.is_boolean() {
            return Ok(());
        }
        if value.is_number() {
            number(value, -1e9, 1e9)?;
            return Ok(());
        }
        if value.is_string() {
            string(value, 4096)?;
            return Ok(());
        }
    }
    *nodes += 1;
    ensure(
        depth < 8 && *nodes <= 32,
        "Visual expression exceeds work limits.",
    )?;
    let child = |v: &Value, nodes: &mut usize| {
        expression(v, parameters, signals, regions, depth + 1, nodes)
    };
    if let Some(v) = value.get("literal") {
        object(value, &["literal"], &[])?;
        ensure(
            v.is_boolean() || v.is_number() || v.is_string(),
            "Invalid visual literal.",
        )?;
        if v.is_number() {
            number(v, -1e9, 1e9)?;
        } else if v.is_string() {
            string(v, 120)?;
        }
        return Ok(());
    }
    if let Some(v) = value.get("ref") {
        object(value, &["ref"], &[])?;
        let id = stable(v)?;
        return ensure(
            parameters.contains(id) || signals.contains(id),
            "Unknown visual reference.",
        );
    }
    if let Some(v) = value.get("state") {
        object(value, &["state"], &[])?;
        return ensure(regions.contains(stable(v)?), "Unknown visual region.");
    }
    match value["op"].as_str() {
        Some("not") => {
            object(value, &["op", "value"], &[])?;
            child(&value["value"], nodes)?;
        }
        Some("and" | "or") => {
            object(value, &["op", "values"], &[])?;
            for v in list(&value["values"], 1, 16)? {
                child(v, nodes)?;
            }
        }
        Some("eq" | "ne" | "lt" | "lte" | "gt" | "gte" | "add" | "sub" | "mul" | "div") => {
            object(value, &["op", "left", "right"], &[])?;
            child(&value["left"], nodes)?;
            child(&value["right"], nodes)?;
        }
        Some("clamp") => {
            object(value, &["op", "value", "min", "max"], &[])?;
            for key in ["value", "min", "max"] {
                child(&value[key], nodes)?;
            }
        }
        Some("if") => {
            object(value, &["op", "condition", "then", "else"], &[])?;
            for key in ["condition", "then", "else"] {
                child(&value[key], nodes)?;
            }
        }
        _ => return Err("Unsupported visual expression.".into()),
    }
    Ok(())
}
struct Scene<'a> {
    parameters: &'a BTreeSet<String>,
    signals: &'a BTreeSet<String>,
    regions: &'a BTreeSet<String>,
    assets: &'a [Value],
    ids: BTreeSet<String>,
    count: usize,
}
impl Scene<'_> {
    fn nodes(&mut self, value: &Value, depth: usize) -> Result<(), String> {
        ensure(depth <= 8, "Visual group depth exceeded.")?;
        for node in list(value, 0, 128)? {
            self.count += 1;
            ensure(self.count <= 128, "Too many source scene nodes.")?;
            let id = stable(&node["id"])?;
            ensure(self.ids.insert(id.into()), "Duplicate scene node ID.")?;
            if node.get("label").is_some() {
                string(&node["label"], 120)?;
            }
            let geometry: &[&str] = match node["kind"].as_str() {
                Some("group") => &["children"],
                Some("rectangle" | "ellipse") => &["x", "y", "width", "height"],
                Some("line") => &["x", "y", "x2", "y2"],
                Some("path") => &["d"],
                Some("text") => &["x", "y", "text", "font", "fontSize"],
                Some("image") => &["x", "y", "width", "height", "assetId"],
                _ => return Err("Unsupported source scene primitive.".into()),
            };
            let mut required = vec!["id", "kind"];
            required.extend(geometry);
            let mut optional = vec![
                "label",
                "visible",
                "opacity",
                "fill",
                "stroke",
                "strokeWidth",
                "transform",
            ];
            if node["kind"] == "rectangle" {
                optional.push("radius");
            }
            if node["kind"] == "text" {
                optional.push("align");
            }
            let map = object(node, &required, &optional)?;
            for (key, value) in map {
                if ![
                    "id",
                    "label",
                    "kind",
                    "children",
                    "transform",
                    "assetId",
                    "font",
                    "align",
                ]
                .contains(&key.as_str())
                {
                    expression(
                        value,
                        self.parameters,
                        self.signals,
                        self.regions,
                        0,
                        &mut 0,
                    )?;
                }
            }
            if let Some(transform) = node.get("transform") {
                for value in object(
                    transform,
                    &[],
                    &[
                        "translateX",
                        "translateY",
                        "rotate",
                        "scaleX",
                        "scaleY",
                        "originX",
                        "originY",
                    ],
                )?
                .values()
                {
                    expression(
                        value,
                        self.parameters,
                        self.signals,
                        self.regions,
                        0,
                        &mut 0,
                    )?;
                }
            }
            if node["kind"] == "text" {
                ensure(
                    matches!(node["font"].as_str(), Some("sans" | "mono")),
                    "Unsupported source font.",
                )?;
                if let Some(align) = node.get("align") {
                    ensure(
                        matches!(align.as_str(), Some("left" | "center" | "right")),
                        "Invalid source text alignment.",
                    )?;
                }
            }
            if node["kind"] == "image" {
                ensure(
                    self.assets.iter().any(|asset| {
                        asset["id"] == node["assetId"]
                            && asset["mime"]
                                .as_str()
                                .is_some_and(|mime| mime.starts_with("image/"))
                    }),
                    "Missing source image asset.",
                )?;
            }
            if node["kind"] == "group" {
                self.nodes(&node["children"], depth + 1)?;
            }
        }
        Ok(())
    }
}

fn assets(value: &Value) -> Result<&[Value], String> {
    let Some(value) = value.get("assets") else {
        return Ok(&[]);
    };
    let rows = list(value, 0, 32)?;
    let mut ids = BTreeSet::new();
    let mut total = 0;
    for asset in rows {
        object(asset, &["id", "mime", "base64"], &[])?;
        ensure(ids.insert(stable(&asset["id"])?), "Duplicate source asset.")?;
        let encoded = asset["base64"]
            .as_str()
            .ok_or("Missing source asset bytes.")?;
        ensure(
            encoded.len() <= ASSET_BYTES.div_ceil(3) * 4,
            "Source asset exceeds budget.",
        )?;
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| "Invalid source asset base64.")?;
        ensure(
            !bytes.is_empty() && bytes.len() <= ASSET_BYTES && STANDARD.encode(&bytes) == encoded,
            "Noncanonical source asset base64.",
        )?;
        total += bytes.len();
        ensure(
            total <= 8 * 1024 * 1024,
            "Source asset union exceeds budget.",
        )?;
        if asset["mime"] == "audio/wav" {
            compiled::validate_wave(&bytes)?;
        } else {
            let mime = asset["mime"].as_str().ok_or("Missing source image MIME.")?;
            ensure(
                matches!(mime, "image/png" | "image/jpeg" | "image/webp"),
                "Unsupported source asset MIME.",
            )?;
            let pixels = validate_raster(&bytes, mime)?;
            ensure(
                pixels <= 4 * 1024 * 1024,
                "Source image decoded budget exceeded.",
            )?;
            let format = match mime {
                "image/png" => image::ImageFormat::Png,
                "image/jpeg" => image::ImageFormat::Jpeg,
                _ => image::ImageFormat::WebP,
            };
            let mut reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(4096);
            limits.max_image_height = Some(4096);
            limits.max_alloc = Some(32 * 1024 * 1024);
            reader.limits(limits);
            reader
                .decode()
                .map_err(|_| "Source raster decoding failed.")?;
        }
    }
    Ok(rows)
}

pub(super) fn source(source: &Value) -> Result<(), String> {
    ensure(
        serde_json::to_vec(source).map_err(|e| e.to_string())?.len() <= 1024 * 1024,
        "Widget source exceeds byte budget.",
    )?;
    object(
        source,
        &[
            "formatVersion",
            "id",
            "revision",
            "title",
            "size",
            "parameters",
            "behavior",
            "visualDomains",
            "inputs",
            "outputs",
            "hitRegions",
            "scene",
        ],
        &["motions", "sounds", "assets"],
    )?;
    ensure(
        source["formatVersion"] == 2,
        "Unsupported widget source format; recreate legacy widgets.",
    )?;
    stable(&source["id"])?;
    integer(&source["revision"], 1, 65535)?;
    text(&source["title"])?;
    object(&source["size"], &["width", "height"], &[])?;
    let width = integer(&source["size"]["width"], 16, 2048)? as f64;
    let height = integer(&source["size"]["height"], 16, 2048)? as f64;
    compiled::validate_behavior(&source["behavior"])?;
    let behavior = &source["behavior"];
    let signals = behavior["signals"].as_array().unwrap();
    let signal_ids = signals
        .iter()
        .map(|signal| signal["id"].as_str().unwrap().to_owned())
        .collect::<BTreeSet<_>>();
    let region_rows = behavior["regions"].as_array().unwrap();
    let regions = region_rows
        .iter()
        .map(|region| region["id"].as_str().unwrap().to_owned())
        .collect::<BTreeSet<_>>();
    let mut parameters = BTreeSet::new();
    for parameter in list(&source["parameters"], 0, 32)? {
        object(
            parameter,
            &["id", "label", "type", "default"],
            &["min", "max"],
        )?;
        let id = stable(&parameter["id"])?;
        ensure(
            parameters.insert(id.into()) && !signal_ids.contains(id),
            "Duplicate source parameter/signal.",
        )?;
        string(&parameter["label"], 120)?;
        match parameter["type"].as_str() {
            Some("number") => {
                let min = number(&parameter["min"], -1e9, 1e9)?;
                let max = number(&parameter["max"], -1e9, 1e9)?;
                ensure(min < max, "Invalid parameter range.")?;
                number(&parameter["default"], min, max)?;
            }
            Some("boolean") => ensure(
                parameter["default"].is_boolean(),
                "Invalid boolean parameter.",
            )?,
            Some("color") => color(&parameter["default"])?,
            Some("text") => {
                string(&parameter["default"], 120)?;
            }
            _ => return Err("Unsupported source parameter.".into()),
        }
        if parameter["type"] != "number" {
            ensure(
                parameter.get("min").is_none() && parameter.get("max").is_none(),
                "Unexpected parameter range.",
            )?;
        }
    }
    let mut domains = BTreeSet::new();
    for domain in list(&source["visualDomains"], 0, 16)? {
        object(
            domain,
            &["signal"],
            &[
                "values",
                "min",
                "max",
                "samples",
                "maxErrorPixels",
                "travelPixels",
                "smoothingMs",
            ],
        )?;
        let signal = signals
            .iter()
            .find(|signal| signal["id"] == domain["signal"])
            .ok_or("Unknown visual signal.")?;
        ensure(
            domains.insert(stable(&domain["signal"])?),
            "Duplicate visual domain.",
        )?;
        optional_number(domain, "smoothingMs", 0., 60000., true)?;
        if domain.get("smoothingMs").is_some() {
            ensure(
                signal["type"] == "number",
                "Only numeric visual domains smooth.",
            )?;
        }
        if let Some(values) = domain.get("values") {
            let values = list(values, 1, 4096)?;
            let mut unique = BTreeSet::new();
            for value in values {
                ensure(
                    accepts(signal, value) && unique.insert(value.to_string()),
                    "Invalid finite visual value.",
                )?;
            }
            ensure(
                ["min", "max", "samples"]
                    .iter()
                    .all(|key| domain.get(key).is_none()),
                "Ambiguous visual domain.",
            )?;
            match signal["type"].as_str() {
                Some("boolean") => ensure(
                    values.len() == 2,
                    "Boolean visual domain must cover both values.",
                )?,
                Some("enum") => ensure(
                    values.len() == signal["values"].as_array().unwrap().len(),
                    "Enum visual domain must cover every value.",
                )?,
                _ => {
                    ensure(
                        values.contains(&signal["min"]) && values.contains(&signal["max"]),
                        "Numeric samples must cover signal bounds.",
                    )?;
                    number(&domain["maxErrorPixels"], 0., 2048.)?;
                    number(&domain["travelPixels"], 0., 1e6)?;
                }
            }
        } else {
            ensure(
                signal["type"] == "number"
                    && domain["min"] == signal["min"]
                    && domain["max"] == signal["max"],
                "Numeric visual range must cover signal bounds.",
            )?;
            integer(&domain["samples"], 2, 4096)?;
            number(&domain["maxErrorPixels"], 0., 2048.)?;
            number(&domain["travelPixels"], 0., 1e6)?;
        }
    }
    for direction in ["inputs", "outputs"] {
        let mut ids = BTreeSet::new();
        for port in list(&source[direction], 0, 16)? {
            object(
                port,
                &["id", "label", "type", "signal"],
                &["min", "max", "values"],
            )?;
            let id = stable(&port["id"])?;
            ensure(ids.insert(id), "Duplicate source port.")?;
            string(&port["label"], 120)?;
            let signal = signals
                .iter()
                .find(|signal| signal["id"] == port["signal"])
                .ok_or("Unknown source port signal.")?;
            ensure(
                port["type"] == signal["type"],
                "Port type differs from signal.",
            )?;
            match signal["type"].as_str() {
                Some("number") => {
                    ensure(
                        port["min"] == signal["min"]
                            && port["max"] == signal["max"]
                            && port.get("values").is_none(),
                        "Source port numeric domain must match signal bounds.",
                    )?;
                }
                Some("enum") => ensure(
                    port["values"] == signal["values"]
                        && port.get("min").is_none()
                        && port.get("max").is_none(),
                    "Source port enum mismatch.",
                )?,
                _ => ensure(
                    ["min", "max", "values"]
                        .iter()
                        .all(|key| port.get(key).is_none()),
                    "Unexpected boolean source port domain.",
                )?,
            }
            if direction == "inputs" {
                let output = behavior["outputs"]
                    .as_array()
                    .and_then(|rows| rows.iter().find(|output| output["id"] == id))
                    .ok_or("Input needs a declared emit output.")?;
                ensure(
                    output["type"] == port["type"] && output["neutral"] == signal["initial"],
                    "Input emit type/neutral mismatch.",
                )?;
                for key in ["min", "max", "values"] {
                    ensure(
                        output.get(key) == signal.get(key),
                        "Input emit domain mismatch.",
                    )?;
                }
            }
        }
    }
    let events = behavior["events"].as_array().unwrap();
    let mut hit_ids = BTreeSet::new();
    for hit in list(&source["hitRegions"], 0, 32)? {
        object(hit, &["id", "x", "y", "width", "height", "events"], &[])?;
        ensure(hit_ids.insert(stable(&hit["id"])?), "Duplicate hit region.")?;
        let x = number(&hit["x"], 0., width)?;
        let y = number(&hit["y"], 0., height)?;
        let w = number(&hit["width"], 1., width)?;
        let h = number(&hit["height"], 1., height)?;
        ensure(
            x + w <= width && y + h <= height,
            "Hit region outside widget.",
        )?;
        for event in object(
            &hit["events"],
            &[],
            &["press", "release", "activate", "cancel"],
        )?
        .values()
        {
            ensure(
                events.iter().any(|entry| entry["id"] == *event),
                "Unknown hit event.",
            )?;
        }
    }
    let assets = assets(source)?;
    let mut scene = Scene {
        parameters: &parameters,
        signals: &signal_ids,
        regions: &regions,
        assets,
        ids: BTreeSet::new(),
        count: 0,
    };
    scene.nodes(&source["scene"], 0)?;
    ensure(scene.count > 0, "Widget needs source artwork.")?;
    let mut motions = BTreeSet::new();
    for motion in source
        .get("motions")
        .map(|v| list(v, 0, 32))
        .transpose()?
        .into_iter()
        .flatten()
    {
        object(
            motion,
            &["id", "from", "to", "durationMs", "fps", "tracks"],
            &["interruption", "fromRegions", "toRegions"],
        )?;
        ensure(
            motions.insert(stable(&motion["id"])?),
            "Duplicate source motion.",
        )?;
        let duration = integer(&motion["durationMs"], 1, 60000)?;
        let fps = integer(&motion["fps"], 1, 60)?;
        ensure(
            (duration * fps).div_ceil(1000) + 1 <= 600,
            "Too many baked motion frames.",
        )?;
        if let Some(policy) = motion.get("interruption") {
            ensure(
                matches!(policy.as_str(), Some("reverse" | "restart" | "snap")),
                "Invalid interruption policy.",
            )?;
        }
        for (key, region_key) in [("from", "fromRegions"), ("to", "toRegions")] {
            let selector = motion[key]
                .as_object()
                .ok_or("Expected source motion selector.")?;
            let region_selector = motion
                .get(region_key)
                .map(|value| value.as_object().ok_or("Expected source region selector."))
                .transpose()?;
            ensure(
                selector.len() + region_selector.map_or(0, |v| v.len()) > 0,
                "Motion selector must be explicit.",
            )?;
            for (id, value) in selector {
                let signal = signals
                    .iter()
                    .find(|s| s["id"] == id.as_str())
                    .ok_or("Unknown motion signal.")?;
                ensure(
                    accepts(signal, value),
                    "Motion value outside signal domain.",
                )?;
            }
            for (id, value) in region_selector.into_iter().flatten() {
                let region = region_rows
                    .iter()
                    .find(|r| r["id"] == id.as_str())
                    .ok_or("Unknown motion region.")?;
                ensure(
                    region["states"].as_array().unwrap().contains(value),
                    "Unknown motion region state.",
                )?;
            }
        }
        for track in list(&motion["tracks"], 0, 64)? {
            object(track, &["node", "property", "keyframes"], &["easing"])?;
            ensure(
                scene.ids.contains(stable(&track["node"])?),
                "Unknown motion node.",
            )?;
            let property = track["property"]
                .as_str()
                .ok_or("Expected baked property.")?;
            ensure(
                [
                    "x",
                    "y",
                    "x2",
                    "y2",
                    "width",
                    "height",
                    "radius",
                    "opacity",
                    "strokeWidth",
                    "fontSize",
                    "transform.translateX",
                    "transform.translateY",
                    "transform.rotate",
                    "transform.scaleX",
                    "transform.scaleY",
                    "transform.originX",
                    "transform.originY",
                ]
                .contains(&property),
                "Unsupported baked track property.",
            )?;
            if let Some(easing) = track.get("easing") {
                ensure(
                    matches!(
                        easing.as_str(),
                        Some("linear" | "ease-in" | "ease-out" | "ease-in-out")
                    ),
                    "Unsupported baked easing.",
                )?;
            }
            let keys = list(&track["keyframes"], 2, 32)?;
            let mut previous = -1.;
            for (index, key) in keys.iter().enumerate() {
                object(key, &["at", "value"], &[])?;
                let at = number(&key["at"], 0., 1.)?;
                number(&key["value"], -1e6, 1e6)?;
                ensure(
                    if index == 0 { at == 0. } else { at > previous },
                    "Invalid keyframe ordering.",
                )?;
                previous = at;
            }
            ensure(previous == 1., "Missing final keyframe.")?;
        }
    }
    let mut sounds = BTreeSet::new();
    for sound in source
        .get("sounds")
        .map(|v| list(v, 0, 32))
        .transpose()?
        .into_iter()
        .flatten()
    {
        object(
            sound,
            &["id", "cue", "assetId"],
            &["cooldownMs", "stopOnExit", "volume"],
        )?;
        ensure(
            sounds.insert(stable(&sound["id"])?),
            "Duplicate source sound.",
        )?;
        stable(&sound["cue"])?;
        ensure(
            assets
                .iter()
                .any(|asset| asset["id"] == sound["assetId"] && asset["mime"] == "audio/wav"),
            "Missing source sound asset.",
        )?;
        optional_number(sound, "cooldownMs", 0., 60000., true)?;
        optional_number(sound, "volume", 0., 1., false)?;
        if let Some(stop) = sound.get("stopOnExit") {
            ensure(stop.is_boolean(), "Invalid sound stop policy.")?;
        }
    }
    Ok(())
}
pub(super) fn bundle(bundle: &Value) -> Result<(), String> {
    ensure(
        serde_json::to_vec(bundle).map_err(|e| e.to_string())?.len() <= 8 * 1024 * 1024,
        "Generic source library exceeds budget.",
    )?;
    object(bundle, &["bundleVersion", "library"], &[])?;
    ensure(
        bundle["bundleVersion"] == 2,
        "Legacy widget imports are retired; create generic widget sources.",
    )?;
    object(&bundle["library"], &["formatVersion", "definitions"], &[])?;
    ensure(
        bundle["library"]["formatVersion"] == 2,
        "Unsupported generic widget library.",
    )?;
    let mut revisions = BTreeSet::new();
    for definition in list(&bundle["library"]["definitions"], 0, 256)? {
        source(definition)?;
        ensure(
            revisions.insert((
                definition["id"].as_str().unwrap(),
                definition["revision"].as_u64().unwrap(),
            )),
            "Duplicate source revision.",
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn seed() -> Value {
        serde_json::from_str(include_str!(
            "../../../../examples/companion/generic-toggle.source.json"
        ))
        .unwrap()
    }
    fn wrapped(source: Value) -> Value {
        json!({"bundleVersion":2,"library":{"formatVersion":2,"definitions":[source]}})
    }
    #[test]
    fn generated_toggle_source_and_library_pass_native_admission() {
        let source = seed();
        super::source(&source).unwrap();
        bundle(&wrapped(source)).unwrap();
    }
    #[test]
    fn legacy_imports_duplicate_revisions_and_untrusted_fields_fail() {
        assert!(bundle(&json!({"bundleVersion":1,"library":{"formatVersion":1,"definitions":[]},"manifest":[],"assets":[]})).is_err());
        let mut duplicate = wrapped(seed());
        duplicate["library"]["definitions"]
            .as_array_mut()
            .unwrap()
            .push(seed());
        assert!(bundle(&duplicate).is_err());
        for (path, value) in [
            ("/formatVersion", json!(1)),
            ("/revision", json!(0)),
            ("/scene/0/kind", json!("script")),
            ("/scene/0/text", json!({"ref":"missing"})),
            ("/scene/0/transform", json!({"exec":"x"})),
            ("/behavior/signals/0/initial", json!(1)),
            ("/inputs/0/id", json!("missing-emit")),
            ("/hitRegions/0/width", json!(99999)),
            ("/motions/0/tracks/0/property", json!("draw")),
            ("/motions/0/from/requested", json!(2)),
        ] {
            let mut source = seed();
            let (parent, key) = path.rsplit_once('/').unwrap();
            source
                .pointer_mut(parent)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert(key.into(), value);
            assert!(super::source(&source).is_err(), "accepted {path}");
        }
    }
    #[test]
    fn source_asset_and_finite_domain_admission_is_bounded() {
        let mut source = seed();
        source["visualDomains"] = json!([{"signal":"requested","values":[false]}]);
        assert!(super::source(&source).is_err());
        source["visualDomains"] = json!([{"signal":"requested","values":[false,true]}]);
        super::source(&source).unwrap();
        source["assets"] = json!([{"id":"bad","mime":"image/png","base64":"eA=="}]);
        assert!(super::source(&source).is_err());
        source["assets"] = json!([]);
        source["scene"][0]["text"] = json!({"op":"if","condition":{"literal":true},"then":{"literal":"A"},"else":{"literal":"B"}});
        super::source(&source).unwrap();
    }
    #[test]
    fn typescript_and_native_source_contracts_match_generated_corpus() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../../../packages/widget-compiler/test/fixtures/native-sources.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let result = super::source(&case["source"]);
            assert_eq!(
                result.is_ok(),
                case["valid"] == true,
                "source fixture {}: {:?}",
                case["name"],
                result
            );
        }
    }
}
