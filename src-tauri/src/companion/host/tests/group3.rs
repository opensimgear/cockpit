use super::*;
    #[cfg(windows)]
    #[test]
    fn native_operator_status_pairing_publication_and_assignment_remain_separate_from_output() {
        let runtime = runtime();
        let initial = call(&runtime, "companion_status", json!({})).unwrap();
        assert_eq!(initial["listener"]["enabled"], false);
        assert!(initial["interfaces"].is_array());
        assert!(call(
            &runtime,
            "companion_enable",
            json!({"address":"invalid","port":54437,"advertisedHost":"127.0.0.1"})
        )
        .is_err());
        assert!(call(
            &runtime,
            "companion_enable",
            json!({"address":"127.0.0.1","port":1,"advertisedHost":"127.0.0.1"})
        )
        .is_err());
        assert!(call(&runtime, "companion_pair_offer", json!({})).is_err());
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = occupied.local_addr().unwrap().port();
        let config = json!({"address":"127.0.0.1","port":port,"advertisedHost":"127.0.0.1","discovery":false});
        let waiting=call(&runtime,"companion_enable",config.clone()).unwrap();
        assert_eq!(waiting["listener"]["configuredEnabled"],true);
        assert_eq!(waiting["listener"]["enabled"],false);
        assert!(waiting["listener"]["lastError"].as_str().unwrap().contains("Cannot bind"));
        drop(occupied);
        assert_eq!(
            call(&runtime, "companion_enable", config.clone()).unwrap()["listener"]["enabled"],
            true
        );
        assert!(call(&runtime, "companion_enable", config).is_err());
        if !BROWSER_ASSETS.is_empty() {
            let browser = call(&runtime, "companion_pair_offer", json!({"kind":"browser"})).unwrap();
            assert!(browser["pairing"]["browserUrl"].as_str().unwrap().starts_with(&format!("https://127.0.0.1:{port}/#pair=")));
            assert_eq!(browser["pairing"]["ttlMs"],60_000);
        } else {
            assert!(call(&runtime,"companion_pair_offer",json!({"kind":"browser"})).unwrap_err().contains("Build the browser"));
        }
        assert!(call(&runtime,"companion_browser_certificate",json!({})).unwrap()["certificatePem"].as_str().unwrap().starts_with("-----BEGIN CERTIFICATE-----"));
        let offer = call(&runtime, "companion_pair_offer", json!({})).unwrap();
        let rows = offer["pairing"]["qrMatrix"].as_array().unwrap();
        assert!((21..=177).contains(&rows.len()));
        let original = qrcode_matrix(offer["pairing"]["manualPayload"].as_str().unwrap()).unwrap();
        for (row, expected) in rows.iter().zip(original) {
            let bits = row.as_str().unwrap();
            assert_eq!(bits.len(), rows.len());
            assert!(bits.bytes().all(|bit| bit == b'0' || bit == b'1'));
            assert_eq!(
                bits.bytes().map(|bit| bit == b'1').collect::<Vec<_>>(),
                expected
            );
        }
        // Pairing must fit both the native acceptance frame and browser bridge,
        // without increasing either transport's bound to accommodate QR data.
        assert!(
            serde_json::to_vec(&json!({"ok":true,"state":{"companions":offer}}))
                .unwrap()
                .len()
                < 66_561
        );
        let pairing: Value =
            serde_json::from_str(offer["pairing"]["manualPayload"].as_str().unwrap()).unwrap();
        assert!(pairing["endpoint"]
            .as_str()
            .unwrap()
            .starts_with("wss://127.0.0.1:"));
        let services = runtime.services().unwrap();
        assert_eq!(call(&runtime, "companion_pair_offer", json!({"kind":"esp32"})).unwrap_err(), "ESP32 pairing is temporarily unavailable.");
        assert_eq!(call(&runtime, "companion_usb_provision", json!({"mode":"serial"})).unwrap_err(), "ESP32 pairing is temporarily unavailable.");
        // Rejected ESP32 setup must leave the active non-ESP32 offer usable.
        let pending=services.hub.handle_wire(json!({"type":"pair","version":1,"secret":pairing["secret"],"companionId":"test-installation","name":"Desktop panel"}),"root-test-peer",services.hub.now_ms());
        assert_eq!(pending["type"], "pair-pending");
        call(
            &runtime,
            "companion_approve",
            json!({"requestId":pending["requestId"],"companionId":"test-installation"}),
        )
        .unwrap();

        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../../crates/companion-service/tests/fixtures/compiled-toggle-publication.json"
        ))
        .unwrap();
        let upload = json!({"canonicalDocument":fixture["canonicalDocument"],"publicationHash":fixture["publicationHash"],"assets":fixture["assets"]});
        let bytes = serde_json::to_vec(&upload).unwrap();
        let upload_id = call(
            &runtime,
            "companion_publish_begin",
            json!({"size":bytes.len()}),
        )
        .unwrap()["uploadId"]
            .clone();
        for (index, chunk) in bytes.chunks(8192).enumerate() {
            call(
                &runtime,
                "companion_publish_chunk",
                json!({"uploadId":upload_id,"offset":index*8192,"bytes":chunk}),
            )
            .unwrap();
        }
        let published = call(
            &runtime,
            "companion_publish_commit",
            json!({"uploadId":upload_id}),
        )
        .unwrap();
        assert_eq!(
            published["host"]["publications"].as_array().unwrap().len(),
            1
        );
        let assigned=call(&runtime,"companion_assign",json!({"companionId":"test-installation","instanceId":"saved-panel","publicationHash":fixture["publicationHash"]})).unwrap();
        assert_eq!(assigned["host"]["instances"][0]["armed"], false);
        assert_eq!(runtime.hub().unwrap().host_id(), services.hub.host_id());
        assert!(runtime.resource_instance_ids().unwrap().contains("saved-panel"));
        assert_eq!(runtime.resource_companions().unwrap().len(), 1);
        assert_eq!(runtime.resource_publication("saved-panel").unwrap().publication_hash, fixture["publicationHash"]);
        assert!(runtime.profile_panel_operation("saved-panel", fixture["publicationHash"].as_str().unwrap()).is_none(),
            "An assignment without an opened session has no live operation");
        assert!(runtime.profile_panel_operation_history("saved-panel").is_none());
        assert!(runtime.profile_retry_panel("saved-panel", fixture["publicationHash"].as_str().unwrap()).is_ok());
        assert!(runtime.profile_retry_panel("saved-panel", "changed-publication").is_err());
        let legacy_screen = json!({"kind":"esp32","connected":true,"screen":{"width":480,"height":320}});
        assert!(validate_target_panel(&services.hub, "missing", &legacy_screen, &Value::Null).is_err());
        assert!(validate_target_panel(&services.hub, fixture["publicationHash"].as_str().unwrap(), &legacy_screen, &Value::Null).is_err(),
            "A published panel still needs to pass the target screen preflight");
        assert_eq!(
            runtime
                .assigned_publication("saved-panel")
                .unwrap()
                .publication_hash,
            fixture["publicationHash"].as_str().unwrap()
        );
        assert!(runtime.assigned_publication("unknown-instance").is_err());
        assert!(call(&runtime,"companion_game_capture",json!({"instanceIds":["saved-panel"]})).unwrap_err().contains("retired"));
        assert!(runtime.game_command("_game_companion_snapshot",&json!({})).is_err());
        assert!(runtime.game_command("_game_companion_take",&json!({})).is_err());
        assert!(runtime.game_command("_game_companion_unknown", &json!({})).is_err());
        assert!(call(&runtime, "companion_status", json!({})).unwrap()["host"]["instances"]
            .as_array().unwrap().iter().all(|instance| instance["armed"] == false));
        assert!(call(
            &runtime,
            "companion_approve",
            json!({"requestId":"missing"})
        )
        .is_err());
        call(
            &runtime,
            "companion_disconnect",
            json!({"instanceId":"saved-panel"}),
        )
        .unwrap();
        call(
            &runtime,
            "companion_takeover",
            json!({"instanceId":"saved-panel","companionId":"test-installation"}),
        )
        .unwrap();
        call(
            &runtime,
            "companion_revoke",
            json!({"companionId":"test-installation"}),
        )
        .unwrap();
        assert!(runtime.assigned_publication("saved-panel").is_err());
        assert!(call(&runtime, "companion_unsupported", json!({})).is_err());
        assert_eq!(
            call(&runtime, "companion_disable", json!({})).unwrap()["listener"]["enabled"],
            false
        );
        runtime.shutdown();
    }
