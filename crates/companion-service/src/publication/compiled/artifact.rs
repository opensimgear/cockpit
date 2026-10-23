//! Source-free raster artifact and media admission; all offsets and hashes are checked natively.
use super::*;
pub(super) const TILE_BYTES: u64 = 768;
pub(super) fn capabilities(c: &Value) -> Result<(), String> {
    object(
        c,
        &[
            "version",
            "id",
            "rasterFormats",
            "maxWidth",
            "maxHeight",
            "maxFrames",
            "maxAssetBytes",
            "maxTotalBytes",
            "maxResidentBytes",
            "motion",
            "audio",
            "audioFormats",
        ],
        &[],
    )?;
    ensure(c["version"] == 1, "Unsupported widget capability version.")?;
    stable(&c["id"])?;
    ensure(
        c["rasterFormats"] == json!(["rgb565le-a8"]),
        "Unsupported raster capability.",
    )?;
    integer(&c["maxWidth"], 1, 2048)?;
    integer(&c["maxHeight"], 1, 2048)?;
    integer(&c["maxFrames"], 1, 4096)?;
    integer(&c["maxAssetBytes"], 768, ASSET_BYTES as u64)?;
    integer(&c["maxTotalBytes"], 768, TOTAL_ASSET_BYTES as u64)?;
    integer(&c["maxResidentBytes"], 768, 256 * 1024 * 1024)?;
    ensure(
        c["motion"].is_boolean() && c["audio"].is_boolean(),
        "Invalid media capabilities.",
    )?;
    ensure(
        list(&c["audioFormats"], 0, 1)?
            .iter()
            .all(|v| v == "wav-pcm16"),
        "Unsupported audio capability.",
    )
}
pub(super) fn default_capabilities() -> Value {
    json!({"version":1,"id":"browser-raster-v1","rasterFormats":["rgb565le-a8"],"maxWidth":2048,"maxHeight":2048,"maxFrames":4096,"maxAssetBytes":2097152,"maxTotalBytes":8388608,"maxResidentBytes":67108864,"motion":true,"audio":true,"audioFormats":["wav-pcm16"]})
}
pub(super) fn descriptor(asset: &Value, caps: &Value) -> Result<(), String> {
    object(asset, &["id", "sha256", "sizeBytes", "mime"], &[])?;
    let name = stable(&asset["id"])?;
    let sha = digest(&asset["sha256"])?;
    integer(
        &asset["sizeBytes"],
        1,
        caps["maxAssetBytes"]
            .as_u64()
            .unwrap()
            .min(ASSET_BYTES as u64),
    )?;
    let prefix = match asset["mime"].as_str() {
        Some("application/vnd.opensimgear.raster-tiles") => "pixels-",
        Some("audio/wav") => "audio-",
        _ => return Err("Unsupported compiled asset MIME.".into()),
    };
    ensure(
        name == format!("{prefix}{}", &sha[..32]),
        "Compiled asset ID must identify its content.",
    )
}
pub(super) fn validate(
    a: &Value,
    bytes: &BTreeMap<String, Vec<u8>>,
    caps: &Value,
) -> Result<(), String> {
    capabilities(caps)?;
    object(
        a,
        &[
            "format",
            "compiler",
            "rasterizer",
            "id",
            "sourceHash",
            "parameterHash",
            "behaviorHash",
            "capabilityHash",
            "width",
            "height",
            "tileSize",
            "pixelFormat",
            "behavior",
            "inputs",
            "outputs",
            "hitRegions",
            "domains",
            "frames",
            "tiles",
            "assets",
            "motions",
            "sounds",
            "fallbacks",
            "resources",
        ],
        &[],
    )?;
    ensure(
        a["format"] == "osg-raster-widget-1"
            && a["compiler"] == "osg-widget-compiler-1"
            && a["rasterizer"] == "resvg-wasm-2.6.2",
        "Unsupported compiled format/compiler.",
    )?;
    stable(&a["id"])?;
    for key in [
        "sourceHash",
        "parameterHash",
        "behaviorHash",
        "capabilityHash",
    ] {
        digest(&a[key])?;
    }
    let width = integer(&a["width"], 1, caps["maxWidth"].as_u64().unwrap())?;
    let height = integer(&a["height"], 1, caps["maxHeight"].as_u64().unwrap())?;
    ensure(
        a["tileSize"] == 16 && a["pixelFormat"] == "rgb565le-a8",
        "Unsupported pixel encoding.",
    )?;
    behavior::validate(&a["behavior"])?;
    ensure(
        hash(&canonical(&a["behavior"])?) == a["behaviorHash"].as_str().unwrap(),
        "Compiled behavior digest mismatch.",
    )?;
    let signals = a["behavior"]["signals"].as_array().unwrap();
    for (field, input) in [("inputs", true), ("outputs", false)] {
        let mut ids = BTreeSet::new();
        for port in list(&a[field], 0, 16)? {
            object(
                port,
                &["id", "label", "type", "signal"],
                &["min", "max", "values"],
            )?;
            ensure(ids.insert(stable(&port["id"])?), "Duplicate compiled port.")?;
            let label = port["label"].as_str().ok_or("Invalid port label.")?;
            ensure(label.encode_utf16().count() <= 120, "Invalid port label.")?;
            let signal = signals
                .iter()
                .find(|s| s["id"] == port["signal"])
                .ok_or("Unknown port signal.")?;
            ensure(
                signal["type"] == port["type"],
                "Compiled port type mismatch.",
            )?;
            match signal["type"].as_str() {
                Some("number") => ensure(
                    port["min"] == signal["min"]
                        && port["max"] == signal["max"]
                        && port.get("values").is_none(),
                    "Compiled numeric port domain mismatch.",
                )?,
                Some("enum") => ensure(
                    port["values"] == signal["values"]
                        && port.get("min").is_none()
                        && port.get("max").is_none(),
                    "Compiled enum port domain mismatch.",
                )?,
                _ => ensure(
                    ["min", "max", "values"]
                        .iter()
                        .all(|key| port.get(key).is_none()),
                    "Unexpected compiled port domain.",
                )?,
            }
            if input {
                let output = a["behavior"]["outputs"]
                    .as_array()
                    .and_then(|outputs| outputs.iter().find(|o| o["id"] == port["id"]))
                    .ok_or("Compiled input requires a declared emit output.")?;
                ensure(
                    output["type"] == port["type"] && output["neutral"] == signal["initial"],
                    "Compiled emit output neutral mismatch.",
                )?;
                for key in ["min", "max", "values"] {
                    ensure(
                        output.get(key) == port.get(key),
                        "Compiled emit output domain mismatch.",
                    )?;
                }
            }
        }
    }
    let events = a["behavior"]["events"].as_array().unwrap();
    let mut hits = BTreeSet::new();
    for hit in list(&a["hitRegions"], 0, 32)? {
        object(hit, &["id", "x", "y", "width", "height", "events"], &[])?;
        ensure(hits.insert(stable(&hit["id"])?), "Duplicate hit region.")?;
        let x = integer(&hit["x"], 0, width)?;
        let y = integer(&hit["y"], 0, height)?;
        let w = integer(&hit["width"], 1, width)?;
        let h = integer(&hit["height"], 1, height)?;
        ensure(
            x + w <= width && y + h <= height,
            "Hit region outside surface.",
        )?;
        for event in object(
            &hit["events"],
            &[],
            &["press", "release", "activate", "cancel"],
        )?
        .values()
        {
            ensure(
                events.iter().any(|e| e["id"] == *event),
                "Unknown hit event.",
            )?;
        }
    }
    let domains = list(&a["domains"], 0, 16)?;
    let mut domain_signals = BTreeSet::new();
    for domain in domains {
        object(
            domain,
            &[
                "signal",
                "values",
                "numeric",
                "maxErrorPixels",
                "smoothingMs",
            ],
            &[],
        )?;
        let signal = signals
            .iter()
            .find(|s| s["id"] == domain["signal"])
            .ok_or("Unknown visual domain signal.")?;
        ensure(
            domain_signals.insert(stable(&domain["signal"])?),
            "Repeated visual domain.",
        )?;
        let values = list(&domain["values"], 1, 4096)?;
        let mut seen = BTreeSet::new();
        let mut last = None;
        for value in values {
            ensure(
                behavior::accepts(signal, value) && seen.insert(canonical(value)?),
                "Invalid visual domain sample.",
            )?;
            if signal["type"] == "number" {
                let n = value.as_f64().unwrap();
                ensure(
                    last.is_none_or(|last| n > last),
                    "Numeric samples must increase.",
                )?;
                last = Some(n);
            }
        }
        match signal["type"].as_str() {
            Some("boolean") => ensure(
                values.len() == 2
                    && values.contains(&json!(false))
                    && values.contains(&json!(true)),
                "Boolean visual domain must cover false and true.",
            )?,
            Some("enum") => ensure(
                values.len() == signal["values"].as_array().unwrap().len()
                    && signal["values"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|value| values.contains(value)),
                "Enum visual domain must cover every value.",
            )?,
            Some("number") => ensure(
                values.len() >= 2
                    && values.first() == Some(&signal["min"])
                    && values.last() == Some(&signal["max"]),
                "Numeric visual domain must include its complete endpoints.",
            )?,
            _ => return Err("Unsupported visual domain.".into()),
        }
        ensure(
            domain["numeric"] == json!(signal["type"] == "number"),
            "Visual numeric domain mismatch.",
        )?;
        number(&domain["maxErrorPixels"], 0., f64::MAX)?;
        let smoothing=integer(&domain["smoothingMs"], 0, 60000)?;
        ensure(smoothing==0||caps["motion"]==true,"Numeric smoothing is not supported by this target.")?;
    }
    let assets = list(&a["assets"], 0, 64)?;
    let mut asset_ids = BTreeSet::new();
    let mut total = 0;
    let mut dictionary = 0;
    let mut media = 0;
    for asset in assets {
        descriptor(asset, caps)?;
        let id = asset["id"].as_str().unwrap();
        ensure(asset_ids.insert(id), "Duplicate compiled asset.")?;
        let data = bytes.get(id).ok_or("Incomplete compiled asset closure.")?;
        let size = asset["sizeBytes"].as_u64().unwrap();
        ensure(
            data.len() as u64 == size && hash(data) == asset["sha256"].as_str().unwrap(),
            "Compiled asset digest/size mismatch.",
        )?;
        total += size;
        if asset["mime"] == "audio/wav" {
            media += size;
            wave(data)?;
        } else {
            dictionary += size;
        }
    }
    ensure(
        total <= caps["maxTotalBytes"].as_u64().unwrap(),
        "Compiled assets exceed target byte budget.",
    )?;
    let mut used_assets = BTreeSet::new();
    let mut tile_offsets: BTreeMap<&str, BTreeSet<u64>> = BTreeMap::new();
    let tiles = list(&a["tiles"], 1, 32768)?;
    for tile in tiles {
        object(tile, &["assetId", "offset", "length", "sha256"], &[])?;
        digest(&tile["sha256"])?;
        let offset = integer(&tile["offset"], 0, ASSET_BYTES as u64)?;
        ensure(
            tile["length"] == 768 && offset % 768 == 0,
            "Invalid tile size/alignment.",
        )?;
        let id = tile["assetId"].as_str().ok_or("Invalid tile asset ID.")?;
        let asset = assets
            .iter()
            .find(|asset| {
                asset["id"] == id && asset["mime"] == "application/vnd.opensimgear.raster-tiles"
            })
            .ok_or("Unknown pixel dictionary.")?;
        ensure(
            offset + 768 <= asset["sizeBytes"].as_u64().unwrap(),
            "Tile outside pixel dictionary.",
        )?;
        used_assets.insert(id);
        ensure(
            hash(&bytes[id][offset as usize..offset as usize + 768])
                == tile["sha256"].as_str().unwrap(),
            "Pixel tile digest mismatch.",
        )?;
    }
    for tile in tiles {
        let id = tile["assetId"].as_str().unwrap();
        ensure(
            tile_offsets
                .entry(id)
                .or_default()
                .insert(tile["offset"].as_u64().unwrap()),
            "Repeated pixel dictionary offset.",
        )?;
    }
    for asset in assets
        .iter()
        .filter(|asset| asset["mime"] == "application/vnd.opensimgear.raster-tiles")
    {
        let size = asset["sizeBytes"].as_u64().unwrap();
        ensure(
            size % 768 == 0
                && tile_offsets
                    .get(asset["id"].as_str().unwrap())
                    .is_some_and(|offsets| offsets.len() as u64 == size / 768),
            "Pixel dictionary contains unreferenced bytes.",
        )?;
    }
    let cells = width.div_ceil(16) * height.div_ceil(16);
    let frames = list(
        &a["frames"],
        1,
        caps["maxFrames"].as_u64().unwrap() as usize,
    )?;
    let mut frame_ids = BTreeSet::new();
    let mut selectors = BTreeSet::new();
    let mut selector_count = 0;
    let mut used_tiles = BTreeSet::new();
    for frame in frames {
        object(frame, &["id", "tiles"], &["selector"])?;
        ensure(
            frame_ids.insert(stable(&frame["id"])?),
            "Duplicate compiled frame.",
        )?;
        for index in list(&frame["tiles"], cells as usize, cells as usize)? {
            used_tiles.insert(integer(index, 0, tiles.len() as u64 - 1)?);
        }
        if let Some(selector) = frame.get("selector") {
            object(selector, &["values", "regions"], &[])?;
            object(
                &selector["values"],
                &domains
                    .iter()
                    .map(|d| d["signal"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                &[],
            )?;
            for domain in domains {
                ensure(
                    domain["values"]
                        .as_array()
                        .unwrap()
                        .contains(&selector["values"][domain["signal"].as_str().unwrap()]),
                    "Frame selector outside visual domain.",
                )?;
            }
            let regions = a["behavior"]["regions"].as_array().unwrap();
            object(
                &selector["regions"],
                &regions
                    .iter()
                    .map(|r| r["id"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                &[],
            )?;
            for region in regions {
                ensure(
                    region["states"]
                        .as_array()
                        .unwrap()
                        .contains(&selector["regions"][region["id"].as_str().unwrap()]),
                    "Frame selector outside region.",
                )?;
            }
            selector_count += 1;
            ensure(
                selectors.insert(canonical(selector)?),
                "Duplicate static presentation selector.",
            )?;
        }
    }
    ensure(
        used_tiles.len() == tiles.len(),
        "Compiled tile is not referenced by a frame.",
    )?;
    let expected = domains
        .iter()
        .map(|d| d["values"].as_array().unwrap().len())
        .chain(
            a["behavior"]["regions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["states"].as_array().unwrap().len()),
        )
        .try_fold(1usize, |total, n| {
            total
                .checked_mul(n)
                .filter(|n| *n <= 4096)
                .ok_or("Static selector product exceeds budget.")
        })?;
    ensure(
        expected == selector_count,
        "Static presentation selector coverage incomplete.",
    )?;
    let mut motion_ids = BTreeSet::new();
    for motion in list(&a["motions"], 0, 128)? {
        object(
            motion,
            &[
                "id",
                "fromFrameId",
                "toFrameId",
                "frames",
                "durationMs",
                "interruption",
            ],
            &[],
        )?;
        ensure(
            motion_ids.insert(stable(&motion["id"])?),
            "Duplicate motion ID.",
        )?;
        ensure(
            caps["motion"] == true
                && motion["fromFrameId"]
                    .as_str()
                    .is_some_and(|v| frame_ids.contains(v))
                && motion["toFrameId"]
                    .as_str()
                    .is_some_and(|v| frame_ids.contains(v)),
            "Unsupported or invalid motion.",
        )?;
        integer(&motion["durationMs"], 1, 60000)?;
        let sequence = list(&motion["frames"], 2, 600)?;
        ensure(
            sequence
                .iter()
                .all(|id| id.as_str().is_some_and(|id| frame_ids.contains(id)))
                && sequence.first() == Some(&motion["fromFrameId"])
                && sequence.last() == Some(&motion["toFrameId"])
                && matches!(
                    motion["interruption"].as_str(),
                    Some("reverse" | "restart" | "snap")
                ),
            "Invalid motion sequence.",
        )?;
    }
    let mut sound_ids = BTreeSet::new();
    for sound in list(&a["sounds"], 0, 32)? {
        object(
            sound,
            &["id", "cue", "assetId", "cooldownMs", "stopOnExit", "volume"],
            &[],
        )?;
        ensure(
            sound_ids.insert(stable(&sound["id"])?),
            "Duplicate sound ID.",
        )?;
        stable(&sound["cue"])?;
        let id = sound["assetId"].as_str().ok_or("Invalid sound asset ID.")?;
        ensure(
            caps["audio"] == true
                && caps["audioFormats"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("wav-pcm16"))
                && assets
                    .iter()
                    .any(|a| a["id"] == id && a["mime"] == "audio/wav"),
            "Unsupported or invalid compiled audio.",
        )?;
        used_assets.insert(id);
        integer(&sound["cooldownMs"], 0, 60000)?;
        ensure(
            sound["stopOnExit"].is_boolean(),
            "Invalid sound exit policy.",
        )?;
        number(&sound["volume"], 0., 1.)?;
    }
    ensure(
        used_assets == asset_ids,
        "Unused compiled asset descriptor.",
    )?;
    for fallback in list(&a["fallbacks"], 0, 2)? {
        object(fallback, &["feature", "reason"], &[])?;
        ensure(
            matches!(fallback["feature"].as_str(), Some("motion" | "audio"))
                && fallback["reason"]
                    .as_str()
                    .is_some_and(|s| s.encode_utf16().count() <= 240),
            "Invalid fallback report.",
        )?;
    }
    let resources = object(
        &a["resources"],
        &[
            "frameCount",
            "uniqueTiles",
            "dictionaryBytes",
            "frameMapBytes",
            "mediaBytes",
            "transferBytes",
            "manifestBytes",
            "residentBytes",
            "decodeScratchBytes",
            "audioWorkingBytes",
            "activeCandidateBytes",
            "worstChangedPixels",
            "rawFrameBytes",
        ],
        &[],
    )?;
    for v in resources.values() {
        integer(v, 0, 1024 * 1024 * 1024)?;
    }
    let manifest = canonical(a)?.len() as u64;
    let maps = frames.len() as u64 * cells * 4;
    let audio_working = assets.iter()
        .filter(|asset| asset["mime"] == "audio/wav")
        .map(|asset| asset["sizeBytes"].as_u64().unwrap())
        .max().unwrap_or(0) * 16;
    let resident = total * 3 + dictionary + manifest * 4 + maps * 2
        + width * height * 8 + TILE_BYTES + audio_working;
    for (key, value) in [
        ("frameCount", frames.len() as u64),
        ("uniqueTiles", tiles.len() as u64),
        ("dictionaryBytes", dictionary),
        ("mediaBytes", media),
        ("transferBytes", total),
        ("frameMapBytes", maps),
        ("decodeScratchBytes", TILE_BYTES),
        ("audioWorkingBytes", audio_working),
        ("rawFrameBytes", frames.len() as u64 * width * height * 3),
        ("activeCandidateBytes", resident * 2),
        ("worstChangedPixels", width * height),
        ("residentBytes", resident),
        ("manifestBytes", manifest),
    ] {
        ensure(
            a["resources"][key] == json!(value),
            "Incorrect compiled resource report.",
        )?;
    }
    ensure(
        manifest <= DOCUMENT_BYTES as u64
            && resident * 2 <= caps["maxResidentBytes"].as_u64().unwrap(),
        "Compiled widget exceeds resident/manifest budget.",
    )?;
    Ok(())
}
pub(super) fn wave(bytes: &[u8]) -> Result<(), String> {
    ensure(
        (44..=ASSET_BYTES).contains(&bytes.len()),
        "Invalid WAV size.",
    )?;
    let u16le = |offset: usize| u16::from_le_bytes([bytes[offset], bytes[offset + 1]]) as u64;
    let u32le = |offset: usize| {
        u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize
    };
    ensure(
        &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE" && u32le(4) + 8 == bytes.len(),
        "Invalid WAV container.",
    )?;
    let mut cursor = 12;
    let mut channels = 0;
    let mut rate = 0;
    let mut data = 0;
    while cursor + 8 <= bytes.len() {
        let length = u32le(cursor + 4);
        let start = cursor + 8;
        ensure(start + length <= bytes.len(), "Truncated WAV chunk.")?;
        if &bytes[cursor..cursor + 4] == b"fmt " {
            ensure(
                channels == 0 && length >= 16 && u16le(start) == 1 && u16le(start + 14) == 16,
                "Only PCM16 WAV is supported.",
            )?;
            channels = u16le(start + 2);
            rate = u32le(start + 4) as u64;
            ensure(
                (channels == 1 || channels == 2)
                    && (8000..=48000).contains(&rate)
                    && u16le(start + 12) == channels * 2
                    && u32le(start + 8) as u64 == rate * channels * 2,
                "Invalid WAV sample format.",
            )?;
        } else if &bytes[cursor..cursor + 4] == b"data" {
            ensure(data == 0 && length > 0, "Invalid WAV data.")?;
            data = length as u64;
        }
        cursor = start + length + length % 2;
    }
    ensure(
        cursor == bytes.len()
            && channels > 0
            && data > 0
            && data % (channels * 2) == 0
            && data <= rate * channels * 2 * 10,
        "Incomplete or oversized WAV.",
    )
}
