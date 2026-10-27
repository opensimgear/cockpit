use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::model::{
    normalized_title, valid_qid, valid_text, Entity, ExternalId, GameMetadata, LookupStatus,
    MatchMethod, MetadataLookup, MetadataQuery,
};

const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

/// Builds one bounded WDQS query for a provider batch. Local keys are never sent: `k0`, `k1`, …
/// are opaque positions which are mapped back to the caller's keys by `parse`.
pub(super) fn query(queries: &[MetadataQuery]) -> String {
    let mut ids = Vec::new();
    let mut titles = Vec::new();
    for (index, query) in queries.iter().enumerate() {
        if !query.valid() {
            continue;
        }
        let opaque = format!("k{index}");
        for external in &query.external_ids {
            if let Some((property, value)) = supported_id(&external.platform, &external.id) {
                ids.push(format!(
                    "({} wdt:{} {})",
                    sparql_string(&opaque), property, sparql_string(&value)
                ));
            }
        }
        for title in title_variants(&query.title) {
            for language in ["en", "mul"] {
                titles.push(format!(
                    "({} {}@{})",
                    sparql_string(&opaque), sparql_string(&title), language
                ));
            }
        }
    }

    let id_values = if ids.is_empty() {
        "VALUES (?key ?matchProp ?matchId) { }".to_owned()
    } else {
        format!("VALUES (?key ?matchProp ?matchId) {{ {} }}", ids.join(" "))
    };
    let title_values = if titles.is_empty() {
        "VALUES (?key ?needle) { }".to_owned()
    } else {
        format!("VALUES (?key ?needle) {{ {} }}", titles.join(" "))
    };
    let properties = supported_property_iris();

    // WDQS otherwise may expand the entire video-game class before applying our indexed matches.
    // Keep the VALUES-first join order and traverse each matched item's class path forwards.
    format!(
        r#"SELECT DISTINCT ?key ?item ?itemLabel ?description ?matchedBy ?matchLabel ?matchProp ?matchId ?field ?value ?valueLabel ?externalProp ?externalId WHERE {{
  hint:Query hint:optimizer "None".
  {{
    {id_values}
    ?item ?matchProp ?matchId .
    BIND("external-id" AS ?matchedBy)
    BIND("" AS ?matchLabel)
  }} UNION {{
    {title_values}
    ?item (rdfs:label|skos:altLabel) ?needle .
    BIND(?needle AS ?matchLabel)
    BIND("exact-title" AS ?matchedBy)
  }}
  ?item wdt:P31/wdt:P279* wd:Q7889 . hint:Prior hint:gearing "forward".
  OPTIONAL {{ ?item rdfs:label ?englishLabel . FILTER(LANG(?englishLabel) = "en") }}
  OPTIONAL {{ ?item rdfs:label ?neutralLabel . FILTER(LANG(?neutralLabel) = "mul") }}
  BIND(COALESCE(?englishLabel, ?neutralLabel) AS ?itemLabel)
  FILTER(BOUND(?itemLabel))
  OPTIONAL {{ ?item schema:description ?description . FILTER(LANG(?description) = "en") }}
  {{ ?item wdt:P136 ?value . BIND("genre" AS ?field) }} UNION {{
    ?item wdt:P136/wdt:P279+ ?value . FILTER NOT EXISTS {{ ?item wdt:P136 ?value }} BIND("genreAncestor" AS ?field)
  }} UNION {{ ?item wdt:P400 ?value . BIND("platform" AS ?field) }} UNION {{ ?item wdt:P178 ?value . BIND("developer" AS ?field) }} UNION {{ ?item wdt:P123 ?value . BIND("publisher" AS ?field) }} UNION {{ ?item wdt:P577 ?value . BIND("releaseDate" AS ?field) }} UNION {{
    ?item skos:altLabel ?value . FILTER(LANG(?value) = "en") BIND("alias" AS ?field)
  }} UNION {{
    ?item ?externalProp ?externalId . VALUES ?externalProp {{ {properties} }} BIND("externalId" AS ?field)
  }} UNION {{ BIND("identity" AS ?field) BIND("" AS ?value) }}
  SERVICE wikibase:label {{ bd:serviceParam wikibase:language "en,mul". }}
}}"#
    )
}

/// Parses WDQS's SPARQL JSON results. A complete, valid empty result is a negative lookup; transport
/// and response-shape failures remain errors so the caller can mark them unavailable.
pub(super) fn parse(
    queries: &[MetadataQuery],
    bytes: &[u8],
    fetched_at: u64,
) -> Result<BTreeMap<String, MetadataLookup>, String> {
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err("Wikidata response exceeds 2 MiB".into());
    }
    let expected: BTreeMap<String, &MetadataQuery> = queries
        .iter()
        .map(|query| (query.key.clone(), query))
        .collect();
    if expected.len() != queries.len() || queries.iter().any(|query| !query.valid()) {
        return Err("invalid or duplicate metadata query".into());
    }
    let opaque: BTreeMap<String, &MetadataQuery> = queries
        .iter()
        .enumerate()
        .map(|(index, query)| (format!("k{index}"), query))
        .collect();

    let document: Value = serde_json::from_slice(bytes).map_err(|_| "invalid Wikidata JSON".to_owned())?;
    let rows = document
        .get("results")
        .and_then(|value| value.get("bindings"))
        .and_then(Value::as_array)
        .ok_or_else(|| "Wikidata response is missing results.bindings".to_owned())?;

    let mut candidates: BTreeMap<String, BTreeMap<String, Candidate>> = BTreeMap::new();
    for row in rows {
        let key = binding_value(row, "key")?;
        let Some(query) = opaque.get(key) else {
            return Err("Wikidata response contains an unknown opaque key".into());
        };
        let qid = qid_from_binding(row, "item")?;
        let title = binding_value(row, "itemLabel")?;
        if !valid_text(title, 256) {
            return Err("Wikidata response contains an invalid title".into());
        }
        let matched_by = match binding_value(row, "matchedBy")? {
            "external-id" => {
                let property = property_from_binding(row, "matchProp")?;
                let identifier = binding_value(row, "matchId")?;
                let known = query.external_ids.iter().any(|external| {
                    supported_id(&external.platform, &external.id)
                        .is_some_and(|(expected_property, expected_id)| {
                            expected_property == property && expected_id == identifier
                        })
                });
                if !known {
                    return Err("Wikidata response contains an unrequested identifier match".into());
                }
                MatchMethod::ExternalId
            }
            "exact-title" => {
                let label = binding_value(row, "matchLabel")?;
                if !valid_text(label, 256)
                    || normalized_title(label) != normalized_title(&query.title)
                {
                    continue;
                }
                MatchMethod::ExactTitle
            }
            _ => return Err("Wikidata response contains an unknown match method".into()),
        };

        let groups = candidates.entry(query.key.clone()).or_default();
        let candidate = groups.entry(qid.to_owned()).or_insert_with(|| {
            Candidate::new(qid, title, binding_optional_value(row, "description"), fetched_at)
        });
        if candidate.metadata.title != title {
            return Err("Wikidata entity has inconsistent English labels".into());
        }
        match matched_by {
            MatchMethod::ExternalId => candidate.has_external_id = true,
            MatchMethod::ExactTitle => {},
        }
        candidate.add_field(row)?;
    }

    let mut lookups = BTreeMap::new();
    for query in queries {
        let matches = candidates.remove(&query.key).unwrap_or_default();
        let id_matches: Vec<_> = matches
            .values()
            .filter(|candidate| candidate.has_external_id)
            .collect();
        let considered = if id_matches.is_empty() {
            matches.values().collect::<Vec<_>>()
        } else {
            id_matches
        };
        let lookup = match considered.as_slice() {
            [] => MetadataLookup::empty(LookupStatus::NotFound, fetched_at),
            [candidate] => {
                let mut metadata = candidate.finish();
                metadata.matched_by = if candidate.has_external_id {
                    MatchMethod::ExternalId
                } else {
                    MatchMethod::ExactTitle
                };
                if !metadata.valid() {
                    return Err("Wikidata metadata failed validation".into());
                }
                MetadataLookup {
                    status: LookupStatus::Matched,
                    metadata: Some(metadata),
                    checked_at: fetched_at,
                    stale: false,
                }
            }
            _ => MetadataLookup::empty(LookupStatus::Ambiguous, fetched_at),
        };
        lookups.insert(query.key.clone(), lookup);
    }
    Ok(lookups)
}

#[derive(Clone)]
struct Candidate {
    metadata: GameMetadata,
    has_external_id: bool,
    entities: BTreeMap<String, BTreeSet<Entity>>,
    aliases: BTreeSet<String>,
    external_ids: BTreeMap<(String, String), ExternalId>,
}

impl Candidate {
    fn new(qid: &str, title: &str, description: Option<&str>, fetched_at: u64) -> Self {
        Self {
            metadata: GameMetadata {
                source: "wikidata".into(),
                entity_id: qid.into(),
                url: format!("https://www.wikidata.org/wiki/{qid}"),
                title: title.into(),
                description: description.filter(|text| valid_text(text, 2048)).map(str::to_owned),
                aliases: Vec::new(),
                genres: Vec::new(),
                genre_ancestors: Vec::new(),
                platforms: Vec::new(),
                developers: Vec::new(),
                publishers: Vec::new(),
                release_dates: Vec::new(),
                external_ids: Vec::new(),
                fetched_at,
                matched_by: MatchMethod::ExactTitle,
            },
            has_external_id: false,
            entities: BTreeMap::new(),
            aliases: BTreeSet::new(),
            external_ids: BTreeMap::new(),
        }
    }

    fn add_field(&mut self, row: &Value) -> Result<(), String> {
        let Some(field) = optional_binding_value(row, "field") else {
            return Ok(());
        };
        let value_binding = if field == "externalId" { "externalId" } else { "value" };
        let Some(value) = optional_binding_value(row, value_binding) else {
            return Ok(());
        };
        match field {
            "identity" => {}
            "genre" | "genreAncestor" | "platform" | "developer" | "publisher" => {
                let id = qid_from_binding(row, "value")?;
                let label = binding_value(row, "valueLabel")?;
                if !valid_text(label, 256) {
                    return Err("Wikidata response contains an invalid entity label".into());
                }
                self.entities
                    .entry(field.to_owned())
                    .or_default()
                    .insert(Entity { id: id.into(), label: label.into() });
            }
            "releaseDate" => {
                if valid_text(value, 64) {
                    self.metadata.release_dates.push(value.into());
                }
            }
            "alias" => {
                if valid_text(value, 256) && normalized_title(value) != normalized_title(&self.metadata.title) {
                    self.aliases.insert(value.into());
                }
            }
            "externalId" => {
                let Some(platform) = property_platform(property_from_binding(row, "externalProp")?) else {
                    return Ok(());
                };
                if valid_text(value, 256) {
                    let external = ExternalId { platform: platform.into(), id: value.into() };
                    self.external_ids.insert((external.platform.clone(), external.id.clone()), external);
                }
            }
            _ => return Err("Wikidata response contains an unknown field".into()),
        }
        Ok(())
    }

    fn finish(&self) -> GameMetadata {
        let mut metadata = self.metadata.clone();
        metadata.aliases = self.aliases.iter().cloned().collect();
        metadata.external_ids = self.external_ids.values().cloned().collect();
        metadata.genres = self.entities.get("genre").cloned().unwrap_or_default().into_iter().collect();
        metadata.genre_ancestors = self.entities.get("genreAncestor").cloned().unwrap_or_default().into_iter().collect();
        metadata.platforms = self.entities.get("platform").cloned().unwrap_or_default().into_iter().collect();
        metadata.developers = self.entities.get("developer").cloned().unwrap_or_default().into_iter().collect();
        metadata.publishers = self.entities.get("publisher").cloned().unwrap_or_default().into_iter().collect();
        metadata.release_dates.sort();
        metadata.release_dates.dedup();
        metadata
    }
}

fn binding_value<'a>(row: &'a Value, name: &str) -> Result<&'a str, String> {
    optional_binding_value(row, name).ok_or_else(|| format!("Wikidata response is missing {name}"))
}
fn optional_binding_value<'a>(row: &'a Value, name: &str) -> Option<&'a str> {
    row.get(name)?.get("value")?.as_str()
}
fn binding_optional_value<'a>(row: &'a Value, name: &str) -> Option<&'a str> {
    optional_binding_value(row, name)
}
fn qid_from_binding<'a>(row: &'a Value, name: &str) -> Result<&'a str, String> {
    let uri = binding_value(row, name)?;
    let Some(qid) = uri.strip_prefix("http://www.wikidata.org/entity/") else {
        return Err(format!("Wikidata response contains invalid {name} URI"));
    };
    if valid_qid(qid) { Ok(qid) } else { Err(format!("Wikidata response contains invalid {name} QID")) }
}
fn property_from_binding<'a>(row: &'a Value, name: &str) -> Result<&'a str, String> {
    let uri = binding_value(row, name)?;
    let Some(property) = uri.strip_prefix("http://www.wikidata.org/prop/direct/") else {
        return Err("Wikidata response contains an invalid property URI".into());
    };
    if supported_properties().contains(&property) { Ok(property) } else { Err("Wikidata response contains an unsupported property".into()) }
}

fn title_variants(title: &str) -> Vec<String> {
    let without_marks: String = title.chars().filter(|character| !matches!(character, '™' | '®' | '©')).collect();
    let mut values = vec![title.to_owned(), title.to_lowercase()];
    if without_marks != title {
        values.push(without_marks.to_owned());
        values.push(without_marks.to_lowercase());
    }
    values.dedup();
    values
}

fn sparql_string(value: &str) -> String {
    let mut out = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => out.push(' '),
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

fn supported_id(platform: &str, id: &str) -> Option<(&'static str, String)> {
    let platform: String = platform.chars().filter(char::is_ascii_alphanumeric).flat_map(char::to_lowercase).collect();
    let value = id.trim();
    let digits = |value: &str| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit());
    let slug = |value: &str| {
        !value.is_empty()
            && value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_')
            && value.bytes().any(|byte| byte.is_ascii_alphanumeric())
    };
    match platform.as_str() {
        "steam" if digits(value) && !value.starts_with('0') => Some(("P1733", value.into())),
        "gog" | "gogcom" if digits(value) => Some(("P12727", value.into())),
        "gog" | "gogcom" if value.strip_prefix("game/").is_some_and(slug) => Some(("P2725", value.into())),
        "epicstore" | "epicgamesstore" if slug(value)
            && !(value.len() >= 20 && value.bytes().all(|byte| byte.is_ascii_hexdigit())) => Some(("P6278", value.into())),
        "ubisoft" | "ubisoftconnect" | "ubisoftstore" if value.len() == 24 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) => Some(("P8268", value.into())),
        "ea" | "eaapp" | "eagames" | "origin" if valid_ea_path(value) => Some(("P8261", value.into())),
        "microsoftstore" if (value.len() == 12 || value.len() == 14) && value.bytes().all(|byte| byte.is_ascii_alphanumeric()) => Some(("P5885", value.to_ascii_lowercase())),
        _ => None,
    }
}

fn valid_ea_path(value: &str) -> bool {
    let mut segments = value.split('/');
    let (Some(first), Some(second)) = (segments.next(), segments.next()) else { return false; };
    segments.next().is_none() && [first, second].iter().all(|segment| {
        !segment.is_empty() && segment.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    })
}

fn supported_properties() -> Vec<&'static str> {
    vec!["P1733", "P12727", "P2725", "P6278", "P8268", "P8261", "P5885", "P5944", "P5971", "P5999", "P8084", "P4685", "P4477", "P7294", "P13193"]
}
fn supported_property_iris() -> String { supported_properties().into_iter().map(|property| format!("wdt:{property}")).collect::<Vec<_>>().join(" ") }
fn property_platform(property: &str) -> Option<&'static str> {
    Some(match property {
        "P1733" => "steam", "P12727" | "P2725" => "gog", "P6278" => "epic", "P8268" => "ubisoft",
        "P8261" => "ea", "P5885" => "microsoftStore", "P5944" | "P5971" | "P5999" => "playstationStore",
        "P8084" | "P4685" => "nintendoStore", "P4477" => "humble", "P7294" => "itchIo", "P13193" => "battleNet",
        _ => return None,
    })
}

#[cfg(test)]
#[path = "wikidata_tests.rs"]
mod tests;
