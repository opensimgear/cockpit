use super::*;

fn operation(open: &Value, phase: &str) -> Value {
    json!({"instanceId":"instance","publicationHash":open["publicationHash"],
        "sessionId":open["sessionId"],"generation":open["generation"],"attemptId":1,
        "phase":phase,"completedBytes":32,"totalBytes":64,"cacheHits":1,"cacheMisses":1})
}

#[test]
fn operation_progress_is_metadata_and_never_arms_the_source() {
    let sink = Arc::new(Sink::default());
    let hub = Hub::new("host".into(), sink.clone());
    let secret = paired(&hub,"screen"); let publication = publish(&hub,fixture());
    assign(&hub,"screen","instance",&publication.publication_hash);
    let open = connect(&hub,"connection",&secret,"instance");
    let mut heartbeat = frame(&open,"heartbeat","1");
    heartbeat["operation"] = operation(&open,"ready");
    assert_eq!(wire(&hub,"connection",heartbeat)["ok"],true);
    let status = operator(&hub,json!({"command":"companion_status"}));
    assert_eq!(status["instances"][0]["operation"]["phase"],"ready");
    assert_eq!(status["instances"][0]["armed"],false);
    assert!(sink.owners.lock().unwrap().is_empty());
    let mut preparing=frame(&open,"heartbeat","2"); preparing["operation"]=operation(&open,"checking-cache");
    assert_eq!(wire(&hub,"connection",preparing)["ok"],true);
    assert!(!hub.serial_panel_preparing("screen","connection"),"A network session cannot extend any USB worker's watchdog");
    assert_eq!(wire(&hub,"connection",input(&open,&publication,"1"))["ok"],false);
    assert_eq!(baseline(&hub,"connection",&open,&publication)["ok"],true);
}

#[test]
fn stale_or_invalid_operation_cannot_arm_or_replace_current_diagnostics() {
    let sink = Arc::new(Sink::default()); let hub = Hub::new("host".into(),sink.clone());
    let secret = paired(&hub,"screen"); let publication = publish(&hub,fixture());
    assign(&hub,"screen","instance",&publication.publication_hash);
    let open = connect(&hub,"connection",&secret,"instance");
    let mut heartbeat = frame(&open,"heartbeat","1"); heartbeat["operation"] = operation(&open,"downloading");
    assert_eq!(wire(&hub,"connection",heartbeat.clone())["ok"],true);
    heartbeat["sequence"] = json!("2"); heartbeat["operation"]["sessionId"] = json!("old-session");
    assert_eq!(wire(&hub,"connection",heartbeat.clone())["ok"],false);
    heartbeat["operation"] = operation(&open,"failed");
    heartbeat["operation"]["errorCode"] = json!("cache_full"); heartbeat["operation"]["retryable"] = json!(false);
    heartbeat["operation"]["message"] = json!("The active panel and incoming images do not fit in flash");
    heartbeat["panelError"] = json!(true);
    assert_eq!(wire(&hub,"connection",heartbeat)["ok"],true);
    let status = operator(&hub,json!({"command":"companion_status"}));
    assert_eq!(status["clients"][0]["panelError"],"The active panel and incoming images do not fit in flash");
    assert_eq!(hub.panel_operation("instance",&publication.publication_hash).unwrap()["errorCode"],"cache_full");
    assert_eq!(hub.record_serial_panel_operation("screen",open["sessionId"].as_str().unwrap(),"1",&operation(&open,"ready")),Err("Serial panel operation identity changed.".into()));
    let other = publish(&hub,mutated(|d|d["ui"]["title"]=json!("Other")));
    assign(&hub,"screen","instance",&other.publication_hash);
    assert!(hub.panel_operation("instance",&publication.publication_hash).is_none());
    assert!(hub.panel_operation("instance",&other.publication_hash).is_none());
    assert!(sink.owners.lock().unwrap().is_empty());
}

#[test]
fn malformed_baseline_diagnostics_are_rejected_before_source_acceptance() {
    let sink = Arc::new(Sink::default()); let hub = Hub::new("host".into(),sink.clone());
    let secret = paired(&hub,"screen"); let publication = publish(&hub,fixture());
    assign(&hub,"screen","instance",&publication.publication_hash);
    let open = connect(&hub,"connection",&secret,"instance");
    let mut baseline = frame(&open,"baseline","0");
    baseline["values"] = serde_json::to_value(publication.controls.iter().map(|c|(c.id.clone(),c.fallback())).collect::<BTreeMap<_,_>>()).unwrap();
    baseline["operation"] = operation(&open,"ready"); baseline["operation"]["instanceId"] = json!("other");
    assert_eq!(wire(&hub,"connection",baseline)["ok"],false);
    assert!(sink.owners.lock().unwrap().is_empty());
}

#[test]
fn serial_operation_reports_need_current_session_and_companion_identity() {
    let hub = Hub::new("host".into(),Arc::new(NullSourceSink));
    let secret = paired(&hub,"screen"); let publication = publish(&hub,fixture());
    assign(&hub,"screen","instance",&publication.publication_hash);
    assert_eq!(hub.handle_serial_wire(json!({"type":"auth","token":secret}),"usb",hub.now_ms())["ok"],true);
    let open = hub.handle_serial_wire(json!({"type":"open","instanceId":"instance"}),"usb",hub.now_ms());
    let report = operation(&open,"rendering"); let session = open["sessionId"].as_str().unwrap();
    assert!(hub.record_serial_panel_operation("other",session,"1",&report).is_err());
    assert!(hub.record_serial_panel_operation("screen",session,"2",&report).is_err());
    assert!(hub.record_serial_panel_operation("screen",session,"1",&report).is_ok());
    assert_eq!(hub.panel_operation("instance",&publication.publication_hash).unwrap()["phase"],"rendering");
    assert!(hub.serial_panel_preparing("screen","usb"));
    assert!(!hub.serial_panel_preparing("screen","other-usb"));
    assert!(!hub.serial_panel_preparing("other-screen","usb"));
    assert_eq!(operator(&hub,json!({"command":"companion_status"}))["instances"][0]["armed"],false);
    let mut close = frame(&open,"close","0"); close.as_object_mut().unwrap().remove("sequence");
    let closed = hub.handle_serial_wire(close,"usb",hub.now_ms());
    assert_eq!(closed["ok"],true);
    let replacement = hub.handle_serial_wire(json!({"type":"open","instanceId":"instance"}),"usb",hub.now_ms());
    assert_eq!(replacement["ok"],true);
    assert_ne!(replacement["sessionId"],open["sessionId"]);
    assert!(hub.record_serial_panel_operation("screen",session,"1",&report).is_err());
}

#[test]
fn disarmed_cache_preparation_has_a_bounded_lease_without_extending_armed_inputs() {
    for arm in [false,true] {
        let sink = Arc::new(Sink::default()); let hub = Hub::new("host".into(),sink.clone());
        let secret = paired(&hub,"screen"); let publication = publish(&hub,fixture());
        assign(&hub,"screen","instance",&publication.publication_hash);
        assert_eq!(hub.handle_serial_wire(json!({"type":"auth","token":secret}),"connection",hub.now_ms())["ok"],true);
        let open=hub.handle_serial_wire(json!({"type":"open","instanceId":"instance"}),"connection",hub.now_ms());
        assert_eq!(open["ok"],true); let now=hub.now_ms();
        let mut heartbeat = frame(&open,"heartbeat","1"); heartbeat["operation"] = operation(&open,"checking-cache");
        assert_eq!(hub.handle_wire(heartbeat,"connection",now)["ok"],true);
        assert!(hub.serial_panel_preparing("screen","connection"));
        assert!(!hub.serial_panel_preparing("other","connection"));
        hub.tick(now+2000);
        assert!(hub.panel_operation("instance",&publication.publication_hash).is_some(),"Flash work may take longer than an armed input lease");
        assert!(sink.owners.lock().unwrap().is_empty());
        if arm {
            assert_eq!(baseline(&hub,"connection",&open,&publication)["ok"],true);
            assert!(!hub.serial_panel_preparing("screen","connection"),"Armed input cannot extend the USB preparation watchdog");
            hub.tick(now+2000+crate::hub::LEASE_MS);
            assert!(hub.panel_operation("instance",&publication.publication_hash).is_none());
            assert!(sink.owners.lock().unwrap().is_empty(),"Armed input must retain its ordinary 1500ms lease");
        } else {
            hub.tick(now+crate::hub::PANEL_PREPARATION_LEASE_MS);
            assert!(hub.panel_operation("instance",&publication.publication_hash).is_none(),"Stalled flash preparation must expire after 30s");
            assert!(!hub.serial_panel_preparing("screen","connection"));
        }
    }
}

#[test]
fn preparation_heartbeats_preserve_the_initial_rendered_panel_feedback_snapshot() {
    let hub = Hub::new("host".into(),Arc::new(NullSourceSink));
    let secret = paired(&hub,"screen"); let publication = publish(&hub,fixture());
    assign(&hub,"screen","instance",&publication.publication_hash);
    let open = connect(&hub,"connection",&secret,"instance");
    // The final cache phase also covers display-task font/resource preflight.
    for (sequence,phase) in [("1","checking-cache"),("2","downloading"),("3","checking-cache")] {
        let mut heartbeat=frame(&open,"heartbeat",sequence);
        heartbeat["operation"]=operation(&open,phase);
        let ack=wire(&hub,"connection",heartbeat);
        assert_eq!(ack["ok"],true);
        assert!(ack["feedback"].is_null(),"Preparation ACKs cannot consume the initial snapshot");
    }
    let mut heartbeat=frame(&open,"heartbeat","4");
    heartbeat["operation"]=operation(&open,"rendering");
    let ack=wire(&hub,"connection",heartbeat);
    assert_eq!(ack["ok"],true);
    assert_eq!(ack["feedback"]["mode"],"snapshot");
    assert_eq!(baseline(&hub,"connection",&open,&publication)["ok"],true);
}
