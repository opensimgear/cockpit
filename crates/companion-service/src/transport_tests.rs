//! Real TLS/WebSocket tests; no controller drivers, games, firewall changes or external network.
use crate::{
    client::ClientRuntime,
    server::{receive_json, send_json, ServerConfig, ServerRuntime},
    tls::{self, NativeIdentity},
    Hub, SourceChange, SourceDefinition, SourceSink, SourceToken,
};
use serde_json::json;
use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<String>>,
}
impl SourceSink for Recorder {
    fn register(&self, _: SourceDefinition) -> Result<(), String> {
        Ok(())
    }
    fn baseline(&self, _: &SourceToken, _: u64, _: &[SourceChange]) -> Result<(), String> {
        self.events.lock().unwrap().push("baseline".into());
        Ok(())
    }
    fn changes(&self, _: &SourceToken, _: u64, changes: &[SourceChange]) -> Result<(), String> {
        self.events
            .lock()
            .unwrap()
            .push(serde_json::to_string(changes).unwrap());
        Ok(())
    }
    fn loss(&self, _: &SourceToken) -> Result<(), String> {
        self.events.lock().unwrap().push("loss".into());
        Ok(())
    }
}
fn document() -> String {
    let bounds = json!({"x":0.1,"y":0.1,"width":0.5,"height":0.4});
    let value = json!({"schemaVersion":2,"ui":{"schemaVersion":1,"id":"panel","title":"Transport test","controls":[{"id":"press","kind":"button","label":"Press"}],"pages":[{"id":"page","title":"Main","items":[{"kind":"control","refId":"press","bounds":bounds}]}],"feedbackWidgets":[],"assets":[]},"presentation":{"widgets":[{"id":"widget","pageId":"page","label":"Press","bounds":bounds,"controlIds":["press"],"feedbackIds":[],"kind":"button","config":{"buttonMode":"momentary","pulseMs":0,"repeatMs":0}}],"groups":[],"viewports":[{"pageId":"page","width":800,"height":600}],"grid":{"columns":24,"rows":16,"snap":true}},"targets":[{"id":"logical","label":"Target","axes":[],"buttons":1,"hats":0}],"bindings":[{"controlId":"press","targetId":"logical","kind":"button","index":0}]});
    serde_json_canonicalizer::to_string(&value).unwrap()
}
fn wait(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(6);
    while !condition() {
        assert!(Instant::now() < deadline, "condition timed out");
        thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn host_ca_pin_is_checked_and_leaf_is_not_accepted_as_a_ca() {
    let identity = NativeIdentity::generate("host", vec!["localhost".into()]).unwrap();
    assert!(tls::pinned_client_config(&identity.ca_base64(), &identity.pin().unwrap()).is_ok());
    assert!(tls::pinned_client_config(
        &identity.ca_base64(),
        &format!("sha256:{}", "0".repeat(64))
    )
    .is_err());
    assert!(tls::ca_spki_pin(&identity.server_cert_der).is_err());
    assert!(tls::verified_ca("not-base64", &identity.pin().unwrap()).is_err());
}
#[cfg(windows)]
#[test]
fn real_wss_pair_approval_publication_input_ui_stall_and_sealed_restart() {
    let directory = tempfile::tempdir().unwrap();
    let sink = Arc::new(Recorder::default());
    let hub = Hub::new("host-test".into(), sink.clone());
    let server = ServerRuntime::new(hub.clone(), directory.path().join("host")).unwrap();
    assert!(!server.status().enabled);
    let status = server
        .start(ServerConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            advertised_host: "127.0.0.1".into(),
            discovery: false,
        })
        .unwrap();
    assert!(status.enabled);
    let canonical = document();
    let publication_hash = tls::sha256(canonical.as_bytes());
    hub.command(&json!({"command":"companion_publish","canonicalDocument":canonical,"publicationHash":publication_hash,"assets":[]})).unwrap();
    let offer = hub
        .command(&json!({"command":"companion_pair_offer"}))
        .unwrap();
    let payload = server
        .pairing_payload(
            offer["secret"].as_str().unwrap(),
            offer["expiresAtMs"].as_u64().unwrap(),
        )
        .unwrap();
    let client = ClientRuntime::open(directory.path().join("client")).unwrap();
    client
        .command(json!({"type":"pair-start","payload":payload,"name":"Remote desktop"}))
        .unwrap();
    wait(|| client.status().phase == "awaiting-approval");
    let request_id = client.status().request_id.unwrap();
    let approved = hub
        .command(&json!({"command":"companion_pair_approve","requestId":request_id}))
        .unwrap();
    hub.command(&json!({"command":"companion_assign","instanceId":"remote-instance","companionId":approved["companionId"],"publicationHash":publication_hash})).unwrap();
    wait(|| client.status().phase == "paired" && !client.status().busy);
    let serialized = serde_json::to_string(&client.status()).unwrap();
    assert!(!serialized.contains("token"));
    assert!(!serialized.contains(offer["secret"].as_str().unwrap()));
    client
        .command(json!({"type":"refresh-assignments"}))
        .unwrap();
    wait(|| client.status().phase == "paired" && !client.status().busy);
    assert_eq!(
        client.status().assignments[0]["instanceId"],
        "remote-instance"
    );
    client
        .command(json!({"type":"connect","instanceId":"remote-instance"}))
        .unwrap();
    wait(|| client.status().phase == "ready");
    let epoch = client.status().epoch;
    let loaded = client.command(json!({"type":"publication"})).unwrap();
    assert_eq!(loaded["canonicalDocument"], canonical);
    assert_eq!(loaded["publicationHash"], publication_hash);
    assert_eq!(client.status().ack.unwrap()["ackStage"], "source-accepted");
    assert!(client
        .command(
            json!({"type":"input","epoch":epoch,"changes":[{"controlId":"press","value":true}]})
        )
        .is_err());
    client.command(json!({"type":"arm","epoch":epoch})).unwrap();
    client
        .command(json!({"type":"keepalive","epoch":epoch}))
        .unwrap();
    client
        .command(
            json!({"type":"input","epoch":epoch,"changes":[{"controlId":"press","value":true}]}),
        )
        .unwrap();
    wait(|| {
        sink.events
            .lock()
            .unwrap()
            .iter()
            .any(|event| event.contains("true"))
    });
    assert!(sink
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|event| event == "baseline"));
    // Intentionally stop renderer keepalives: native worker must neutralize and close independently.
    wait(|| client.status().phase == "disconnected");
    wait(|| {
        sink.events
            .lock()
            .unwrap()
            .iter()
            .any(|event| event == "loss")
    });
    assert!(sink
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|event| event.contains("false")));
    assert!(client.command(json!({"type":"arm","epoch":epoch})).is_err());
    drop(client);
    let restored = ClientRuntime::open(directory.path().join("client")).unwrap();
    assert_eq!(restored.status().phase, "paired");
    assert!(restored.status().session.is_none());
    assert!(!serde_json::to_string(&restored.status())
        .unwrap()
        .contains("token"));
    server.stop();
    assert!(!server.status().enabled);
    wait(|| server.status().active_connections == 0);
}
#[cfg(windows)]
#[test]
fn tls_rejects_wrong_authority_and_wss_frame_oversize_without_affecting_another_client() {
    let directory = tempfile::tempdir().unwrap();
    let hub = Hub::new("host-test".into(), Arc::new(Recorder::default()));
    let server = ServerRuntime::new(hub.clone(), directory.path().to_path_buf()).unwrap();
    let status = server
        .start(ServerConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            advertised_host: "127.0.0.1".into(),
            discovery: false,
        })
        .unwrap();
    let offer = hub
        .command(&json!({"command":"companion_pair_offer"}))
        .unwrap();
    let payload = server
        .pairing_payload(offer["secret"].as_str().unwrap(), 0)
        .unwrap();
    let other = NativeIdentity::generate("other-host", vec!["127.0.0.1".into()]).unwrap();
    assert!(crate::client::connect(
        status.endpoint.as_ref().unwrap(),
        &other.ca_base64(),
        &other.pin().unwrap()
    )
    .is_err());
    let mut valid = crate::client::connect(
        &payload.endpoint,
        &payload.ca_certificate,
        &payload.ca_spki_sha256,
    )
    .unwrap();
    send_json(
        &mut valid,
        &json!({"type":"pair","secret":payload.secret,"companionId":"test-device","name":"Test"}),
    )
    .unwrap();
    let reply = receive_json(&mut valid, Duration::from_secs(2)).unwrap();
    assert_eq!(reply["type"], "pair-pending");
    let mut oversized = crate::client::connect(
        &payload.endpoint,
        &payload.ca_certificate,
        &payload.ca_spki_sha256,
    )
    .unwrap();
    oversized
        .send(tungstenite::Message::Text("x".repeat(16 * 1024 + 1).into()))
        .unwrap();
    assert!(receive_json(&mut oversized, Duration::from_secs(2)).is_err());
    send_json(
        &mut valid,
        &json!({"type":"pair-status","requestId":reply["requestId"],"secret":payload.secret}),
    )
    .unwrap();
    assert_eq!(
        receive_json(&mut valid, Duration::from_secs(2)).unwrap()["type"],
        "pair-pending"
    );
    drop(valid);
    server.stop();
}
#[test]
fn publication_transfer_chunks_round_trip_without_losing_canonical_bytes() {
    use std::net::{TcpListener, TcpStream};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let expected = json!({"canonicalDocument":"x".repeat(40000),"assets":[]});
    let sent = expected.clone();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        send_json(&mut socket, &sent).unwrap();
    });
    let stream = TcpStream::connect(address).unwrap();
    let (mut socket, _) = tungstenite::client(format!("ws://{address}"), stream).unwrap();
    assert_eq!(
        receive_json(&mut socket, Duration::from_secs(2)).unwrap(),
        expected
    );
    server.join().unwrap();
}

#[cfg(windows)]
#[test]
fn one_real_listener_routes_two_different_ui_and_two_instances_of_one_ui_independently() {
    let directory = tempfile::tempdir().unwrap();
    let sink = Arc::new(Recorder::default());
    let hub = Hub::new("multi-host".into(), sink.clone());
    let server = ServerRuntime::new(hub.clone(), directory.path().join("host")).unwrap();
    server
        .start(ServerConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            advertised_host: "127.0.0.1".into(),
            discovery: false,
        })
        .unwrap();
    let flight: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/c1-flight-publication.json")).unwrap();
    let race: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/c1-racing-publication.json")).unwrap();
    for publication in [&flight, &race] {
        hub.command(&json!({"command":"companion_publish","canonicalDocument":publication["canonicalDocument"],"publicationHash":publication["publicationHash"],"capabilityFingerprint":publication["capabilityFingerprint"],"assets":publication["assets"]})).unwrap();
    }
    let mut clients = Vec::new();
    for (index, (instance, publication)) in [
        ("flight-a", &flight),
        ("flight-b", &flight),
        ("race", &race),
    ]
    .into_iter()
    .enumerate()
    {
        let offer = hub
            .command(&json!({"command":"companion_pair_offer"}))
            .unwrap();
        let payload = server
            .pairing_payload(
                offer["secret"].as_str().unwrap(),
                offer["expiresAtMs"].as_u64().unwrap(),
            )
            .unwrap();
        let client = ClientRuntime::open(directory.path().join(format!("client-{index}"))).unwrap();
        client
            .command(json!({"type":"pair-start","payload":payload,"name":format!("Panel {index}")}))
            .unwrap();
        wait(|| client.status().phase == "awaiting-approval");
        let approved = hub
            .command(&json!({"command":"companion_approve","requestId":client.status().request_id}))
            .unwrap();
        hub.command(&json!({"command":"companion_assign","instanceId":instance,"companionId":approved["companionId"],"publicationHash":publication["publicationHash"]})).unwrap();
        wait(|| client.status().phase == "paired" && !client.status().busy);
        client
            .command(json!({"type":"connect","instanceId":instance}))
            .unwrap();
        wait(|| client.status().phase == "ready");
        assert_eq!(
            client.status().publication_hash.as_deref(),
            publication["publicationHash"].as_str()
        );
        clients.push(client);
    }
    assert_ne!(
        clients[0].status().paired_host.as_ref().unwrap()["companionId"],
        clients[1].status().paired_host.as_ref().unwrap()["companionId"]
    );
    // Arm only after all clients are connected; send one physical primitive independently per instance.
    for (index, client) in clients.iter().enumerate() {
        let epoch = client.status().epoch;
        client.command(json!({"type":"arm","epoch":epoch})).unwrap();
        client
            .command(json!({"type":"keepalive","epoch":epoch}))
            .unwrap();
        let document: serde_json::Value = serde_json::from_str(
            client.command(json!({"type":"publication"})).unwrap()["canonicalDocument"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let control = document["ui"]["controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|control| control["kind"] == "button")
            .unwrap();
        client.command(json!({"type":"input","epoch":epoch,"changes":[{"controlId":control["id"],"value":true}]})).unwrap();
        assert!(index < 3);
    }
    let deadline = Instant::now() + Duration::from_millis(950);
    while Instant::now() < deadline {
        for client in &clients[1..] {
            client
                .command(json!({"type":"keepalive","epoch":client.status().epoch}))
                .unwrap();
        }
        thread::sleep(Duration::from_millis(50));
    }
    wait(|| clients[0].status().phase == "disconnected");
    assert_eq!(clients[1].status().phase, "armed");
    assert_eq!(clients[2].status().phase, "armed");
    let status = hub.command(&json!({"command":"companion_status"})).unwrap();
    assert_eq!(
        status["instances"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|instance| instance["armed"] == true)
            .count(),
        2
    );
    for client in &clients {
        client.invalidate("test complete");
    }
    server.stop();
}
#[cfg(windows)]
#[test]
fn occupied_port_is_reported_and_stalled_handshake_has_an_absolute_shutdown_bound() {
    let directory = tempfile::tempdir().unwrap();
    let hub = Hub::new("host".into(), Arc::new(Recorder::default()));
    let server = ServerRuntime::new(hub, directory.path().to_path_buf()).unwrap();
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    assert!(server
        .start(ServerConfig {
            bind: occupied.local_addr().unwrap(),
            advertised_host: "127.0.0.1".into(),
            discovery: false
        })
        .unwrap_err()
        .contains("Cannot bind"));
    drop(occupied);
    let started = server
        .start(ServerConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            advertised_host: "127.0.0.1".into(),
            discovery: false,
        })
        .unwrap();
    let uri: tungstenite::http::Uri = started.endpoint.unwrap().parse().unwrap();
    let _stalled = std::net::TcpStream::connect(("127.0.0.1", uri.port_u16().unwrap())).unwrap();
    wait(|| server.status().active_connections == 1);
    let start = Instant::now();
    server.stop();
    assert!(start.elapsed() < Duration::from_secs(6));
    assert_eq!(server.status().active_connections, 0);
}
