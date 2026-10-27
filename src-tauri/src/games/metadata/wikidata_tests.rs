use super::*;

fn query(key: &str, title: &str, platform: &str, id: &str) -> MetadataQuery {
    MetadataQuery {
        key: key.into(),
        title: title.into(),
        external_ids: vec![ExternalId { platform: platform.into(), id: id.into() }],
    }
}

fn binding(value: &str, kind: &str) -> Value {
    serde_json::json!({"type":kind,"value":value})
}

fn row(key: &str, qid: &str, title: &str, matched_by: &str, label: &str) -> Value {
    serde_json::json!({
        "key":binding(key,"literal"),
        "item":binding(&format!("http://www.wikidata.org/entity/{qid}"),"uri"),
        "itemLabel":binding(title,"literal"),
        "description":binding("A game description","literal"),
        "matchedBy":binding(matched_by,"literal"),
        "matchLabel":binding(label,"literal"),
        "field":binding("identity","literal"),
        "value":binding("","literal"),
        "valueLabel":binding("","literal")
    })
}

fn response(rows: Vec<Value>) -> Vec<u8> {
    serde_json::json!({"results":{"bindings":rows}}).to_string().into_bytes()
}

#[test]
fn query_uses_opaque_keys_and_only_safe_store_identifiers() {
    let queries = vec![
        query("private-record-key", "Portal 2", "Steam", "620"),
        query("another-secret", "Example", "Epic Games", "4f1c8b7e2f4a"),
        query("third-secret", "Example", "Ubisoft Connect", "56c494ae88a7e300458b4d72"),
        query("battle-secret", "Overwatch", "Battle.net", "overwatch"),
    ];
    let sparql = super::query(&queries);
    assert!(sparql.contains("(\"k0\" wdt:P1733 \"620\")"));
    assert!(sparql.contains("(\"k2\" wdt:P8268 \"56c494ae88a7e300458b4d72\")"));
    assert!(!sparql.contains("private-record-key"));
    assert!(!sparql.contains("another-secret"));
    assert!(!sparql.contains("battle-secret"));
    assert!(sparql.contains("VALUES (?key ?needle)"));
    assert!(sparql.contains("?item (rdfs:label|skos:altLabel) ?needle ."));
    assert!(sparql.contains("BIND(?needle AS ?matchLabel)"));
    assert!(!sparql.contains("LCASE("));
    assert!(sparql.contains("wdt:P31/wdt:P279* wd:Q7889"));

    let title_only = query("secret-bnet", "Portal 2", "Battle.net", "some-app-code");
    let title_query = super::query(&[title_only]);
    assert!(title_query.contains("VALUES (?key ?matchProp ?matchId) { }"));
    assert!(title_query.contains("(\"k0\" \"Portal 2\"@en)"));
    assert!(title_query.contains("(\"k0\" \"Portal 2\"@mul)"));
    assert!(!title_query.contains("UNDEF"));
}

#[test]
fn query_bounds_traversal_to_matches_and_prefers_one_english_or_neutral_label() {
    let sparql = super::query(&[query("private-key", "Portal 2", "Steam", "620")]);
    assert!(sparql.contains("hint:Query hint:optimizer \"None\""));
    let matches = sparql.find("VALUES (?key ?matchProp ?matchId)").unwrap();
    let type_check = sparql.find("?item wdt:P31/wdt:P279* wd:Q7889").unwrap();
    let fields = sparql.find("?item wdt:P136 ?value").unwrap();
    assert!(matches < type_check && type_check < fields);
    assert!(sparql.contains("hint:Prior hint:gearing \"forward\""));
    assert!(sparql.contains("?item rdfs:label ?englishLabel . FILTER(LANG(?englishLabel) = \"en\")"));
    assert!(sparql.contains("?item rdfs:label ?neutralLabel . FILTER(LANG(?neutralLabel) = \"mul\")"));
    assert!(sparql.contains("BIND(COALESCE(?englishLabel, ?neutralLabel) AS ?itemLabel)"));
    assert!(sparql.contains("wikibase:language \"en,mul\""));
}

#[test]
fn safe_identifier_formats_cover_gog_ea_and_microsoft_store() {
    assert_eq!(supported_id("GOG", "1361243432"), Some(("P12727", "1361243432".into())));
    assert_eq!(supported_id("GOG", "game/cyberpunk_2077"), Some(("P2725", "game/cyberpunk_2077".into())));
    assert_eq!(supported_id("EA App", "command-and-conquer/command-and-conquer-remastered"), Some(("P8261", "command-and-conquer/command-and-conquer-remastered".into())));
    assert_eq!(supported_id("Microsoft Store", "9NLRT31Z4RWM"), Some(("P5885", "9nlrt31z4rwm".into())));
    assert_eq!(supported_id("Xbox", "9NLRT31Z4RWM"), None);
    assert_eq!(supported_id("Epic Games", "4f1c8b7e2f4a"), None);
    assert_eq!(supported_id("Epic Games Store", "winter-sale-2025"), Some(("P6278", "winter-sale-2025".into())));
    assert_eq!(supported_id("Ubisoft Connect", "overwatch"), None);
    assert_eq!(supported_id("Battle.net", "overwatch"), None);
}

#[test]
fn parses_exact_title_and_metadata_rows_into_model() {
    let queries = vec![query("local-key", "Portal 2™", "Steam", "620")];
    let mut item = row("k0", "Q123", "Portal 2", "exact-title", "PORTAL 2");
    item["field"] = binding("genre", "literal");
    item["value"] = binding("http://www.wikidata.org/entity/Q659563", "uri");
    item["valueLabel"] = binding("video game genre", "literal");
    let parsed = parse(&queries, &response(vec![item]), 1234).unwrap();
    let lookup = parsed.get("local-key").unwrap();
    assert_eq!(lookup.status, LookupStatus::Matched);
    let metadata = lookup.metadata.as_ref().unwrap();
    assert_eq!(metadata.entity_id, "Q123");
    assert_eq!(metadata.matched_by, MatchMethod::ExactTitle);
    assert_eq!(metadata.genres, vec![Entity { id: "Q659563".into(), label: "video game genre".into() }]);
    assert_eq!(metadata.fetched_at, 1234);
}

#[test]
fn parses_external_identifier_field_rows() {
    let queries = vec![query("local-key", "Portal 2", "Steam", "620")];
    let mut item = row("k0", "Q279446", "Portal 2", "exact-title", "Portal 2");
    item["field"] = binding("externalId", "literal");
    item.as_object_mut().unwrap().remove("value");
    item["externalId"] = binding("620", "literal");
    item["externalProp"] = binding("http://www.wikidata.org/prop/direct/P1733", "uri");
    let parsed = parse(&queries, &response(vec![item]), 1234).unwrap();
    assert_eq!(parsed["local-key"].metadata.as_ref().unwrap().external_ids,
        vec![ExternalId { platform: "steam".into(), id: "620".into() }]);
}

#[test]
fn exact_id_precedes_title_and_multiple_exact_items_are_ambiguous() {
    let queries = vec![query("k-user", "Portal 2", "Steam", "620")];
    let mut id_row = row("k0", "Q123", "Portal 2", "external-id", "");
    id_row["matchProp"] = binding("http://www.wikidata.org/prop/direct/P1733", "uri");
    id_row["matchId"] = binding("620", "literal");
    let title_row = row("k0", "Q456", "Portal 2", "exact-title", "Portal 2");
    let parsed = parse(&queries, &response(vec![id_row.clone(), title_row]), 456).unwrap();
    assert_eq!(parsed["k-user"].metadata.as_ref().unwrap().entity_id, "Q123");
    assert_eq!(parsed["k-user"].metadata.as_ref().unwrap().matched_by, MatchMethod::ExternalId);

    let mut second = id_row.clone();
    second["item"] = binding("http://www.wikidata.org/entity/Q789", "uri");
    let parsed = parse(&queries, &response(vec![id_row, second]), 456).unwrap();
    assert_eq!(parsed["k-user"].status, LookupStatus::Ambiguous);
    assert!(parsed["k-user"].metadata.is_none());
}

#[test]
fn parser_rejects_unknown_keys_wrong_id_matches_and_bad_response_shapes() {
    let queries = vec![query("my-key", "Portal 2", "Steam", "620")];
    assert!(parse(&queries, b"{}", 10).is_err());
    assert!(parse(&queries, &vec![b' '; MAX_RESPONSE_BYTES + 1], 10).is_err());
    assert!(parse(&queries, &response(vec![row("k99", "Q123", "Portal 2", "exact-title", "Portal 2")]), 10).is_err());

    let mut bad_id = row("k0", "Q123", "Portal 2", "external-id", "");
    bad_id["matchProp"] = binding("http://www.wikidata.org/prop/direct/P1733", "uri");
    bad_id["matchId"] = binding("999", "literal");
    assert!(parse(&queries, &response(vec![bad_id]), 10).is_err());

    let fuzzy = row("k0", "Q123", "Portal 2", "exact-title", "Portal 2: The Movie");
    let parsed = parse(&queries, &response(vec![fuzzy]), 10).unwrap();
    assert_eq!(parsed["my-key"].status, LookupStatus::NotFound);

    let mut hostile_uri = row("k0", "Q123", "Portal 2", "exact-title", "Portal 2");
    hostile_uri["item"] = binding("https://evil.example/entity/Q123", "uri");
    assert!(parse(&queries, &response(vec![hostile_uri]), 10).is_err());

    let mut hostile_property = row("k0", "Q123", "Portal 2", "external-id", "");
    hostile_property["matchProp"] = binding("https://evil.example/prop/direct/P1733", "uri");
    hostile_property["matchId"] = binding("620", "literal");
    assert!(parse(&queries, &response(vec![hostile_property]), 10).is_err());
}
