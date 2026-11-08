use super::*;

#[test]
fn usb_worker_routes_rejections_to_the_current_link_only() {
    let fixture = start_worker(None, false);
    let probe = fixture.receive();
    let mut status = json!({"protocolVersion":2,"deviceType":"esp32","statusEvents":true,
        "requestId":probe["requestId"],"companionId":"screen","status":"offline","setupSaved":false});
    fixture.send(status.clone());
    let watch = fixture.receive();
    assert_eq!(watch["command"], "status-watch");
    status["type"] = json!("status-event");
    status["requestId"] = watch["requestId"].clone();
    fixture.send(status);
    fixture.wait_field("ready", json!(true));
    fixture.send(json!({"type":"serial-open","linkId":"usb-first"}));
    fixture.send(json!({"type":"serial-close","linkId":"usb-first"}));
    fixture.send(json!({"type":"serial-open","linkId":"usb-second"}));
    fixture.send(json!({"type":"serial-close","linkId":"usb-first"}));
    fixture.send(
        json!({"type":"serial-request","linkId":"usb-second","requestId":"1",
        "frame":{"type":"auth","token":"not-a-valid-token"}}),
    );
    let reply = fixture.receive();
    assert_eq!(reply["command"], "serial-reply");
    assert_eq!(reply["linkId"], "usb-second");
    assert_eq!(reply["requestId"], "1");
    assert_eq!(reply["frame"]["type"], "error");
    let error = reply["frame"]["error"].clone();
    assert!(error.is_string());
    fixture.wait_field("lastHostError", error);
    fixture.send(json!({"status":"panel-load-failed"}));
    fixture.wait_field("deviceStatus", json!("panel-load-failed"));
    fixture.send(json!({"type":"serial-close","linkId":"usb-second"}));
    fixture.send(
        json!({"type":"serial-request","linkId":"usb-second","requestId":"2",
        "frame":{"type":"auth","token":"not-a-valid-token"}}),
    );
    assert_eq!(
        fixture.handle.join().unwrap().unwrap_err(),
        "Stale USB link."
    );
}
