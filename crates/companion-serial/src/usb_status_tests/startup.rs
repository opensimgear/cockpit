use super::*;
use super::fixture::{duplex, start_worker_with_reopen};

#[test]
fn startup_accepts_a_delayed_reply_after_retrying_the_same_port() {
    let fixture = start_worker(None, false);
    let first = fixture.receive();
    let retry = fixture.receive();
    assert_eq!(first["command"], "status");
    assert_eq!(retry, first, "retry must preserve the pending discovery identity");

    fixture.send(json!({
        "protocolVersion":2,"deviceType":"esp32","statusEvents":true,
        "requestId":first["requestId"],"companionId":"","status":"offline","setupSaved":false,
    }));
    let watch = fixture.receive();
    assert_eq!(watch["command"], "status-watch");
    fixture.wait_field("ready", json!(true));
    fixture.command(Action::Stop);
    fixture.handle.join().unwrap().unwrap();
}

#[test]
fn firmware_ready_hint_retries_discovery_before_the_timer() {
    let fixture = start_worker(None, false);
    let first = fixture.receive();
    fixture.send(json!({"status":"serial-provisioning-ready"}));
    let retry = fixture.outgoing.recv_timeout(Duration::from_millis(350)).unwrap();
    assert_eq!(retry, first);
    assert_eq!(fixture.public.lock().unwrap()["ready"], false, "a startup hint cannot identify a device");
    fixture.send(json!({"status":"serial-provisioning-ready"}));
    assert_eq!(
        fixture.outgoing.recv_timeout(Duration::from_millis(350)),
        Err(mpsc::RecvTimeoutError::Timeout),
        "the repeated hint must not trigger another early retry",
    );
    fixture.command(Action::Stop);
    fixture.handle.join().unwrap().unwrap();
}

#[test]
fn baud_reopen_rejects_a_reply_to_the_retired_attempt() {
    let (port, device, outgoing) = duplex();
    let fixture = start_worker_with_reopen(None, false, Some(port));
    let first = fixture.receive();
    let current = outgoing.recv_timeout(Duration::from_secs(4)).unwrap();
    assert_eq!(current["command"], "status");
    assert_ne!(current["requestId"], first["requestId"]);

    let status = |id: &Value, name: &str| json!({
        "protocolVersion":2,"deviceType":"esp32","statusEvents":true,
        "requestId":id,"companionId":"","status":"offline","setupSaved":false,"name":name,
    });
    send_frame(&device, status(&first["requestId"], "Retired attempt"));
    send_frame(&device, status(&current["requestId"], "Current attempt"));
    let watch = outgoing.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(watch["command"], "status-watch");
    fixture.wait_field("ready", json!(true));
    assert_eq!(fixture.public.lock().unwrap()["device"]["name"], "Current attempt");
    fixture.command(Action::Stop);
    fixture.handle.join().unwrap().unwrap();
}
