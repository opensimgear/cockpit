//! One terminal pairing reply, woken by the exact pending request or Stop.
use super::*;
use osg_streams::NativeWake;

pub(super) fn serve<S: Read + Write>(
    mut socket: WebSocket<S>, hub: &Hub, connection: &str, stop: &AtomicBool,
    attempts: &Mutex<Attempts>, peer: IpAddr, frame: &Value,
) -> Result<(), String> {
    let wake = Arc::new(NativeWake::default());
    let mut reader = match hub.subscribe_pairing(frame, connection, wake.clone()) {
        Ok(reader) => reader,
        Err(error) => return feedback_socket::reject(&mut socket, attempts, peer, error),
    };
    let _ = wake.bind_thread(thread::current());
    while !stop.load(Ordering::Acquire) {
        let observed = wake.observed();
        match reader.take_current() {
            Ok(reply) if reply["type"] == "paired" => {
                send_json(&mut socket, &reply)?;
                let _ = socket.close(None);
                return Ok(());
            }
            Ok(_) => {}
            Err(error) => {
                send_json(&mut socket, &json!({"ok":false,"error":error}))?;
                return Err(error);
            }
        }
        // This is the original offer's expiry, never a refresh cadence.
        let remaining = reader.remaining_ms();
        wake.wait_scheduled(observed, Some(Duration::from_millis(remaining)));
    }
    let _ = socket.close(None);
    Ok(())
}
