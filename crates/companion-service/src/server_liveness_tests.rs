use super::*;
use tungstenite::protocol::Role;

#[test]
fn browser_control_pongs_keep_the_socket_open_without_application_traffic() {
    let hub = Hub::new("browser-liveness".into(), Arc::new(crate::NullSourceSink));
    let offer = hub.command(&json!({"command":"companion_pair_offer","kind":"browser"})).unwrap();
    let secret = &offer["secret"];
    let pending = hub.handle_wire(
        json!({"type":"pair","secret":secret,"companionId":"idle-browser","name":"Idle browser"}),
        "pairing", hub.now_ms(),
    );
    hub.command(&json!({"command":"companion_pair_approve","requestId":pending["requestId"],"companionId":"idle-browser"})).unwrap();
    let paired = hub.handle_wire(
        json!({"type":"pair-status","secret":secret,"requestId":pending["requestId"]}),
        "pairing", hub.now_ms(),
    );

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(4))).unwrap();
    let (server, peer) = listener.accept().unwrap();
    server.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
    let worker_hub = hub.clone();
    let (done, closed) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        let socket = WebSocket::from_raw_socket(server, Role::Server, Some(websocket_config()));
        let result = serve_messages(socket, &worker_hub, "browser", &AtomicBool::new(false),
            &Mutex::new(Attempts::new()), peer.ip(), true);
        worker_hub.disconnect_connection("browser", worker_hub.now_ms());
        done.send(result).unwrap();
    });
    let mut client = WebSocket::from_raw_socket(stream, Role::Client, Some(websocket_config()));
    send_json(&mut client, &json!({"type":"auth","token":paired["token"]})).unwrap();
    let reply = client.read().unwrap().into_text().unwrap();
    assert_eq!(serde_json::from_str::<Value>(&reply).unwrap()["ok"], true);
    let started = Instant::now();
    for _ in 0..3 {
        assert!(matches!(client.read().unwrap(), Message::Ping(_)));
        client.flush().unwrap();
    }
    assert!(started.elapsed() > Duration::from_secs(5));
    assert!(hub.connection_authenticated("browser"));
    assert!(hub.command(&json!({"command":"companion_status"})).unwrap()["instances"].as_array().unwrap().is_empty());

    // A silent socket still expires. Stopping reads also stops automatic pongs.
    closed.recv_timeout(Duration::from_secs(7)).unwrap().unwrap();
    worker.join().unwrap();
    assert!(!hub.connection_authenticated("browser"));
}
