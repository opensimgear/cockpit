use super::*;
use std::sync::mpsc;

#[test]
fn same_port_return_during_pending_config_change_keeps_original_worker() {
    exercise_reconciliation(true);
}

#[test]
fn completed_reconciliation_without_a_current_endpoint_retires_the_worker() {
    exercise_reconciliation(false);
}

fn exercise_reconciliation(port_returns: bool) {
    let (fleet, ports, created) = fixture(vec![port("COM8", false)], false);
    let module = connect(&fleet, "COM8");
    let key = &module["key"];
    let key_text = key.as_str().unwrap();
    let original_route = module["snapshot"]["route"].clone();
    let original = created.lock().unwrap()[0].clone();
    operation(&fleet, key, "module_usb_claim", json!({"route":original_route})).unwrap();

    let (entered, observed) = mpsc::sync_channel(1);
    let (resume, blocked) = mpsc::sync_channel(1);
    *original.hold_config_change.lock().unwrap() = Some((entered, blocked));
    let fleet = Arc::new(fleet);
    let background = fleet.clone();
    let worker_key = key.clone();
    let pending = std::thread::spawn(move || {
        operation(
            &background,
            &worker_key,
            "module_usb_config_apply",
            json!({"route":original_route,"config":{"name":"Recovered"},"save":true}),
        )
    });
    observed.recv_timeout(Duration::from_secs(1)).unwrap();

    ports.lock().unwrap().clear();
    fleet.scan().unwrap();
    let absent = fleet.snapshots().into_iter().find(|slot| slot.key == key_text).unwrap();
    assert!(!absent.present);
    assert!(!absent.snapshot.connected);
    assert!(absent.snapshot.route.is_none());
    assert!(fleet.input_capture(key_text).is_err());
    {
        let state = fleet.state.lock().unwrap();
        let slot = &state.slots[key_text];
        assert!(slot.service.is_some(), "the admitted write must retain its worker for readback");
        assert!(!slot.closing.load(Ordering::Acquire));
    }
    assert_eq!(created.lock().unwrap().len(), 1);

    if port_returns {
        ports.lock().unwrap().push(port("COM8", false));
        fleet.scan().unwrap();
        assert_eq!(created.lock().unwrap().len(), 1);
        let returning = &fleet.snapshots()[0];
        assert!(returning.present);
        assert!(!returning.admissible);
        assert!(returning.snapshot.route.is_none(), "a returned port cannot revive the old route");
    }
    resume.send(()).unwrap();
    let result = pending.join().unwrap();

    let mutations = original.calls.lock().unwrap().iter().filter(|call| call.as_str() == "apply-save").count();
    assert_eq!(mutations, 1, "the config mutation must execute exactly once");
    assert_eq!(created.lock().unwrap().len(), 1);
    if port_returns {
        let reply = result.unwrap();
        assert_eq!(reply["module"]["snapshot"]["config"]["name"], "Recovered");
        assert_eq!(reply["module"]["snapshot"]["route"]["connectionGeneration"], 2);
        assert!(!original.stopped.load(Ordering::Acquire));
    } else {
        assert!(result.unwrap_err().contains("USB endpoint changed"));
        wait(|| original.stopped.load(Ordering::Acquire));
        assert!(fleet.snapshots()[0].snapshot.route.is_none());
    }
}
