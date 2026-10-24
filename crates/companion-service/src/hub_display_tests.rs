use crate::{Hub, NullSourceSink, PublicationUpload};
use serde_json::{json, Value};
use std::sync::Arc;

fn wire(hub: &Hub, connection: &str, message: Value) -> Value {
    hub.handle_wire(message, connection, hub.now_ms())
}

fn pair(hub: &Hub, companion: &str) -> String {
    let offer = hub
        .command(&json!({"command":"companion_pair_offer","kind":"browser"}))
        .unwrap();
    let secret = offer["secret"].as_str().unwrap();
    let pending = wire(
        hub,
        "pairing",
        json!({"type":"pair","secret":secret,"companionId":companion,"name":companion}),
    );
    assert_eq!(pending["ok"], true, "{pending}");
    hub.command(&json!({"command":"companion_pair_approve","requestId":pending["requestId"],"companionId":companion})).unwrap();
    wire(
        hub,
        "pairing",
        json!({"type":"pair-status","secret":secret,"requestId":pending["requestId"]}),
    )["token"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn setup() -> (Hub, String, Value, String) {
    let hub = Hub::new("display-host".into(), Arc::new(NullSourceSink));
    let upload: PublicationUpload =
        serde_json::from_str(include_str!("../tests/fixtures/c1-flight-publication.json")).unwrap();
    let publication_hash = upload.publication_hash.clone();
    let document: Value = serde_json::from_str(&upload.canonical_document).unwrap();
    hub.retain_legacy_test_fixture(upload).unwrap();
    let token = pair(&hub, "screen");
    hub.command(&json!({"command":"companion_assign","companionId":"screen","instanceId":"panel","publicationHash":publication_hash})).unwrap();
    assert_eq!(
        wire(&hub, "browser", json!({"type":"auth","token":token}))["ok"],
        true
    );
    (hub, publication_hash, document, token)
}

fn display(
    publication_hash: &str,
    document: &Value,
    css_width: f64,
    css_height: f64,
    pixel_ratio: f64,
) -> Value {
    let viewport = &document["presentation"]["viewports"][0];
    json!({"publicationHash":publication_hash,"pageId":viewport["pageId"],"cssWidth":css_width,"cssHeight":css_height,"pixelRatio":pixel_ratio})
}

fn reports(hub: &Hub) -> Value {
    hub.command(&json!({"command":"companion_status"})).unwrap()["clients"][0]["displayReports"]
        .clone()
}

#[test]
fn authenticated_presence_reports_actual_dimensions_and_warning() {
    let (hub, publication_hash, document, _) = setup();
    let viewport = &document["presentation"]["viewports"][0];
    let panel_width = viewport["width"].as_u64().unwrap();
    let panel_height = viewport["height"].as_u64().unwrap();
    let sent = display(
        &publication_hash,
        &document,
        panel_width as f64 * 2.0,
        panel_height as f64 * 2.0,
        2.0,
    );
    let result = wire(&hub, "browser", json!({"type":"presence","display":sent}));
    assert_eq!(result["ok"], true, "{result}");
    let report = &reports(&hub)[0];
    assert_eq!(report["publicationHash"], publication_hash);
    assert_eq!(report["pageId"], viewport["pageId"]);
    assert_eq!(report["panelWidth"], panel_width);
    assert_eq!(report["panelHeight"], panel_height);
    assert_eq!(report["displayWidth"], panel_width * 4);
    assert_eq!(report["displayHeight"], panel_height * 4);
    assert_eq!(report["warning"], format!("The selected panel is too small for this display: {panel_width} × {panel_height} px, displayed at {} × {} px. Widgets may look blurry. Increase the panel size in Cockpit to at least {} × {} px and recompile, or use a smaller browser window.",panel_width*4,panel_height*4,panel_width*4,panel_height*4));

    let fitting = wire(
        &hub,
        "browser",
        json!({"type":"presence","display":display(&publication_hash,&document,panel_width as f64 / 2.0,panel_height as f64 / 2.0,2.0)}),
    );
    assert_eq!(fitting["ok"], true, "{fitting}");
    assert_eq!(reports(&hub)[0]["warning"], "");
}

#[test]
fn malformed_display_is_rejected_without_replacing_last_report() {
    let (hub, publication_hash, document, _) = setup();
    let valid = wire(
        &hub,
        "browser",
        json!({"type":"presence","display":display(&publication_hash,&document,800.0,600.0,2.0)}),
    );
    assert_eq!(valid["ok"], true);
    for bad in [
        json!({"publicationHash":publication_hash,"pageId":"p","cssWidth":0,"cssHeight":600,"pixelRatio":2}),
        json!({"publicationHash":publication_hash,"pageId":"p","cssWidth":16385,"cssHeight":600,"pixelRatio":2}),
        json!({"publicationHash":publication_hash,"pageId":"p","cssWidth":800,"cssHeight":600,"pixelRatio":9}),
        json!({"publicationHash":publication_hash,"pageId":"p","cssWidth":800,"cssHeight":600,"pixelRatio":2,"extra":true}),
        json!({"publicationHash":publication_hash,"pageId":"p","cssWidth":800,"cssHeight":600}),
    ] {
        assert_eq!(
            wire(&hub, "browser", json!({"type":"presence","display":bad}))["ok"],
            false
        );
    }
    assert_eq!(reports(&hub).as_array().unwrap().len(), 1);
}

#[test]
fn stale_refs_missing_null_disconnect_and_revoke_clear_reports() {
    let (hub, publication_hash, document, token) = setup();
    assert_eq!(
        wire(
            &hub,
            "browser",
            json!({"type":"presence","display":display(&publication_hash,&document,800.0,600.0,2.0)})
        )["ok"],
        true
    );
    let mut stale = display(&publication_hash, &document, 800.0, 600.0, 2.0);
    stale["publicationHash"] =
        json!("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    assert_eq!(
        wire(&hub, "browser", json!({"type":"presence","display":stale}))["ok"],
        true
    );
    assert!(reports(&hub).as_array().unwrap().is_empty());
    assert_eq!(
        wire(
            &hub,
            "browser",
            json!({"type":"presence","display":display(&publication_hash,&document,800.0,600.0,2.0)})
        )["ok"],
        true
    );
    assert_eq!(
        wire(&hub, "browser", json!({"type":"presence","display":null}))["ok"],
        true
    );
    assert!(reports(&hub).as_array().unwrap().is_empty());
    assert_eq!(
        wire(
            &hub,
            "browser",
            json!({"type":"presence","display":display(&publication_hash,&document,800.0,600.0,2.0)})
        )["ok"],
        true
    );
    hub.disconnect_connection("browser", hub.now_ms());
    assert!(reports(&hub).as_array().unwrap().is_empty());
    assert_eq!(
        wire(&hub, "browser", json!({"type":"auth","token":token}))["ok"],
        true
    );
    assert_eq!(
        wire(
            &hub,
            "browser",
            json!({"type":"presence","display":display(&publication_hash,&document,800.0,600.0,2.0)})
        )["ok"],
        true
    );
    hub.command(&json!({"command":"companion_revoke","companionId":"screen"}))
        .unwrap();
    assert!(
        hub.command(&json!({"command":"companion_status"})).unwrap()["clients"]
            .as_array()
            .unwrap()
            .iter()
            .all(|client| client["companionId"] != "screen")
    );
}

#[test]
fn presence_requires_authentication_and_unassigned_page_is_ignored() {
    let (hub, publication_hash, document, _) = setup();
    assert_eq!(
        wire(
            &hub,
            "anonymous",
            json!({"type":"presence","display":display(&publication_hash,&document,800.0,600.0,2.0)})
        )["ok"],
        false
    );
    let mut stale_page = display(&publication_hash, &document, 800.0, 600.0, 2.0);
    stale_page["pageId"] = json!("removed-page");
    assert_eq!(
        wire(
            &hub,
            "browser",
            json!({"type":"presence","display":stale_page})
        )["ok"],
        true
    );
    assert!(reports(&hub).as_array().unwrap().is_empty());
}

#[test]
fn generic_presence_reports_dimensions_without_an_assignment_for_authoring() {
    let (hub, _, _, _) = setup();
    hub.command(&json!({"command":"companion_unassign","instanceId":"panel"}))
        .unwrap();
    let report = wire(
        &hub,
        "browser",
        json!({"type":"presence","display":{"cssWidth":1280,"cssHeight":720,"pixelRatio":2}}),
    );
    assert_eq!(report["ok"], true, "{report}");
    let report = &reports(&hub)[0];
    assert!(report["publicationHash"].is_null());
    assert!(report["pageId"].is_null());
    assert!(report["panelWidth"].is_null());
    assert!(report["panelHeight"].is_null());
    assert_eq!(report["displayWidth"], 2560);
    assert_eq!(report["displayHeight"], 1440);
    assert_eq!(report["warning"], "");
    let partial = wire(
        &hub,
        "browser",
        json!({"type":"presence","display":{"publicationHash":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","cssWidth":1280,"cssHeight":720,"pixelRatio":2}}),
    );
    assert_eq!(partial["ok"], false);
    assert_eq!(reports(&hub)[0]["displayWidth"], 2560);
}
