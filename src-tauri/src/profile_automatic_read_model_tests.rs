use super::*;

#[test]
fn emergency_control_does_not_wait_for_a_settings_lock_and_captures_preferences() {
    let runtime = Arc::new(Runtime::new(None));
    let captures = Arc::new(Mutex::new(Vec::new()));
    let output = captures.clone();
    runtime.observe_read_model(Arc::new(move |change| output.lock().unwrap().push(change)));
    let state = runtime.state.lock().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let emergency = runtime.clone();
    let worker = std::thread::spawn(move || { emergency.control("profile_stop"); send.send(()).unwrap(); });
    // Simulates an arbitrarily slow settings save holding its ordinary mutex.
    receive.recv_timeout(std::time::Duration::from_secs(1)).expect("Stop waited on settings");
    assert!(runtime.emergency.load(Ordering::Acquire));
    drop(state);
    worker.join().unwrap();
    assert!(captures.lock().unwrap().iter().any(|change| matches!(change,
        crate::profile_read_models::AutomaticChange::Emergency { enabled: true, .. })));
    runtime.change(|settings| { settings.preferred_profiles.insert("dcs".into(), osg_profiles::DEFAULT_PROFILE_ID.into()); Ok(()) }).unwrap();
    assert!(captures.lock().unwrap().iter().any(|change| matches!(change,
        crate::profile_read_models::AutomaticChange::Snapshot(snapshot) if snapshot.preferred_profiles.contains_key("dcs"))));
    let count = captures.lock().unwrap().len();
    assert!(runtime.change(|_| Err("Rejected".into())).is_err());
    assert_eq!(captures.lock().unwrap().len(), count);
}
