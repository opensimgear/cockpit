use super::*;
use osg_streams::NativeWake;

/// A dedicated, authenticated visual connection. Input and leases remain on the
/// primary connection, whose retirement wakes and closes this reader.
pub(super) fn serve<S: Read + Write>(
    mut socket: WebSocket<S>, hub: &Hub, connection: &str, stop: &AtomicBool,
    attempts: &Mutex<Attempts>, peer: IpAddr, frame: &Value,
) -> Result<(), String> {
    let wake = Arc::new(NativeWake::default());
    let admission = (|| {
        crate::hub::exact(frame, &["type", "sessionId", "generation", "publicationHash", "token"], &["version"])?;
        let session = frame["sessionId"].as_str().ok_or("Missing feedback session identity.")?;
        let generation = frame["generation"].as_str().and_then(|value| value.parse::<u64>().ok())
            .ok_or("Missing feedback generation.")?;
        let publication = frame["publicationHash"].as_str().ok_or("Missing feedback publication.")?;
        let credential = frame["token"].as_str().ok_or("Missing installation credential.")?;
        hub.subscribe_feedback(connection, session, generation, publication, credential, wake.clone())
    })();
    let mut reader = match admission {
        Ok(reader) => reader,
        Err(error) => {
            let error = if attempts.lock().map_err(|_| "Admission lock poisoned")?.allow(peer) {
                error
            } else {
                "Authentication attempt budget exceeded".into()
            };
            send_json(&mut socket, &json!({"ok":false,"error":error}))?;
            return Err(error);
        }
    };
    let _ = wake.bind_thread(thread::current());
    send_json(&mut socket, &json!({"ok":true,"type":"feedback-subscribed"}))?;
    while !stop.load(Ordering::Acquire) {
        let observed = wake.observed();
        match reader.take() {
            Ok(Some(feedback)) => send_json(&mut socket, &json!({"ok":true,"type":"feedback-event","feedback":feedback}))?,
            Ok(None) => {}
            Err(_) => break,
        }
        wake.wait_scheduled(observed, None);
    }
    let _ = socket.close(None);
    Ok(())
}
