use super::*;
use crate::system_inventory::{Snapshot, TOPIC, VJOY_DEMAND};
use std::sync::mpsc;

fn snapshot(at: u64) -> Value {
    json!({"observedAt":at,"hiddenDevices":[],"controllerAssociations":[{"controllerId":"kept"}],
        "diagnostics":["Other provider evidence"],"drivers":{"vjoy":{"installed":true}},
        "virtualDevices":[{"id":"vjoy:3","slot":3,"backend":"vjoy","status":"free","available":true,
            "ownerPid":null,"owner":null,"configured":true,"buttons":12}]})
}

fn owner(pid: u32) -> SlotStatus {
    SlotStatus { id: 3, status: Status::Busy, owner_pid: Some(pid) }
}

#[test]
fn ownership_changes_only_the_vjoy_fields_and_rejects_a_newer_inventory_revision() {
    let runtime = Runtime::new(true);
    let observations = crate::observations::Runtime::new();
    runtime.observe(&observations).unwrap();
    let _demand = observations.broker().subscribe("visible-devices", "ui", VJOY_DEMAND).unwrap();
    let mut inventory = observations.broker().subscribe("passive-inventory", "mcp", TOPIC).unwrap();
    inventory.try_recv().unwrap().unwrap();
    runtime.commit(snapshot(1));
    assert_eq!(inventory.try_recv().unwrap().unwrap().revision, "2");

    runtime.commit_vjoy_sample(1, Some(&[owner(83)]));
    let value = runtime.snapshot.lock().unwrap().as_ref().unwrap().value.clone();
    let revision = value["observedAt"].as_u64().unwrap();
    let mut expected = snapshot(revision);
    expected["virtualDevices"][0]["status"] = json!("busy");
    expected["virtualDevices"][0]["ownerPid"] = json!(83);
    expected["virtualDevices"][0]["owner"] = json!("Process 83");
    assert_eq!(value, expected);
    assert_eq!(inventory.try_recv().unwrap().unwrap().revision, "3");

    runtime.commit_vjoy_sample(revision, Some(&[owner(83)]));
    runtime.commit_vjoy_sample(1, Some(&[owner(91)]));
    assert!(inventory.try_recv().unwrap().is_none());
    runtime.commit_vjoy_sample(revision, None);
    let value = runtime.snapshot.lock().unwrap().as_ref().unwrap().value.clone();
    assert_eq!(value["virtualDevices"][0]["status"], "unknown");
    assert!(value["virtualDevices"][0]["ownerPid"].is_null());
    assert_eq!(value["virtualDevices"][0]["buttons"], 12);
    assert_eq!(inventory.try_recv().unwrap().unwrap().revision, "4");
}

#[test]
fn one_visible_reader_owner_shares_queries_and_stops_after_the_last_reader() {
    struct Reader(mpsc::Sender<&'static str>);
    impl Drop for Reader {
        fn drop(&mut self) { let _ = self.0.send("closed"); }
    }

    let runtime = Arc::new(Runtime::new(true));
    let observations = crate::observations::Runtime::new();
    runtime.observe(&observations).unwrap();
    *runtime.snapshot.lock().unwrap() = Some(Snapshot { value: snapshot(1) });
    let _passive = observations.broker().subscribe("passive-inventory", "mcp", TOPIC).unwrap();
    assert!(observations.broker().subscribe("mcp-demand", "mcp", VJOY_DEMAND).is_err());
    let (send, events) = mpsc::channel();
    runtime.start_vjoy_status_with(move || {
        send.send("opened").unwrap();
        Ok(Reader(send.clone()))
    }, |reader| {
        reader.0.send("queried").unwrap();
        Ok(vec![owner(83)])
    }).unwrap();

    let expect = |event| assert_eq!(events.recv_timeout(Duration::from_secs(2)).unwrap(), event);
    assert!(events.recv_timeout(Duration::from_millis(100)).is_err());
    let ui = observations.subscribe("visible-ui", "ui", VJOY_DEMAND, |_| Ok(())).unwrap();
    runtime.vjoy_reader_changed();
    expect("opened");
    expect("queried");

    let browser = observations.subscribe("visible-browser", "browser", VJOY_DEMAND, |_| Ok(())).unwrap();
    runtime.vjoy_reader_changed();
    observations.unsubscribe("visible-ui", &ui).unwrap();
    expect("queried");
    observations.unsubscribe("visible-browser", &browser).unwrap();
    expect("closed");
    assert!(events.recv_timeout(SAMPLE_PERIOD + Duration::from_millis(50)).is_err());

    let reopened = observations.subscribe("visible-ui", "ui", VJOY_DEMAND, |_| Ok(())).unwrap();
    runtime.vjoy_reader_changed();
    expect("opened");
    expect("queried");
    observations.unsubscribe("visible-ui", &reopened).unwrap();
    expect("closed");
    runtime.shutdown();
}
