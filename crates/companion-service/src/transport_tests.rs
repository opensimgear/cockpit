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

#[path = "feedback_transport_tests.rs"]
mod feedback_cases;


#[cfg(windows)]
#[test]
fn short_code_pairing_persists_both_ends_tracks_presence_and_revocation() {
    let directory=tempfile::tempdir().unwrap();
    let sink=Arc::new(Recorder::default());
    let hub=Hub::open(directory.path().join("host-record"),sink.clone()).unwrap();
    let server=ServerRuntime::new(hub.clone(),directory.path().join("tls")).unwrap();
    let status=server.start(ServerConfig{bind:"127.0.0.1:0".parse().unwrap(),advertised_host:"127.0.0.1".into(),discovery:false}).unwrap();
    let offer=hub.command(&json!({"command":"companion_pair_offer","kind":"desktop","autoApprove":true})).unwrap();
    let payload=server.pairing_payload(offer["secret"].as_str().unwrap(),offer["expiresAtMs"].as_u64().unwrap()).unwrap();
    let code=hub.command(&json!({"command":"companion_pair_code","payload":payload})).unwrap();
    let client=ClientRuntime::open(directory.path().join("client")).unwrap();
    client.command(json!({"type":"pair-code-start","code":code["code"],"endpoint":status.endpoint,"name":"Code-paired desktop"})).unwrap();
    wait(|| client.status().phase=="paired" && !client.status().busy);
    let state=hub.command(&json!({"command":"companion_status"})).unwrap();
    let id=state["clients"][0]["companionId"].clone();
    assert_eq!(state["clients"][0]["kind"],"desktop");
    wait(||hub.command(&json!({"command":"companion_status"})).unwrap()["clients"][0]["connected"]==true);
    assert!(sink.events.lock().unwrap().is_empty());
    let reopened=Hub::open(directory.path().join("host-record"),sink.clone()).unwrap();
    assert_eq!(reopened.command(&json!({"command":"companion_status"})).unwrap()["clients"][0]["companionId"],id);
    assert!(ClientRuntime::open(directory.path().join("client")).unwrap().status().paired_host.is_some());
    hub.command(&json!({"command":"companion_revoke","companionId":id})).unwrap();
    assert!(hub.command(&json!({"command":"companion_status"})).unwrap()["clients"].as_array().unwrap().is_empty());
    drop(client);
    server.stop();
}

#[test]
fn browser_https_serving_pairing_live_assignment_input_disconnect_and_revocation() {
    use crate::browser::BrowserAsset;
    use std::{io::{Read,Write}, net::TcpStream};
    use tungstenite::client::IntoClientRequest;
    static ASSETS:&[BrowserAsset]=&[BrowserAsset{path:"/",mime:"text/html",bytes:b"browser companion"}];
    let sink=Arc::new(Recorder::default());
    let hub=Hub::new("browser-host".into(),sink.clone());
    let directory=tempfile::tempdir().unwrap();
    let server=ServerRuntime::with_browser(hub.clone(),directory.path().to_path_buf(),ASSETS).unwrap();
    let status=server.start(ServerConfig{bind:"127.0.0.1:0".parse().unwrap(),advertised_host:"127.0.0.1".into(),discovery:false}).unwrap();
    let origin=server.browser_url().unwrap().trim_end_matches('/').to_owned();
    let endpoint=status.endpoint.unwrap();let address=origin.trim_start_matches("https://");
    let payload=server.pairing_payload(&"a".repeat(43),10).unwrap();
    let exported=server.browser_certificate().unwrap();
    let exported_base64=exported.lines().filter(|line|!line.starts_with("-----")).collect::<String>();
    assert_eq!(exported_base64,payload.ca_certificate);
    assert!(!exported.contains("PRIVATE"));
    let config=tls::pinned_client_config(&exported_base64,&payload.ca_spki_sha256).unwrap();
    let stream=|| {
        let tcp=TcpStream::connect(address).unwrap();tcp.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        rustls::StreamOwned::new(rustls::ClientConnection::new(config.clone(),rustls_pki_types::ServerName::try_from("127.0.0.1").unwrap()).unwrap(),tcp)
    };
    let http=|path:&str,host:&str| {
        let mut stream=stream();write!(stream,"GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
        let mut response=Vec::new();let mut buffer=[0;1024];
        loop{match stream.read(&mut buffer){Ok(0)=>break,Ok(n)=>response.extend_from_slice(&buffer[..n]),Err(e) if e.kind()==std::io::ErrorKind::UnexpectedEof=>break,Err(e)=>panic!("{e}")}}
        String::from_utf8(response).unwrap()
    };
    let response=http("/",address);assert!(response.starts_with("HTTP/1.1 200"));assert!(response.contains("frame-ancestors 'none'"));assert!(response.ends_with("browser companion"));
    assert!(http("/../../Cargo.toml",address).starts_with("HTTP/1.1 404"));
    assert!(http("/","attacker.example").starts_with("HTTP/1.1 403"));
    let connect=|request_origin:&str| {
        let mut request=format!("wss://{address}/socket").into_client_request().unwrap();
        request.headers_mut().insert("Origin",request_origin.parse().unwrap());
        request.headers_mut().insert("Sec-WebSocket-Protocol",crate::server::SUBPROTOCOL.parse().unwrap());
        tungstenite::client(request,stream())
    };
    assert!(connect("https://attacker.example").is_err());
    // Native pinned clients retain their separate, Origin-free protocol endpoint.
    let mut native=endpoint.into_client_request().unwrap();
    native.headers_mut().insert("Sec-WebSocket-Protocol",crate::server::SUBPROTOCOL.parse().unwrap());
    let (mut native,_)=tungstenite::client(native,stream()).unwrap();native.close(None).unwrap();drop(native);
    let (mut socket,_)=connect(&origin).unwrap();
    let call=|socket:&mut tungstenite::WebSocket<rustls::StreamOwned<rustls::ClientConnection,TcpStream>>,value| {
        send_json(socket,&value).unwrap();receive_json(socket,Duration::from_secs(2)).unwrap()
    };
    assert_eq!(call(&mut socket,json!({"type":"presence"}))["ok"],false);
    let canonical=document();let hash=tls::sha256(canonical.as_bytes());
    hub.command(&json!({"command":"companion_publish","canonicalDocument":canonical,"publicationHash":hash,"assets":[]})).unwrap();
    let offer=hub.command(&json!({"command":"companion_pair_offer","kind":"browser","autoApprove":true})).unwrap();
    let pending=call(&mut socket,json!({"type":"pair","secret":offer["secret"],"companionId":"browser-one","name":"Browser"}));
    let paired=call(&mut socket,json!({"type":"pair-status","secret":offer["secret"],"requestId":pending["requestId"]}));
    let token=paired["token"].clone();assert!(token.is_string());
    assert_eq!(call(&mut socket,json!({"type":"pair","secret":offer["secret"],"companionId":"browser-two","name":"Other"}))["ok"],false);
    assert_eq!(call(&mut socket,json!({"type":"auth","token":token}))["ok"],true);
    hub.command(&json!({"command":"companion_assign","instanceId":"browser-panel","companionId":"browser-one","publicationHash":hash})).unwrap();
    assert_eq!(call(&mut socket,json!({"type":"presence"}))["assignments"][0]["instanceId"],"browser-panel");
    let opened=call(&mut socket,json!({"type":"open","instanceId":"browser-panel","publicationHash":hash}));
    assert_eq!(opened["publication"]["publicationHash"],hash);
    let frame=|kind:&str,seq:&str|json!({"version":1,"type":kind,"clientId":opened["clientId"],"sessionId":opened["sessionId"],"generation":opened["generation"],"publicationHash":hash,"sequence":seq});
    let mut baseline=frame("baseline","0");baseline["values"]=json!({"press":false});
    assert_eq!(call(&mut socket,baseline)["ok"],true);
    let mut input=frame("input","1");input["changes"]=json!([{"controlId":"press","value":true}]);
    assert_eq!(call(&mut socket,input)["ok"],true);
    assert!(sink.events.lock().unwrap().iter().any(|e|e.contains("true")));
    socket.close(None).unwrap();drop(socket);
    wait(||sink.events.lock().unwrap().iter().any(|e|e=="loss"));
    // A new browser window uses the saved credential, with a fresh session/baseline.
    let (mut socket,_)=connect(&origin).unwrap();
    assert_eq!(call(&mut socket,json!({"type":"auth","token":token}))["ok"],true);
    assert_eq!(call(&mut socket,frame("input","2"))["ok"],false);
    hub.command(&json!({"command":"companion_unassign","instanceId":"browser-panel"})).unwrap();
    assert_eq!(call(&mut socket,json!({"type":"presence"}))["assignments"],json!([]));
    hub.command(&json!({"command":"companion_revoke","companionId":"browser-one"})).unwrap();
    assert_eq!(call(&mut socket,json!({"type":"presence"}))["ok"],false);
    socket.close(None).unwrap();drop(socket);
    let (mut socket,_)=connect(&origin).unwrap();
    assert_eq!(call(&mut socket,json!({"type":"auth","token":token}))["ok"],false);
    drop(socket);drop(server);
}

#[cfg(windows)]
#[path = "render_transport_tests.rs"]
mod render_transport;
