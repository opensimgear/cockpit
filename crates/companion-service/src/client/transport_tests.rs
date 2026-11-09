use super::*;
use crate::{
    persistence::StateStorage,
    server::{websocket_config, PairingPayload, SUBPROTOCOL},
    tls::NativeIdentity,
};
use rustls::{ServerConnection, StreamOwned};
use serde_json::json;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tungstenite::{handshake::server::Response, Message, WebSocket};

#[derive(Default)]
struct MemoryStore {
    value: Mutex<Option<Vec<u8>>>,
    writes: AtomicUsize,
}

impl StateStorage for MemoryStore {
    fn read(&self) -> Result<Option<Vec<u8>>, String> {
        Ok(self.value.lock().unwrap().clone())
    }

    fn write(&self, bytes: &[u8]) -> Result<(), String> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        *self.value.lock().unwrap() = Some(bytes.to_vec());
        Ok(())
    }

    fn delete(&self) -> Result<(), String> {
        *self.value.lock().unwrap() = None;
        Ok(())
    }
}

fn fixture() -> (ClientRuntime, Arc<MemoryStore>, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let storage = Arc::new(MemoryStore::default());
    let client = ClientRuntime::with_storage(
        PathBuf::from(directory.path()),
        storage.clone(),
    )
    .unwrap();
    (client, storage, directory)
}

fn pairing_payload(endpoint: String, identity: &NativeIdentity) -> PairingPayload {
    PairingPayload {
        version: 1,
        endpoint,
        host_id: identity.host_id.clone(),
        ca_certificate: identity.ca_base64(),
        ca_spki_sha256: identity.pin().unwrap(),
        secret: "fixture-pairing-secret".into(),
        expires_at_ms: u64::MAX,
    }
}

fn wait_for_status(
    client: &ClientRuntime,
    timeout: Duration,
    predicate: impl Fn(&ClientStatus) -> bool,
) -> ClientStatus {
    let wake = client.status_wake();
    let _ = wake.bind_thread(thread::current());
    let deadline = Instant::now() + timeout;
    loop {
        let observed = wake.observed();
        let status = client.status();
        if predicate(&status) {
            return status;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "Timed out waiting for companion client status");
        wake.wait_scheduled(observed, Some(remaining));
    }
}

fn start_pairing(client: &ClientRuntime, payload: PairingPayload) {
    client
        .command(json!({
            "type":"pair-start",
            "payload":serde_json::to_value(payload).unwrap(),
            "name":"Fixture panel"
        }))
        .unwrap();
}

fn accept_tls_websocket(tcp: TcpStream, config: Arc<rustls::ServerConfig>) -> WebSocket<StreamOwned<ServerConnection, TcpStream>> {
    let tls = StreamOwned::new(ServerConnection::new(config).unwrap(), tcp);
    tungstenite::accept_hdr_with_config(
        tls,
        |_request: &tungstenite::handshake::server::Request, mut response: Response| {
            response.headers_mut().insert(
                "sec-websocket-protocol",
                SUBPROTOCOL.parse().unwrap(),
            );
            Ok(response)
        },
        Some(websocket_config()),
    )
    .unwrap()
}

fn localhost_listener() -> TcpListener {
    TcpListener::bind("127.0.0.1:0").unwrap()
}

#[test]
fn invalidating_a_tls_stalled_pairing_cancels_the_initial_handshake() {
    let (client, storage, _directory) = fixture();
    let identity = NativeIdentity::generate("fixture-host", vec!["127.0.0.1".into()]).unwrap();
    let listener = localhost_listener();
    let endpoint = format!("wss://127.0.0.1:{}/companion", listener.local_addr().unwrap().port());
    let payload = pairing_payload(endpoint, &identity);
    let (hello_tx, hello_rx) = mpsc::channel();
    let peer = thread::spawn(move || {
        let (mut tcp, _) = listener.accept().unwrap();
        let mut hello = [0; 1];
        tcp.read_exact(&mut hello).unwrap();
        hello_tx.send(()).unwrap();
        let mut ignored = [0; 64];
        let _ = tcp.read(&mut ignored);
    });

    start_pairing(&client, payload);
    hello_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let connecting = wait_for_status(&client, Duration::from_secs(2), |status| status.busy);
    let started_epoch = connecting.epoch;

    let cancel_started = Instant::now();
    client.invalidate("test lifecycle cancellation");
    let stopped = wait_for_status(&client, Duration::from_secs(1), |status| !status.busy);

    assert!(cancel_started.elapsed() < Duration::from_secs(1));
    assert_eq!(stopped.epoch, started_epoch + 1);
    assert_eq!(stopped.epoch, client.status().epoch, "client epoch must not wrap on invalidation");
    assert_eq!(storage.writes.load(Ordering::SeqCst), 0);
    assert!(storage.value.lock().unwrap().is_none());
    peer.join().unwrap();
}

#[test]
fn invalidating_a_partial_pairing_reply_cancels_the_client_read_without_saving() {
    let (client, storage, _directory) = fixture();
    let identity = NativeIdentity::generate("fixture-host", vec!["127.0.0.1".into()]).unwrap();
    let server_config = identity.server_config().unwrap();
    let listener = localhost_listener();
    let endpoint = format!("wss://127.0.0.1:{}/companion", listener.local_addr().unwrap().port());
    let payload = pairing_payload(endpoint, &identity);
    let (partial_tx, partial_rx) = mpsc::channel();
    let peer = thread::spawn(move || {
        let (tcp, _) = listener.accept().unwrap();
        let mut socket = accept_tls_websocket(tcp, server_config);
        let pair = socket.read().unwrap();
        assert!(matches!(pair, Message::Text(_)));
        let reply = json!({
            "ok":true,
            "type":"paired",
            "hostId":"fixture-host",
            "companionId":"fixture-device",
            "token":"fixture-token",
            "assignments":[],
            "padding":"x".repeat(256)
        })
        .to_string();
        let bytes = reply.as_bytes();
        let mut frame = vec![0x81];
        let header_len = if bytes.len() <= 125 {
            frame.push(bytes.len() as u8);
            2
        } else if bytes.len() <= u16::MAX as usize {
            frame.push(126);
            frame.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
            4
        } else {
            panic!("fixture reply frame too large");
        };
        frame.extend_from_slice(bytes);
        socket.get_mut().write_all(&frame[..header_len + 8]).unwrap();
        socket.get_mut().flush().unwrap();
        partial_tx.send(()).unwrap();
        let mut ignored = [0; 64];
        let _ = socket.get_mut().read(&mut ignored);
    });

    start_pairing(&client, payload);
    partial_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let connecting = wait_for_status(&client, Duration::from_secs(2), |status| status.busy);
    let started_epoch = connecting.epoch;

    let cancel_started = Instant::now();
    client.invalidate("test partial reply cancellation");
    let stopped = wait_for_status(&client, Duration::from_secs(1), |status| !status.busy);

    assert!(cancel_started.elapsed() < Duration::from_secs(1));
    assert_eq!(stopped.epoch, started_epoch + 1);
    assert_eq!(storage.writes.load(Ordering::SeqCst), 0);
    assert!(storage.value.lock().unwrap().is_none());
    peer.join().unwrap();
}

#[test]
fn pending_pairing_sends_one_subscription_and_no_status_acquisition() {
    let (client, storage, _directory) = fixture();
    let identity = NativeIdentity::generate("fixture-host", vec!["127.0.0.1".into()]).unwrap();
    let host_id = identity.host_id.clone();
    let config = identity.server_config().unwrap();
    let listener = localhost_listener();
    let payload = pairing_payload(format!("wss://127.0.0.1:{}/companion",listener.local_addr().unwrap().port()), &identity);
    let (subscribed_tx,subscribed_rx)=mpsc::channel();
    let peer=thread::spawn(move||{
        let (tcp,_)=listener.accept().unwrap();
        tcp.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut socket=accept_tls_websocket(tcp,config);
        let pair:Value=serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(pair["type"],"pair");
        crate::server::send_json(&mut socket,&json!({"ok":true,"type":"pair-pending","requestId":"request"})).unwrap();
        let subscribe:Value=serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(subscribe,json!({"type":"pair-subscribe","requestId":"request","secret":pair["secret"]}));
        socket.get_mut().sock.set_read_timeout(Some(Duration::from_millis(650))).unwrap();
        assert!(socket.read().is_err_and(|error|crate::server::timed_out(&error)),"client must not poll pending approval");
        subscribed_tx.send(()).unwrap();
        crate::server::send_json(&mut socket,&json!({"ok":true,"type":"paired","requestId":"request","hostId":host_id,"companionId":pair["companionId"],"token":"fixture-token","assignments":[]})).unwrap();
    });
    start_pairing(&client,payload);
    subscribed_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    wait_for_status(&client,Duration::from_secs(2),|status|status.phase=="paired"&&!status.busy);
    assert_eq!(storage.writes.load(Ordering::SeqCst),1);
    peer.join().unwrap();
}

#[test]
fn native_pending_pairing_observes_reject_cancel_listener_stop_and_client_suspend() {
    use crate::{Hub,NullSourceSink,server::{ServerConfig,ServerRuntime}};
    for action in ["companion_reject","companion_pair_cancel","listener-stop","client-suspend"] {
        let (client,storage,directory)=fixture();
        let hub=Hub::new("host-test".into(),Arc::new(NullSourceSink));
        let server=ServerRuntime::new(hub.clone(),directory.path().join("host")).unwrap();
        server.start(ServerConfig{bind:"127.0.0.1:0".parse().unwrap(),advertised_host:"127.0.0.1".into(),discovery:false}).unwrap();
        let offer=hub.command(&json!({"command":"companion_pair_offer"})).unwrap();
        let payload=server.pairing_payload(offer["secret"].as_str().unwrap(),offer["expiresAtMs"].as_u64().unwrap()).unwrap();
        start_pairing(&client,payload);
        let pending=wait_for_status(&client,Duration::from_secs(3),|status|status.phase=="awaiting-approval");
        let began=Instant::now();
        match action {
            "listener-stop" => server.stop(),
            "client-suspend" => {
                client.invalidate_active("Native application suspended");
                hub.command(&json!({"command":"companion_pair_approve","requestId":pending.request_id})).unwrap();
            }
            _ => { hub.command(&json!({"command":action,"requestId":pending.request_id})).unwrap(); }
        }
        let ended=wait_for_status(&client,Duration::from_secs(1),|status|!status.busy);
        assert!(began.elapsed()<Duration::from_secs(1),"{action}");
        assert_ne!(ended.phase,"paired");
        assert_eq!(storage.writes.load(Ordering::SeqCst),0);
        if action=="client-suspend" {
            assert_eq!(ended.phase,"unpaired");
            assert_eq!(ended.epoch,pending.epoch+1);
            assert!(ended.request_id.is_none());
            client.invalidate_active("Native application resumed");
            assert_eq!(client.status().epoch,ended.epoch);
        }
        server.stop();
        assert_eq!(server.status().active_connections,0);
    }
}
