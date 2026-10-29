use super::*;
    #[test]
    fn assignment_uses_reported_engines_without_reusing_an_offline_profile_for_a_live_legacy_device(){
        use sha2::{Digest,Sha256};
        let fixtures:Value=serde_json::from_str(include_str!("../../../../../apps/companion-esp32/test/widget-compatibility.json")).unwrap();
        let canonical=fixtures["cases"][0]["canonicalDocument"].as_str().unwrap();let hash=format!("sha256:{:x}",Sha256::digest(canonical.as_bytes()));
        let hub=Hub::new("host".into(),Arc::new(osg_companion_service::NullSourceSink));
        crate::retain_companion_test_fixture(&hub,&json!({"canonicalDocument":canonical,"publicationHash":hash,"assets":[]})).unwrap();
        let reports:Value=serde_json::from_str(include_str!("../../../../../apps/companion-esp32/test/render-profiles.json")).unwrap();let profile=reports["valid"][1].clone();
        let firmware=json!({"version":profile["firmwareVersion"],"profile":profile["boardProfile"],"renderProfile":profile});
        let mut client=json!({"kind":"esp32","connected":true,"screen":{"width":800,"height":480},"renderProfile":profile});
        assert!(validate_target_panel(&hub,&hash,&client,&firmware).is_ok());
        client["renderProfile"]=reports["valid"][0].clone();assert!(validate_target_panel(&hub,&hash,&client,&firmware).unwrap_err().contains("documentBytes"));
        client["renderProfile"]=Value::Null;assert!(validate_target_panel(&hub,&hash,&client,&firmware).is_err());
        client["connected"]=json!(false);assert!(validate_target_panel(&hub,&hash,&client,&firmware).is_ok());
        let mut stale=firmware.clone();stale["version"]=json!("older");assert!(validate_target_panel(&hub,&hash,&client,&stale).is_err());
        client["renderProfileError"]=json!("Conflicting renderer profiles.");assert_eq!(validate_target_panel(&hub,&hash,&client,&firmware).unwrap_err(),"Conflicting renderer profiles.");
    }
    #[test]
    fn reported_renderer_profiles_are_matched_to_current_usb_identity_and_conflicts_are_visible(){
        let fixture:Value=serde_json::from_str(include_str!("../../../../../apps/companion-esp32/test/render-profiles.json")).unwrap();
        let profile=fixture["valid"][0].clone();
        let usb=json!({"connected":true,"ready":true,"device":{"companionId":"screen","firmwareVersion":profile["firmwareVersion"],"boardProfile":profile["boardProfile"],"renderProfile":profile}});
        let mut host=json!({"clients":[{"companionId":"screen","kind":"esp32","connected":false}],"instances":[]});
        describe_usb_presence(&mut host,&[usb.clone()]);assert_eq!(host["clients"][0]["renderProfile"],profile);
        host["clients"][0]["renderProfile"]["screen"]["width"]=json!(480);
        describe_usb_presence(&mut host,&[usb.clone()]);assert!(host["clients"][0]["renderProfile"].is_null()&&host["clients"][0]["renderProfileError"].is_string());
        let mut invalid=usb;invalid["device"]["firmwareVersion"]=json!("older");assert!(usb_render_profile(&invalid).is_none());
        invalid["ready"]=json!(false);assert!(usb_render_profile(&invalid).is_none());
    }
    #[test]
    fn pre_open_usb_diagnostics_require_current_assignment_identity_and_nonce_freshness() {
        let hash=format!("sha256:{}","a".repeat(64));
        let assignment=json!({"instanceId":"instance","publicationHash":hash,"companionId":"sunton"});
        let mut usb=json!({"connected":true,"ready":true,"statusAgeMs":100,"device":{"companionId":"sunton",
            "operation":{"instanceId":"instance","publicationHash":hash,"attemptId":2,"phase":"failed",
                "errorCode":"open-panel-rejected","retryable":true,"message":"Assigned panel was removed"}}});
        let report=usb_panel_operation(&assignment,&[usb.clone()]).unwrap();
        assert!(!report.live_session); assert!(report.fresh); assert_eq!(report.operation["errorCode"],"open-panel-rejected");
        usb["statusAgeMs"]=json!(1500);
        assert!(!usb_panel_operation(&assignment,&[usb.clone()]).unwrap().fresh,"Stale nonce status is retry context only");
        for (path,value) in [("connected",json!(false)),("ready",json!(false))] {
            let mut invalid=usb.clone(); invalid[path]=value;
            assert!(usb_panel_operation(&assignment,&[invalid]).is_none());
        }
        let mut other=usb.clone(); other["device"]["companionId"]=json!("waveshare");
        assert!(usb_panel_operation(&assignment,&[other]).is_none());
        let mut other=usb.clone(); other["device"]["operation"]["instanceId"]=json!("other-instance");
        assert!(usb_panel_operation(&assignment,&[other]).is_none());
        let mut other=usb.clone(); other["device"]["operation"]["publicationHash"]=json!(format!("sha256:{}","b".repeat(64)));
        assert!(usb_panel_operation(&assignment,&[other]).is_none());
        assert!(usb_panel_operation(&assignment,&[usb.clone(),usb]).is_none(),"Ambiguous USB identities cannot explain a switch");
    }
    #[test]
    fn usb_presence_is_independent_of_panel_session_errors(){
        let mut host=json!({"clients":[{"companionId":"screen","connected":false}]});
        describe_usb_presence(&mut host,&[json!({"connected":true,"ready":true,"device":{"companionId":"screen","status":"panel-error"},"deviceStatus":"panel-unsupported-corrupt-or-over-budget"})]);
        assert_eq!(host["clients"][0]["connected"],true);
        assert_eq!(host["clients"][0]["sessionConnected"],false);
        assert!(host["clients"][0]["panelError"].is_string());
        let mut offline=json!({"clients":[{"companionId":"screen","connected":false}]});
        describe_usb_presence(&mut offline,&[json!({"connected":false,"ready":true,"device":{"companionId":"screen"}})]);
        assert_eq!(offline["clients"][0]["connected"],false);
    }
    #[test]
    fn usb_status_preserves_the_reported_failure_without_guessing_unsupported_content(){
        for (status,message,panel_error) in [("panel-error","Panel image download failed",true),("error","Cockpit response timed out",false),("opening-panel","Downloading panel images",false)]{
            let mut host=json!({"clients":[{"companionId":"screen","connected":false}]});
            describe_usb_presence(&mut host,&[json!({"connected":true,"ready":true,"device":{"companionId":"screen","status":status,"statusMessage":message},"deviceStatus":"panel-unsupported-corrupt-or-over-budget","lastHostError":"Old ESP32 memory budget failure"})]);
            let client=&host["clients"][0];
            assert_eq!(client["connected"],true);
            assert_eq!(client["connectionStatus"],message);
            assert_eq!(client["panelError"].as_str(),if panel_error{Some(message)}else{None});
        }
    }
    #[test]
    fn usb_recovery_describes_only_the_exact_paired_companion_without_claiming_a_session(){
        for (phase,connected) in [("waiting",true),("connecting",true),("waiting-for-device",false)]{
            let message="USB connection interrupted; reconnecting automatically";
            let mut host=json!({"clients":[{"companionId":"screen","connected":false},{"companionId":"other","connected":false}]});
            describe_usb_presence(&mut host,&[json!({"connected":connected,"ready":connected,"recovery":{"phase":phase,"message":message},"device":{"companionId":"screen","status":"error","statusMessage":"Heartbeat: invalid USB reply"}})]);
            assert_eq!(host["clients"][0]["connectionStatus"],message);
            assert_eq!(host["clients"][0]["connected"],connected);
            assert_eq!(host["clients"][0]["sessionConnected"],false);
            assert!(host["clients"][1]["connectionStatus"].is_null());
            assert_eq!(host["clients"][1]["connected"],false);
        }
    }
    #[test]
    fn recovered_usb_presence_prefers_the_live_port_and_restores_normal_status(){
        let mut host=json!({"clients":[{"companionId":"screen","connected":true}]});
        let old=json!({"connected":false,"ready":false,"recovery":{"phase":"waiting-for-device","message":"Reconnecting automatically"},"device":{"companionId":"screen","status":"error"}});
        let recovered=json!({"connected":true,"ready":true,"recovery":{"phase":"recovered","message":"Recovered old connection"},"device":{"companionId":"screen","status":"live-armed","statusMessage":"Panel connected"}});
        describe_usb_presence(&mut host,&[old,recovered]);
        assert_eq!(host["clients"][0]["connected"],true);
        assert_eq!(host["clients"][0]["connectionStatus"],"Panel connected");
        for message in ["","Bad\nmessage"]{
            assert!(usb_recovery_message(&json!({"recovery":{"phase":"waiting","message":message}})).is_none());
        }
        assert!(usb_recovery_message(&json!({"recovery":{"phase":"waiting","message":"x".repeat(192)}})).is_none());
    }
    #[test]
    fn esp_panels_match_screen_dimensions_on_every_page() {
        assert_eq!(screen_dimensions("sunton-3248s035r"),Some((480,320)));
        assert_eq!(screen_dimensions("waveshare-lcd7-s3"),Some((800,480)));
        assert_eq!(screen_dimensions("unknown"),None);
        let mut document=json!({"ui":{"pages":[{"id":"a"}]},"presentation":{"widgets":[],"viewports":[{"pageId":"a","width":480,"height":320}]}});
        assert!(validate_screen_document(&document,(480,320)).is_ok());
        assert!(validate_screen_document(&document,(320,480)).is_err());
        document["ui"]["pages"].as_array_mut().unwrap().push(json!({"id":"b"}));
        assert!(validate_screen_document(&document,(480,320)).is_err());
    }
    #[test]
    fn inventory_reads_preserve_absent_setup_and_report_stopped_or_corrupt_state() {
        let directory = crate::test_directory();
        for path in [None, Some(directory.path().join("absent"))] {
            let runtime = Runtime::new(path.clone(), Arc::new(CompanionProvider::new(Arc::new(|| 1))));
            assert!(runtime.resource_instance_ids().unwrap().is_empty());
            assert!(runtime.resource_companions().unwrap().is_empty());
            assert!(runtime.resource_transport("missing").is_none());
            assert!(runtime.resource_publication("missing").is_err());
            assert!(runtime.profile_panel_operation("missing", "missing").is_none());
            assert!(runtime.profile_panel_operation_history("missing").is_none());
            assert!(runtime.profile_retry_panel("missing", "missing").is_ok());
            assert!(runtime.metadata.get().is_none());
            assert!(runtime.services.get().is_none());
            assert!(path.is_none_or(|path| !path.exists()));
            runtime.shutdown();
            assert!(runtime.resource_instance_ids().is_err());
            assert!(runtime.resource_companions().is_err());
        }
        let anchor=directory.path().join("companion-host-state.dpapi");
        std::fs::write(&anchor,b"retired invalid file").unwrap();
        crate::persistence::test_support::record(&anchor,"secrets","companion-host",b"corrupt");
        {
            let runtime = Runtime::new(Some(directory.path().into()), Arc::new(CompanionProvider::new(Arc::new(|| 1))));
            assert!(runtime.resource_instance_ids().is_err());
            assert!(runtime.resource_companions().is_err());
            assert!(runtime.services.get().is_none());
            assert_eq!(osg_persistence::Database::for_namespace(&anchor).unwrap().read("secrets","companion-host").unwrap().unwrap().payload,b"corrupt");
            assert_eq!(std::fs::read(&anchor).unwrap(),b"retired invalid file");
        }
    }
    #[test]
    fn screen_validation_enforces_device_content_and_touch_limits() {
        let base = json!({"ui":{"pages":[{"id":"a"}]},"presentation":{
            "widgets":[{"kind":"button","label":"Test","pageId":"a","bounds":{"width":0.5,"height":0.5},
                "config":{"buttonMode":"momentary","pulseMs":0,"repeatMs":0}}],
            "viewports":[{"pageId":"a","width":480,"height":320}]}});
        assert!(validate_screen_document(&base, (480,320)).is_ok());
        for (pointer, value, message) in [
            ("/presentation/viewports/0/backgroundAssetId", json!("image"), "image widgets"),
            ("/presentation/groups", json!([{"id":"group"}]), "groups"),
            ("/presentation/widgets/0/config/iconAssetId", json!("icon"), "image widgets"),
            ("/presentation/widgets/0/groupId", json!("group"), "groups"),
            ("/presentation/widgets/0/label", json!("Unsupported 🚀"), "font"),
            ("/presentation/widgets/0/kind", json!("slider"), "widget type"),
            ("/presentation/widgets/0/config/buttonMode", json!("toggle"), "momentary"),
            ("/presentation/widgets/0/bounds/width", json!(0.01), "44 by 44"),
            ("/presentation/widgets/0/config/prefix", json!("🚀"), "font"),
            ("/presentation/widgets", json!([]), ""),
        ] {
            let mut document = base.clone();
            // Object keys may be absent in the valid document.
            let (parent, key) = pointer.rsplit_once('/').unwrap();
            document.pointer_mut(parent).unwrap()[key] = value;
            if message.is_empty() { assert!(validate_screen_document(&document,(480,320)).is_ok()); }
            else { assert!(validate_screen_document(&document,(480,320)).unwrap_err().contains(message)); }
        }
        for kind in ["numeric", "gauge", "image", "text", "indicator"] {
            let mut document = base.clone();
            let widget = &mut document["presentation"]["widgets"][0];
            widget["kind"] = json!(kind);
            widget["config"] = json!({"precision":4,"min":-1e9,"max":1e9,"assetId":"image"});
            assert!(validate_screen_document(&document,(480,320)).is_ok());
            if matches!(kind,"numeric"|"gauge") {
                document["presentation"]["widgets"][0]["config"]["precision"] = json!(5);
                assert!(validate_screen_document(&document,(480,320)).unwrap_err().contains("precision"));
            }
        }
        let mut document = base.clone();
        document["presentation"]["widgets"] = json!(vec![base["presentation"]["widgets"][0].clone();33]);
        assert!(validate_screen_document(&document,(480,320)).unwrap_err().contains("32 widget"));
        document["presentation"]["widgets"] = json!([{"kind":"image","label":"Image","pageId":"a","bounds":{"width":4,"height":4},"config":{"assetId":"image"}}]);
        assert!(validate_screen_document(&document,(480,320)).unwrap_err().contains("pixel budget"));
        document["presentation"]["widgets"][0]["bounds"] = json!({"width":0.5,"height":0.5});
        document["presentation"]["widgets"][0]["config"] = json!({});
        assert!(validate_screen_document(&document,(480,320)).unwrap_err().contains("Choose an asset"));
    }
    #[test]
    fn supplied_wire_fixture_acquisition_preserves_its_hub_without_native_services() {
        let directory = crate::test_directory();
        let transport_directory = directory.path().join("uninitialized-transports");
        let provider = Arc::new(CompanionProvider::new(Arc::new(|| 1)));
        let hub = Hub::new("already-connected-wire-fixture".into(), provider.clone());
        let runtime = Runtime::with_profile_metadata(
            hub.clone(), provider.clone(), transport_directory.clone(),
        );
        for _ in 0..2 {
            let acquired = runtime.profile_acquire_transports().unwrap();
            assert_eq!(acquired.host_id(), hub.host_id());
            assert!(Arc::ptr_eq(&runtime.profile_provider(), &provider));
            assert!(runtime.services.get().is_none());
            assert!(!transport_directory.exists());
        }
        runtime.shutdown();
        assert!(runtime.profile_acquire_transports().is_err());
        assert!(runtime.services.get().is_none());

        // The ordinary constructor still requires the real services admission.
        let ordinary = self::runtime();
        assert!(ordinary.services.set(Err("native transport admission required".into())).is_ok());
        assert_eq!(ordinary.profile_acquire_transports().err().unwrap(), "native transport admission required");
    }
    #[test]
    fn missing_profile_metadata_does_not_create_state_or_cache_a_failed_setup() {
        let directory = std::env::temp_dir().join(format!("osg-companion-metadata-{}", uuid::Uuid::new_v4()));
        let runtime = Runtime::new(Some(directory.clone()), Arc::new(CompanionProvider::new(Arc::new(|| 1))));
        assert!(runtime.profile_metadata().is_err());
        assert!(runtime.metadata.get().is_none());
        assert!(runtime.services.get().is_none());
        assert!(!directory.exists());
        assert!(Runtime::new(None, Arc::new(CompanionProvider::new(Arc::new(|| 1)))).profile_metadata().is_err());
        runtime.shutdown();
        assert!(runtime.profile_metadata().is_err());
    }
    #[cfg(windows)]
    #[test]
    fn retained_profile_metadata_restores_descriptors_without_listener_or_tls_setup() {
        let directory = std::env::temp_dir().join(format!("osg-companion-metadata-retained-{}", uuid::Uuid::new_v4()));
        let hub = Hub::open(&directory, Arc::new(osg_companion_service::NullSourceSink)).unwrap();
        let mut upload:Value = serde_json::from_str(include_str!("../../../../../crates/companion-service/tests/fixtures/compiled-toggle-publication.json")).unwrap();
        upload["command"] = json!("companion_publish");
        hub.command(&upload).unwrap();
        // A saved listener is shared setup. Merely inspecting a profile must not
        // construct a server or attempt to bind this saved endpoint.
        std::fs::write(directory.join("companion-listener.json"), b"invalid listener configuration").unwrap();
        let database=osg_persistence::Database::for_namespace(&directory.join("companion-host-state.dpapi")).unwrap();
        let before = database.read("secrets","companion-host").unwrap().unwrap();
        let runtime = Runtime::new(Some(directory.clone()), Arc::new(CompanionProvider::new(Arc::new(|| 1))));
        let metadata = runtime.profile_metadata().unwrap();
        assert_eq!(metadata.host_id(), hub.host_id());
        assert_eq!(metadata.publication_uploads().len(), 1);
        assert!(runtime.resource_instance_ids().unwrap().is_empty());
        assert!(runtime.resource_companions().unwrap().is_empty());
        assert!(runtime.resource_transport("missing").is_none());
        assert!(runtime.resource_publication("missing").is_err());
        assert!(runtime.profile_panel_operation("missing", "missing").is_none());
        assert!(runtime.profile_panel_operation_history("missing").is_none());
        let saved = runtime.saved_panel_updates().unwrap();
        assert_eq!(saved.len(), 1);
        assert!(runtime.saved_panel_heads().unwrap().is_empty());
        assert!(runtime.saved_panel_references().unwrap().is_empty(), "An archive alone is not a saved head");
        let inventory=call(&runtime,"companion_inventory",json!({})).unwrap();
        assert_eq!(inventory["host"]["publications"].as_array().unwrap().len(),1);
        assert_eq!(inventory["listener"]["enabled"],false);
        assert!(runtime.services.get().is_none());
        assert_eq!(database.read("secrets","companion-host").unwrap().unwrap(), before);
        metadata.mark_saved(upload["publicationHash"].as_str().unwrap()).unwrap();
        assert_eq!(runtime.saved_panel_heads().unwrap()[&saved[0].panel_id], saved[0].publication_hash);
        assert_eq!(runtime.saved_panel_references().unwrap()[0].publication_hash, saved[0].publication_hash);
        assert!(runtime.services.get().is_none());
        assert_eq!(database.read("secrets","companion-host").unwrap().unwrap().revision,before.revision+1);
        assert!(database.read("secrets","companion-identity").unwrap().is_none());
        let mut names = std::fs::read_dir(&directory).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, ["cockpit.sqlite3", "companion-listener.json"]);
        runtime.shutdown();
        for name in names { std::fs::remove_file(directory.join(name)).unwrap(); }
        std::fs::remove_dir(directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn native_startup_creates_installation_unique_ca_and_reload_keeps_it() {
        fn start(directory: &std::path::Path) -> (Arc<Runtime>, String) {
            let runtime = Arc::new(Runtime::new(
                Some(directory.to_path_buf()),
                Arc::new(CompanionProvider::new(Arc::new(|| 1))),
            ));
            runtime.start_detection();
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while runtime.services.get().is_none() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "startup services were not initialized"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            let certificate = runtime.services().unwrap().server.browser_certificate().unwrap();
            (runtime, certificate)
        }

        let first_dir = crate::test_directory();
        let second_dir = crate::test_directory();
        let (first, first_ca) = start(first_dir.path());
        let (second, second_ca) = start(second_dir.path());
        assert_ne!(
            first_ca, second_ca,
            "each installation must generate its own root CA"
        );

        first.shutdown();
        let (reloaded, reloaded_ca) = start(first_dir.path());
        assert_eq!(
            first_ca, reloaded_ca,
            "restart must retain the installed root CA"
        );
        reloaded.shutdown();
        second.shutdown();
    }
    #[cfg(windows)]
    #[test]
    fn explicit_companion_request_reports_unreadable_retained_state_without_replacing_it() {
        let directory =
            std::env::temp_dir().join(format!("osg-companion-lazy-error-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("companion-host-state.dpapi");
        std::fs::write(&path,b"retired invalid file").unwrap();
        crate::persistence::test_support::record(&path,"secrets","companion-host",b"preserve-unreadable-state");
        let runtime = Runtime::new(
            Some(directory.clone()),
            Arc::new(CompanionProvider::new(Arc::new(|| 1))),
        );
        assert!(runtime.services.get().is_none());
        assert!(call(&runtime, "companion_status", json!({})).is_err());
        assert!(matches!(runtime.services.get(), Some(Err(_))));
        assert_eq!(osg_persistence::Database::for_namespace(&path).unwrap().read("secrets","companion-host").unwrap().unwrap().payload,b"preserve-unreadable-state");
        assert_eq!(std::fs::read(&path).unwrap(),b"retired invalid file");
        runtime.shutdown();
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(directory.join("cockpit.sqlite3")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
    #[test]
    fn runtime_construction_does_not_initialize_or_modify_protected_companion_state() {
        let directory = std::env::temp_dir().join(format!(
            "osg-companion-lazy-startup-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("companion-host-state.dpapi");
        std::fs::write(&path, b"preserve-unavailable-protected-state").unwrap();
        let provider = Arc::new(CompanionProvider::new(Arc::new(|| 1)));
        let runtime = Runtime::new(Some(directory.clone()), provider);
        assert!(
            runtime.services.get().is_none(),
            "Constructing the runtime must not enter an OS credential service"
        );
        runtime.shutdown();
        assert!(runtime.services.get().is_none());
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"preserve-unavailable-protected-state"
        );
        assert!(!directory.join("companion-identity.dpapi").exists());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
