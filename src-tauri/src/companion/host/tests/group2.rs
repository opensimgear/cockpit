use super::*;
    #[test]
    fn uploads_are_bounded_ordered_and_owned_without_initializing_the_listener() {
        let runtime = runtime();
        assert!(call(&runtime, "companion_publish_begin", json!({"size":0})).is_err());
        assert!(call(
            &runtime,
            "companion_publish_begin",
            json!({"size":MAX_UPLOAD+1})
        )
        .is_err());
        let id = call(&runtime, "companion_publish_begin", json!({"size":3})).unwrap()["uploadId"]
            .clone();
        assert!(call(&runtime, "companion_publish_begin", json!({"size":2})).is_err());
        assert!(runtime.request(&json!({"command":"companion_publish_chunk","clientId":"operator-two","companion":{"uploadId":id,"offset":0,"bytes":[1]}})).is_err());
        assert!(call(
            &runtime,
            "companion_publish_chunk",
            json!({"uploadId":id,"offset":1,"bytes":[1]})
        )
        .is_err());
        assert!(call(
            &runtime,
            "companion_publish_chunk",
            json!({"uploadId":id,"offset":0,"bytes":[999]})
        )
        .is_err());
        assert!(call(
            &runtime,
            "companion_publish_chunk",
            json!({"uploadId":id,"offset":0,"bytes":[1,2,3,4]})
        )
        .is_err());
        assert!(call(&runtime, "companion_publish_commit", json!({"uploadId":id})).is_err());
        call(
            &runtime,
            "companion_publish_chunk",
            json!({"uploadId":id,"offset":0,"bytes":[1,2,3]}),
        )
        .unwrap();
        assert!(call(&runtime, "companion_publish_commit", json!({"uploadId":id})).is_err());
        assert!(runtime.services.get().is_none());
        runtime.shutdown();
    }
    #[test]
    fn upload_abort_and_expiry_preserve_other_operator_ownership() {
        let runtime = runtime();
        let id = call(&runtime, "companion_publish_begin", json!({"size":1})).unwrap()["uploadId"]
            .clone();
        runtime.request(&json!({"command":"companion_publish_abort","clientId":"other","companion":{"uploadId":id}})).unwrap();
        assert!(call(&runtime, "companion_publish_begin", json!({"size":1})).is_err());
        call(&runtime, "companion_publish_abort", json!({"uploadId":id})).unwrap();
        let id = call(&runtime, "companion_publish_begin", json!({"size":1})).unwrap()["uploadId"]
            .clone();
        runtime.upload.lock().unwrap().as_mut().unwrap().started =
            Instant::now() - Duration::from_secs(61);
        assert!(call(
            &runtime,
            "companion_publish_chunk",
            json!({"uploadId":id,"offset":0,"bytes":[1]})
        )
        .unwrap_err()
        .contains("expired"));
        assert!(runtime.upload.lock().unwrap().is_none());
        assert!(runtime.request(&json!({})).is_err());
        assert!(runtime
            .request(&json!({"command":"companion_status","companion":false}))
            .is_err());
        assert!(runtime.services.get().is_none());
    }
    #[test]
    fn new_esp32_pairing_is_rejected_without_starting_services() {
        let runtime = runtime();
        for (command, payload) in [
            ("companion_pair_offer", json!({"kind":"esp32"})),
            ("companion_usb_provision", json!({})),
            ("companion_usb_provision", json!({"companionId":""})),
            ("companion_usb_provision", json!({"companionId":"  "})),
            ("companion_usb_provision", json!({"companionId":null})),
            ("companion_usb_provision", json!({"companionId":123})),
        ] {
            assert_eq!(call(&runtime, command, payload).unwrap_err(), "ESP32 pairing is temporarily unavailable.");
            assert!(runtime.services.get().is_none());
            assert!(runtime.metadata.get().is_none());
        }
    }
    #[cfg(windows)]
    #[test]
    fn usb_operator_requests_require_a_current_selection_and_ready_device() {
        let runtime = runtime();
        let before = call(&runtime, "companion_status", json!({})).unwrap();
        assert!(call(&runtime,"companion_usb_flash_cancel",json!({})).unwrap_err().contains("no update is waiting"));
        // With no paired clients this only enumerates ports; it cannot probe a UART.
        let ports = call(&runtime, "companion_usb_ports", json!({})).unwrap();
        assert!(ports["usbPorts"].is_array());
        assert_eq!(ports["firmwareProfiles"].as_array().unwrap().len(), 2);
        assert_eq!(ports["usb"]["connected"], false);
        for (payload, expected) in [
            (json!({}), "Choose a paired companion."),
            (json!({"companionId":"unpaired"}), "Choose a paired ESP32 companion."),
        ] {
            assert_eq!(call(&runtime, "companion_usb_update_prepare", payload).unwrap_err(), expected);
        }
        assert_eq!(
            call(&runtime, "companion_usb_flash", json!({"profile":"unknown"})).unwrap_err(),
            "Choose a supported board profile."
        );
        for profile in ["waveshare-lcd7-s3", "sunton-3248s035r"] {
            assert_eq!(
                call(&runtime, "companion_usb_flash", json!({"profile":profile})).unwrap_err(),
                "Choose a USB device."
            );
            // A path which was never enumerated cannot reach firmware installation,
            // including the update path with an unverified companion identity.
            let absent = format!("absent-test-port-{}", uuid::Uuid::new_v4());
            for companion in [Value::Null, json!("unpaired")] {
                assert!(call(&runtime, "companion_usb_flash", json!({
                    "profile":profile,"port":absent,"companionId":companion
                })).unwrap_err().contains("USB selection changed"));
            }
        }
        assert_eq!(
            call(&runtime, "companion_usb_connect", json!({})).unwrap_err(),
            "Select a USB device."
        );
        for command in [
            "companion_usb_scan", "companion_usb_arm", "companion_usb_disarm",
            "companion_usb_forget", "companion_usb_reconnect",
        ] {
            assert_eq!(call(&runtime, command, json!({})).unwrap_err(), "Connect a USB device first.");
        }
        assert_eq!(
            call(&runtime, "companion_usb_provision", json!({
                "companionId":"saved-screen","name":"Never provisioned","mode":"serial"
            })).unwrap_err(),
            "Wait for the USB device to be ready."
        );
        let disconnected = call(&runtime, "companion_usb_disconnect", json!({})).unwrap();
        assert_eq!(disconnected["usb"]["connected"], false);
        assert_eq!(call(&runtime, "companion_usb_status", json!({})).unwrap(), disconnected);
        let after = call(&runtime, "companion_status", json!({})).unwrap();
        for key in ["clients", "instances", "publications"] {
            assert_eq!(after["host"][key], before["host"][key]);
        }
        assert_eq!(after["listener"]["enabled"], false);
        assert_eq!(after["usb"]["devices"], json!([]));
        runtime.shutdown();
    }
    #[cfg(windows)]
    #[test]
    fn listener_configuration_restores_after_restart_and_disable_removes_it() {
        let directory=std::env::temp_dir().join(format!("osg-browser-listener-test-{}",uuid::Uuid::new_v4()));
        let make=|| {
            let mut runtime=runtime();runtime.path=Some(directory.clone());runtime
        };
        let first=make();
        let reservation=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port=reservation.local_addr().unwrap().port();drop(reservation);
        call(&first,"companion_enable",json!({"address":"127.0.0.1","advertisedHost":"127.0.0.1","port":port,"discovery":false})).unwrap();
        assert!(crate::companion_settings::Listener::open(&directory).unwrap().snapshot().1.is_some());
        first.shutdown();drop(first);
        let blocker=std::net::TcpListener::bind(("127.0.0.1",port)).unwrap();
        let second=make();
        let waiting=call(&second,"companion_status",json!({})).unwrap();
        assert_eq!(waiting["listener"]["enabled"],false);
        assert_eq!(waiting["listener"]["configuredEnabled"],true);
        assert_eq!(waiting["listener"]["config"]["discovery"],false);
        drop(blocker);
        Runtime::start_saved_listener(second.services().unwrap());
        assert_eq!(call(&second,"companion_status",json!({})).unwrap()["listener"]["enabled"],true);
        call(&second,"companion_disable",json!({})).unwrap();
        assert!(crate::companion_settings::Listener::open(&directory).unwrap().snapshot().1.is_none());
        second.shutdown();drop(second);
        let third=make();
        let disabled=call(&third,"companion_status",json!({})).unwrap();
        assert_eq!(disabled["listener"]["enabled"],false);
        assert_eq!(disabled["listener"]["configuredEnabled"],false);
        third.shutdown();drop(third);
        std::fs::remove_dir_all(directory).unwrap();
    }
