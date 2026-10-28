use super::*;
use osg_mapping::{
    engine::ControlAssignment,
    profile::{Profile, Value as InputValue},
};
use std::sync::atomic::AtomicUsize;
pub(super) fn setup() -> (
    Service,
    tempfile::TempDir,
    Profile,
    Vec<Assignment>,
    Arc<AtomicUsize>,
) {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let profile =
        Profile::parse(include_bytes!("../../mapping/fixtures/direct-profile.json")).unwrap();
    Store::new(root.path().into()).save(&profile).unwrap();
    let assignments = profile
        .roles
        .iter()
        .map(|r| Assignment {
            role: r.id.clone(),
            source: Source {
                origin: r.origin,
                id: r.id.clone(),
            },
            controls: r
                .controls
                .iter()
                .map(|c| ControlAssignment {
                    control: c.id.clone(),
                    source_control: c.id.clone(),
                })
                .collect(),
        })
        .collect();
    let calls = Arc::new(AtomicUsize::new(0));
    let inspected = calls.clone();
    let service = Service::new(
        Arc::new(ControllerService::new(None, false)),
        Some(root.path().into()),
        Arc::new(move || {
            inspected.fetch_add(1, Ordering::Relaxed);
            Err("vJoy missing in fixture".into())
        }),
    );
    (service, root, profile, assignments, calls)
}
#[test]
fn native_windows_readiness_matches_visibility_without_inspection_or_output() {
    let (service, _root, _profile, _assignments, calls) = setup();
    assert_eq!(LIVE_OUTPUT_QUALIFIED, osg_controller_input::visibility::capabilities::WINDOWS_NATIVE_OUTPUTS);
    assert_eq!(service.qualified, LIVE_OUTPUT_QUALIFIED);
    assert!(!SCRIPT_LIVE_QUALIFIED);
    assert!(!service.script_live_qualified);
    #[cfg(windows)]
    {
        assert_eq!(osg_controller_input::visibility::windows::QUALIFIED, LIVE_OUTPUT_QUALIFIED);
        assert!(!osg_virtual_controller::windows_launcher::QUALIFIED);
    }
    assert_eq!(service.request(&json!({"command":"mapping_status"})).unwrap()["running"], false);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    if LIVE_OUTPUT_QUALIFIED {
        assert_eq!(
            QUALIFICATION,
            osg_controller_input::visibility::capabilities::WINDOWS_REASON
        );
    }
    service.shutdown();
}
#[test]
fn stored_preview_runs_without_optional_drivers_and_keeps_edges() {
    let (mut service, root, profile, assignments, calls) = setup();
    service.qualified = false; // Preserve the explicit default-gate oracle in acceptance builds.
    assert_eq!(
        service
            .request(&json!({"command":"mapping_status"}))
            .unwrap()["running"],
        false
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    service.request(&json!({"command":"mapping_assignments_save","profileId":profile.id,"assignments":assignments})).unwrap();
    assert_eq!(
        service
            .request(&json!({"command":"mapping_assignments_get","profileId":profile.id}))
            .unwrap()["roles"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(service.request(&json!({"command":"mapping_assignments_get","profileId":profile.id,"role":"left","offset":0})).unwrap()["controls"].as_array().unwrap().len(),3);
    assert!(service.request(&json!({"command":"mapping_assignments_get","profileId":profile.id,"role":"left","offset":99})).is_err());
    let started = service
        .request(&json!({"command":"mapping_preview_start","profileId":profile.id}))
        .unwrap();
    for pressed in [true, false, true] {
        service.request(&json!({"command":"mapping_simulate","token":started["token"],"source":assignments[0].source,"changes":[{"control":"trigger","value":InputValue::Button(pressed)}]})).unwrap();
    }
    std::thread::sleep(Duration::from_millis(12));
    let recorded = service
        .request(&json!({"command":"mapping_recording","after":"0"}))
        .unwrap();
    assert!(recorded["records"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["report"]["state"]["buttons"][0] == true));
    let status = service
        .request(&json!({"command":"mapping_status"}))
        .unwrap();
    assert_eq!(status["running"], true);
    assert_eq!(status["ready"], true);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    let inspected = service
        .request(&json!({"command":"mapping_virtual_inspect"}))
        .unwrap();
    assert_eq!(inspected["qualified"], false);
    if cfg!(any(target_os = "linux", target_os = "macos")) {
        assert_eq!(inspected["backend"], BACKEND);
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    } else {
        assert_eq!(inspected["state"], "unsupported");
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
    for command in [
        "mapping_virtual_create",
        "mapping_virtual_start",
        "mapping_virtual_recover",
    ] {
        assert_eq!(service
            .request(&json!({"command":command}))
            .unwrap_err(), QUALIFICATION);
    }
    assert_eq!(
        service
            .request(&json!({"command":"mapping_status"}))
            .unwrap()["running"],
        true
    );
    service.request(&json!({"command":"mapping_stop"})).unwrap();
    service.shutdown();
    assert!(service
        .request(&json!({"command":"mapping_status"}))
        .is_err());
    assert_eq!(
        Store::new(root.path().into())
            .load_assignments(&profile.id)
            .unwrap()
            .len(),
        2
    );
}
#[test]
fn malformed_requests_and_unavailable_sources_do_not_replace_preview() {
    let (service, _root, p, assignments, _) = setup();
    for request in [
        json!(null),
        json!({}),
        json!({"command":"unknown"}),
        json!({"command":"mapping_recording","after":3}),
        json!({"command":"mapping_assignments_save","profileId":p.id,"assignments":[]}),
        json!({"command":"mapping_preview_start"}),
        json!({"command":"mapping_preview_start","profileId":p.id}),
        json!({"command":"mapping_simulate"}),
    ] {
        assert!(service.request(&request).is_err());
    }
    let huge = json!({"command":"mapping_status","extra":"x".repeat(65_536)});
    assert!(service.request(&huge).is_err());
    service.request(&json!({"command":"mapping_assignments_save","profileId":p.id,"assignments":assignments})).unwrap();
    assert!(service
        .request(&json!({"command":"mapping_assignments_get","profileId":p.id,"role":"missing"}))
        .is_err());
    let started = service
        .request(&json!({"command":"mapping_preview_start","profileId":p.id}))
        .unwrap();
    let too_many = vec![json!({"control":"trigger","value":{"kind":"button","value":true}}); 257];
    assert!(service.request(&json!({"command":"mapping_simulate","token":started["token"],"source":assignments[0].source,"changes":too_many})).is_err());
    let mut physical = p.clone();
    physical.roles[0].origin = Origin::HostController;
    let mut a = assignments.clone();
    a[0].source.origin = Origin::HostController;
    let store = service.store().unwrap();
    store.save(&physical).unwrap();
    store.save_assignments(&p.id, &a).unwrap();
    assert!(service
        .request(&json!({"command":"mapping_preview_start","profileId":p.id}))
        .is_err());
    assert_eq!(
        service
            .request(&json!({"command":"mapping_status"}))
            .unwrap()["token"],
        started["token"]
    );
    assert!(Service::handles("mapping_stop"));
    assert!(!Service::handles("mapping_profiles"));
    let no_store = Service::new(
        Arc::new(ControllerService::new(None, false)),
        None,
        Arc::new(|| Ok(vec![])),
    );
    assert!(no_store
        .request(&json!({"command":"mapping_preview_start","profileId":p.id}))
        .is_err());
    assert_eq!(
        no_store
            .request(&json!({"command":"mapping_virtual_inspect"}))
            .unwrap()["slots"],
        json!([])
    );
}
#[test]
fn enabling_requires_explicit_inspection_and_target_settings_do_not_activate() {
    let (service, _root, profile, _, calls) = setup();
    let enabled = service.request(&json!({"command":"mapping_virtual_enable","enabled":true}));
    assert_eq!(
        enabled.is_ok(),
        cfg!(any(target_os = "linux", target_os = "macos"))
    );
    assert_eq!(
        calls.load(Ordering::Relaxed),
        usize::from(!cfg!(any(target_os = "linux", target_os = "macos")))
    );
    assert!(service
        .request(&json!({"command":"mapping_virtual_enable","enabled":"yes"}))
        .is_err());
    service
        .request(&json!({"command":"mapping_virtual_enable","enabled":false}))
        .unwrap();
    let bindings: Vec<osg_virtual_controller::vjoy::Binding> = profile
        .targets
        .iter()
        .enumerate()
        .map(|(i, t)| osg_virtual_controller::vjoy::Binding {
            target: t.id.clone(),
            slot: (i + 1) as u8,
            axes: vec![0x30; t.layout.axes.len()],
            layout: t.layout.clone(),
        })
        .collect();
    service
        .request(
            &json!({"command":"mapping_targets_save","profileId":profile.id,"bindings":bindings}),
        )
        .unwrap();
    assert_eq!(
        service
            .request(&json!({"command":"mapping_targets_get","profileId":profile.id}))
            .unwrap()["bindings"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        service
            .request(&json!({"command":"mapping_status"}))
            .unwrap()["running"],
        false
    );
    let inspected = Service::new(
        Arc::new(ControllerService::new(None, false)),
        None,
        Arc::new(|| Ok(vec![])),
    );
    inspected
        .request(&json!({"command":"mapping_virtual_enable","enabled":true}))
        .unwrap();
    assert_eq!(
        inspected
            .request(&json!({"command":"mapping_status"}))
            .unwrap()["virtualDevices"]["enabled"],
        true
    );
    assert!(inspected
        .request(&json!({"command":"mapping_virtual_start"}))
        .is_err());
}

#[test]
fn stop_cancels_a_start_waiting_for_its_profile_without_reviving_after_reply() {
    use std::sync::mpsc;
    let (service, _root, p, assignments, _) = setup();
    service.request(&json!({"command":"mapping_assignments_save","profileId":p.id,"assignments":assignments})).unwrap();
    service
        .request(&json!({"command":"mapping_preview_start","profileId":p.id}))
        .unwrap();
    let (entered, started) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    *service.before_start.lock().unwrap() = Some(Box::new(move || {
        entered.send(()).unwrap();
        wait.recv_timeout(Duration::from_secs(3)).unwrap();
    }));
    std::thread::scope(|scope| {
        let pending = scope.spawn(|| {
            service.request(&json!({"command":"mapping_preview_start","profileId":p.id}))
        });
        started.recv_timeout(Duration::from_secs(3)).unwrap();
        service.request(&json!({"command":"mapping_stop"})).unwrap();
        release.send(()).unwrap();
        assert!(pending.join().unwrap().unwrap_err().contains("superseded"));
    });
    assert_eq!(
        service
            .request(&json!({"command":"mapping_status"}))
            .unwrap()["running"],
        false
    );
    assert!(service.request(&json!({"command":"mapping_assignments_get","profileId":p.id,"role":"left","offset":"0"})).is_err());
}

#[test]
#[cfg(target_os = "linux")]
fn linux_templates_persist_and_limits_are_checked_without_native_creation() {
    let (service, root, mut profile, _, calls) = setup();
    let bindings: Vec<osg_virtual_controller::vjoy::Binding> = profile
        .targets
        .iter()
        .enumerate()
        .map(|(i, t)| osg_virtual_controller::vjoy::Binding {
            target: t.id.clone(),
            slot: i as u8 + 1,
            axes: t
                .layout
                .axes
                .iter()
                .enumerate()
                .map(|(i, _)| 0x30 + i as u32)
                .collect(),
            layout: t.layout.clone(),
        })
        .collect();
    service
        .request(
            &json!({"command":"mapping_targets_save","profileId":profile.id,"bindings":bindings}),
        )
        .unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    let restored = crate::settings::Settings::open(Some(root.path().join("local-settings.json")));
    assert_eq!(restored.value.targets[&profile.id].len(), 2);
    assert_eq!(
        service
            .request(&json!({"command":"mapping_status"}))
            .unwrap()["virtualDevices"]["backend"],
        "uinput"
    );
    assert_eq!(
        service
            .request(&json!({"command":"mapping_virtual_inspect"}))
            .unwrap()["limits"]["buttons"],
        32
    );
    profile.targets[0].layout.buttons = 33;
    service.store().unwrap().save(&profile).unwrap();
    let mut invalid = bindings;
    invalid[0].layout = profile.targets[0].layout.clone();
    assert!(service
        .request(
            &json!({"command":"mapping_targets_save","profileId":profile.id,"bindings":invalid})
        )
        .is_err());
    assert!(service
        .request(&json!({"command":"mapping_virtual_start","profileId":profile.id}))
        .is_err());
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[test]
fn bounded_inspector_does_not_change_run_or_activate_learning() {
    let (service, _root, p, assignments, _) = setup();
    assert!(service
        .request(&json!({"command":"mapping_status","inspect":true}))
        .unwrap()["inspector"]
        .is_null());
    service.request(&json!({"command":"mapping_assignments_save","profileId":p.id,"assignments":assignments})).unwrap();
    assert!(service
        .request(&json!({"command":"mapping_learn_start"}))
        .is_err());
    assert!(service
        .request(&json!({"command":"mapping_learn_start","sourceId":"missing"}))
        .is_err());
    let started = service
        .request(&json!({"command":"mapping_preview_start","profileId":p.id}))
        .unwrap();
    let view = service
        .request(&json!({"command":"mapping_status","inspect":true,"offset":0}))
        .unwrap();
    assert_eq!(view["token"], started["token"]);
    assert!(view["inspector"]["nodes"].as_array().unwrap().len() <= 32);
    assert!(view["learning"].is_null());
    assert!(service
        .request(&json!({"command":"mapping_status","inspect":true,"offset":257}))
        .is_err());
    service.request(&json!({"command":"mapping_stop"})).unwrap();
    let stopped = service
        .request(&json!({"command":"mapping_status","inspect":true}))
        .unwrap();
    assert_eq!(stopped["running"], false);
    assert_eq!(stopped["inspector"]["total"], view["inspector"]["total"]);
}
