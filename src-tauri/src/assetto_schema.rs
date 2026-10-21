//! Factual native field layouts, separate from canonical cross-game quantities.
//! The data tables transcribe SDK declarations, not upstream reader code.
use super::{Channel, Context, Game, RawSample};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Clone, Deserialize)]
struct Field {
    group: String,
    field: String,
    offset: usize,
    kind: String,
    #[serde(default)]
    shape: Vec<usize>,
    #[serde(default)]
    count: usize,
    unit: String,
    supported: bool,
    source: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    axes: Vec<String>,
}

#[derive(Clone)]
struct Scalar {
    field: Field,
    id: String,
    offset: usize,
    indices: Vec<usize>,
    supported: bool,
}

fn width(kind: &str) -> usize {
    match kind {
        "i8" | "u8" | "bool8" | "utf8" => 1,
        "i16" | "u16" | "utf16" => 2,
        "u64text" => 8,
        "i32" | "u32" | "bool32" | "f32" => 4,
        _ => panic!("Invalid authored Assetto field type"),
    }
}

fn expand(fields: &[Field], game: Game) -> Vec<Scalar> {
    let mut result = vec![];
    for field in fields {
        let elements = field.shape.iter().product::<usize>().max(1);
        for element in 0..elements {
            let mut remaining = element;
            let mut indices = vec![0; field.shape.len()];
            for (index, length) in field.shape.iter().enumerate().rev() {
                indices[index] = remaining % length;
                remaining /= length;
            }
            let suffix = indices.iter().map(|index| format!(".{index}")).collect::<String>();
            // Rally's expanded ACC-like declaration is not a verified Rally ABI.
            // Only fields independently established by its existing reader qualify.
            let rally_known = match field.group.as_str() {
                "physics" => matches!(field.field.as_str(), "packetId" | "gear" | "rpms" | "speedKmh" | "currentMaxRpm"),
                "graphics" => matches!(field.field.as_str(), "packetId" | "status"),
                "static" => matches!(field.field.as_str(), "smVersion" | "acVersion" | "carModel" | "track" | "maxRpm"),
                _ => false,
            };
            let supported = (if game == Game::Rally { rally_known } else { field.supported })
                && !(game == Game::Corsa && field.field == "carDamage" && element == 4);
            result.push(Scalar {
                id: format!("{}.{}{}", field.group, field.field, suffix),
                offset: field.offset + element * width(&field.kind),
                field: field.clone(), indices, supported,
            });
        }
    }
    result
}

fn channels(game: Game) -> &'static [Scalar] {
    static LEGACY: OnceLock<BTreeMap<String, Vec<Field>>> = OnceLock::new();
    static EVO_FIELDS: OnceLock<Vec<Field>> = OnceLock::new();
    static AC: OnceLock<Vec<Scalar>> = OnceLock::new();
    static ACC: OnceLock<Vec<Scalar>> = OnceLock::new();
    static EVO: OnceLock<Vec<Scalar>> = OnceLock::new();
    static RALLY: OnceLock<Vec<Scalar>> = OnceLock::new();
    let legacy = || LEGACY.get_or_init(|| serde_json::from_str(include_str!("assetto_schema_legacy.json")).expect("Authored legacy Assetto schema"));
    match game {
        Game::Corsa => AC.get_or_init(|| expand(&legacy()[Game::Corsa.id()], game)),
        Game::Competizione => ACC.get_or_init(|| expand(&legacy()[Game::Competizione.id()], game)),
        Game::Evo => EVO.get_or_init(|| expand(EVO_FIELDS.get_or_init(|| serde_json::from_str(include_str!("assetto_schema_evo.json")).expect("Authored EVO schema")), game)),
        Game::Rally => RALLY.get_or_init(|| expand(&legacy()[Game::Competizione.id()], game)),
    }
}

fn kind_bounds(kind: &str) -> (&'static str, f64, f64) {
    match kind {
        "bool8" | "bool32" => ("boolean", 0.0, 1.0),
        "f32" => ("number", -(f32::MAX as f64), f32::MAX as f64),
        "u8" => ("integer", 0.0, u8::MAX as f64),
        "i8" => ("integer", i8::MIN as f64, i8::MAX as f64),
        "u16" => ("integer", 0.0, u16::MAX as f64),
        "i16" => ("integer", i16::MIN as f64, i16::MAX as f64),
        "u32" => ("integer", 0.0, u32::MAX as f64),
        "i32" => ("integer", i32::MIN as f64, i32::MAX as f64),
        _ => ("string", 0.0, 0.0),
    }
}

fn bounds(scalar: &Scalar, game: Game) -> (&'static str, f64, f64) {
    let (kind, minimum, maximum) = kind_bounds(&scalar.field.kind);
    let range = match scalar.field.field.as_str() {
        "rpms" | "rpm" => Some((0.0,100000.0)),
        "currentMaxRpm" | "maxRpm" => Some((1.0,100000.0)),
        "gear" | "gear_int" => Some((0.0,32.0)),
        "speedKmh" => Some((0.0,2000.0)),
        "gas" | "brake" | "clutch" | "gas_percent" | "brake_percent" | "clutch_percent" | "handbrake_percent" => Some((0.0,1.0)),
        "steering_percent" => Some((-1.0,1.0)),
        "steerAngle" if game==Game::Evo => Some((-1.0,1.0)),
        _ => None,
    };
    let (minimum,maximum)=range.unwrap_or((minimum,maximum));
    (kind,minimum,maximum)
}

fn label(field: &str, indices: &[usize]) -> String {
    let mut result=String::new();let mut previous_lower=false;
    for character in field.chars() {
        if character=='_' || character=='.' {result.push(' ');previous_lower=false;continue;}
        if character.is_uppercase() && previous_lower {result.push(' ');}
        result.push(character);previous_lower=character.is_lowercase();
    }
    for index in indices {result.push_str(&format!(" [{index}]"));}
    result
}

pub(super) fn catalogue(game: Game) -> Vec<Value> {
    channels(game).iter().map(|scalar| {
        let field = &scalar.field;
        let (kind, minimum, maximum) = bounds(scalar,game);
        json!({"id":scalar.id,"label":label(&field.field,&scalar.indices),"name":field.field,"type":kind,"unit":field.unit,
            "minimum":minimum,"maximum":maximum,"maxLength":field.count.max(20)*4,"derived":false,
            "group":field.group,"category":field.group,"nativeField":field.field,"nativeType":field.kind,
            "sourceGame":game.id(),"offset":scalar.offset,"indices":scalar.indices,"shape":field.shape,"axes":field.axes,
            "supported":scalar.supported,"qualification":if game==Game::Rally {if scalar.supported {"observed-prefix"}else{"unverified-layout"}}else{"documented"},
            "source":field.source,"note":field.note})
    }).collect()
}

pub(super) fn ids(game: Game) -> impl Iterator<Item = &'static str> {
    channels(game).iter().map(|scalar| scalar.id.as_str())
}

/// Exact documented prefixes, never speculative mapping allocation sizes.
pub(super) fn page_lengths(game: Game) -> (usize, usize, usize) {
    if game == Game::Rally { return game.page_lengths(); }
    let mut lengths = [0; 3];
    for scalar in channels(game) {
        let field = &scalar.field;
        let index = match field.group.as_str() { "physics" => 0, "graphics" => 1, _ => 2 };
        let length = if matches!(field.kind.as_str(), "utf8" | "utf16") { field.count * width(&field.kind) } else { width(&field.kind) };
        lengths[index] = lengths[index].max(scalar.offset + length);
    }
    // C structs round their complete size to their maximum (four-byte) alignment.
    for length in &mut lengths { *length = (*length + 3) / 4 * 4; }
    (lengths[0], lengths[1], lengths[2])
}

fn layout_known(game: Game, context: &Context) -> bool {
    match game {
        Game::Corsa => context.shared_memory_version == "1.7",
        Game::Competizione => matches!(context.shared_memory_version.as_str(), "1.8" | "1.9"),
        Game::Evo => context.shared_memory_version == "1.0",
        Game::Rally => true, // Per-field observed-prefix gate above remains mandatory.
    }
}

fn value(bytes: &[u8], scalar: &Scalar) -> Option<Value> {
    let field = &scalar.field;
    let length = if matches!(field.kind.as_str(), "utf8" | "utf16") { field.count * width(&field.kind) } else { width(&field.kind) };
    let raw = bytes.get(scalar.offset..scalar.offset.checked_add(length)?)?;
    Some(match field.kind.as_str() {
        "f32" => { let number = f32::from_le_bytes(raw.try_into().ok()?); if !number.is_finite() { return None; } json!(number as f64) },
        "i32" => json!(i32::from_le_bytes(raw.try_into().ok()?)),
        "u32" => json!(u32::from_le_bytes(raw.try_into().ok()?)),
        "i16" => json!(i16::from_le_bytes(raw.try_into().ok()?)),
        "u16" => json!(u16::from_le_bytes(raw.try_into().ok()?)),
        "i8" => json!(raw[0] as i8), "u8" => json!(raw[0]),
        "bool8" | "bool32" => { let number = if raw.len()==1 {raw[0] as u32}else{u32::from_le_bytes(raw.try_into().ok()?)}; if number>1 {return None;} json!(number==1) },
        "u64text" => json!(u64::from_le_bytes(raw.try_into().ok()?).to_string()),
        "utf8" | "utf16" => {
            let text = if field.kind=="utf8" {
                std::str::from_utf8(&raw[..raw.iter().position(|byte|*byte==0).unwrap_or(raw.len())]).ok()?.to_owned()
            } else {
                let words = raw.chunks_exact(2).map(|pair|u16::from_le_bytes([pair[0],pair[1]])).take_while(|word|*word!=0).collect::<Vec<_>>();
                String::from_utf16(&words).ok()?
            };
            if text.chars().any(char::is_control) {return None;}
            json!(text.trim_end())
        },
        _ => return None,
    })
}

pub(super) fn decode(sample: &RawSample, game: Game, context: &Context, result: &mut BTreeMap<String, Channel>) {
    let known = layout_known(game, context);
    for scalar in channels(game) {
        let page = match scalar.field.group.as_str() { "physics" => &sample.physics, "graphics" => &sample.graphics, _ => &sample.statics };
        let channel = if !scalar.supported {
            Channel::unknown(if game==Game::Rally {"Expanded Rally field has no verified layout; unavailable for processing."}else{"The game SDK marks this field unused or undocumented."})
        } else if !known {
            Channel::unknown("The shared-memory version is not qualified for this expanded layout.")
        } else {
            value(page, scalar).filter(|value| {
                let (_,minimum,maximum)=bounds(scalar,game);
                value.as_f64().is_none_or(|number|number>=minimum && number<=maximum)
            }).map(Channel::ready).unwrap_or_else(|| Channel::unknown("Field is absent, out of range, nonfinite, invalid boolean, or invalid text."))
        };
        result.insert(scalar.id.clone(), channel);
    }
}

#[cfg(test)]
#[path = "assetto_schema_tests.rs"]
mod tests;
