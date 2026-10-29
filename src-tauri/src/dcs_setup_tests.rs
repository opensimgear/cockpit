use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn ownship_physics_setup_is_ready_idempotent_and_owned_removal_restores_other_hooks() {
    let scratch = Scratch::new();
    let target = scratch.target();
    let work = scratch.work();
    fs::create_dir_all(target.join("Scripts")).unwrap();
    let original = b"-- third-party hooks\r\nLuaExportStart = otherStart\r\n";
    fs::write(target.join("Scripts/Export.lua"), original).unwrap();
    let ready = install(&target, &work, fixture_files(), &|| false, &|_, _| {}).unwrap();
    assert_eq!(ready["physicsExportsReady"], true);
    assert_eq!(ready["physicsChannelCount"], 101);
    let path = target.join("Scripts").join(physics::RELATIVE);
    let source = read(&path, MAX_FILE).unwrap();
    let export = read(&target.join("Scripts/Export.lua"), MAX_EXPORT).unwrap();
    assert!(export.starts_with(original));
    setup(&target, &work, || false, |_, _| {}, || Ok(())).unwrap();
    assert_eq!(read(&path, MAX_FILE).unwrap(), source);
    assert_eq!(completed_receipts(&target, &work).unwrap().len(), 1);
    remove(&target, &work, || false, |_, _| {}, || Ok(())).unwrap();
    assert!(!path.exists());
    assert_eq!(
        read(&target.join("Scripts/Export.lua"), MAX_EXPORT).unwrap(),
        original
    );
}

#[test]
fn ownship_physics_repair_backs_up_previous_managed_source_and_preserves_external_edits() {
    let scratch = Scratch::new();
    let target = scratch.target();
    let work = scratch.work();
    fixture_module(&target);
    fs::create_dir_all(target.join("Scripts/OpenSimGear")).unwrap();
    let old_body = "-- earlier managed ownship source\n";
    let old = format!(
        "-- OpenSimGear managed ownship physics v1\n-- sha256: {}\n{old_body}",
        hash(old_body.as_bytes())
    );
    let path = target.join("Scripts").join(physics::RELATIVE);
    fs::write(&path, old.as_bytes()).unwrap();
    fs::write(target.join("Scripts/Export.lua"), LOADER).unwrap();
    let ready = setup(&target, &work, || false, |_, _| {}, || Ok(())).unwrap();
    assert_eq!(ready["physicsExportsReady"], true);
    let receipts = completed_receipts(&target, &work).unwrap();
    assert_eq!(receipts.len(), 1);
    let receipt = &receipts[0].2;
    assert_eq!(receipt["physicsBefore"], hash(old.as_bytes()));
    assert_eq!(
        read(
            &work
                .join(receipt["id"].as_str().unwrap())
                .join("DCSPhysics.lua.before"),
            MAX_FILE
        )
        .unwrap(),
        old.as_bytes()
    );
    let installed = read(&path, MAX_FILE).unwrap();
    fs::write(&path, b"-- external customization\n").unwrap();
    assert!(remove(&target, &work, || false, |_, _| {}, || Ok(())).is_err());
    assert_eq!(
        read(&path, MAX_FILE).unwrap(),
        b"-- external customization\n"
    );
    assert_eq!(inspect(&target).unwrap()["repairable"], false);
    fs::write(&path, installed).unwrap();
    remove(&target, &work, || false, |_, _| {}, || Ok(())).unwrap();
    assert_eq!(read(&path, MAX_FILE).unwrap(), old.as_bytes());
    assert!(target.join("Scripts/DCS-BIOS/BIOS.lua").exists());
    assert_eq!(
        read(&target.join("Scripts/Export.lua"), MAX_EXPORT).unwrap(),
        LOADER.as_bytes()
    );
}

#[test]
fn ownship_physics_cancel_after_export_write_restores_exact_original_bytes() {
    let scratch = Scratch::new();
    let target = scratch.target();
    let work = scratch.work();
    fs::create_dir_all(target.join("Scripts")).unwrap();
    let original = b"-- preserved user hook\n";
    fs::write(target.join("Scripts/Export.lua"), original).unwrap();
    let cancelled = AtomicBool::new(false);
    assert!(install(
        &target,
        &work,
        fixture_files(),
        &|| cancelled.load(Ordering::SeqCst),
        &|_, percent| {
            if percent == 95 {
                cancelled.store(true, Ordering::SeqCst);
            }
        }
    )
    .is_err());
    assert!(!target.join("Scripts").join(physics::RELATIVE).exists());
    assert_eq!(
        read(&target.join("Scripts/Export.lua"), MAX_EXPORT).unwrap(),
        original
    );
    assert!(!recovery_pending(&work));
}

#[test]
fn remove_owned_install_restores_original_scripts_and_preserves_other_files() {
    let scratch = Scratch::new();
    let target = scratch.target();
    let work = scratch.work();
    fs::create_dir_all(target.join("Scripts")).unwrap();
    let original = b"-- Existing unrelated exporter\n";
    fs::write(target.join("Scripts/Export.lua"), original).unwrap();
    fs::write(target.join("Scripts/Other.lua"), b"external script").unwrap();
    install_guarded(
        &target,
        &work,
        fixture_files(),
        &|| false,
        &|_, _| {},
        &|| Ok(()),
    )
    .unwrap();
    assert!(removal_available(&target, &work).unwrap());
    remove(&target, &work, || false, |_, _| {}, || Ok(())).unwrap();
    assert_eq!(
        fs::read(target.join("Scripts/Export.lua")).unwrap(),
        original
    );
    assert_eq!(
        fs::read(target.join("Scripts/Other.lua")).unwrap(),
        b"external script"
    );
    assert!(!target.join("Scripts/DCS-BIOS").exists());
    assert!(!removal_available(&target, &work).unwrap());
    assert!(!recovery_pending(&work));
}

#[test]
fn remove_reverses_aircraft_repairs_before_the_owned_installation() {
    let scratch = Scratch::new();
    let target = scratch.target();
    let work = scratch.work();
    install_guarded(
        &target,
        &work,
        fixture_files(),
        &|| false,
        &|_, _| {},
        &|| Ok(()),
    )
    .unwrap();
    mh6_mod(&target);
    install_guarded(
        &target,
        &work,
        BTreeMap::new(),
        &|| false,
        &|_, _| {},
        &|| Ok(()),
    )
    .unwrap();
    assert!(target.join("Scripts").join(aircraft::RELATIVE).exists());
    remove(&target, &work, || false, |_, _| {}, || Ok(())).unwrap();
    assert!(!target.join("Scripts/DCS-BIOS").exists());
    assert!(!target.join("Scripts/Export.lua").exists());
    assert!(!target.join("Scripts").join(aircraft::RELATIVE).exists());
    assert!(target.join("Mods/aircraft/AH-6J/entry.lua").exists());
}

#[test]
fn remove_preserves_external_modules_and_refuses_modified_owned_exports() {
    let scratch = Scratch::new();
    let target = scratch.target();
    let work = scratch.work();
    fixture_module(&target);
    assert!(!removal_available(&target, &work).unwrap());
    assert!(remove(&target, &work, || false, |_, _| {}, || Ok(()))
        .unwrap_err()
        .contains("not installed by Cockpit"));
    install_guarded(
        &target,
        &work,
        BTreeMap::new(),
        &|| false,
        &|_, _| {},
        &|| Ok(()),
    )
    .unwrap();
    remove(&target, &work, || false, |_, _| {}, || Ok(())).unwrap();
    assert!(target.join("Scripts/DCS-BIOS/BIOS.lua").exists());
    assert!(!target.join("Scripts/Export.lua").exists());

    let owned = scratch.0.join("Owned");
    fs::create_dir(&owned).unwrap();
    install_guarded(
        &owned,
        &work,
        fixture_files(),
        &|| false,
        &|_, _| {},
        &|| Ok(()),
    )
    .unwrap();
    let export = owned.join("Scripts/Export.lua");
    fs::write(&export, b"external replacement export").unwrap();
    assert!(remove(&owned, &work, || false, |_, _| {}, || Ok(()))
        .unwrap_err()
        .contains("preserved"));
    assert_eq!(fs::read(&export).unwrap(), b"external replacement export");
    assert!(owned.join("Scripts/DCS-BIOS/BIOS.lua").exists());
    assert!(!recovery_pending(&work));
}

#[test]
fn interrupted_remove_uses_existing_recovery_journal() {
    let scratch = Scratch::new();
    let target = scratch.target();
    let work = scratch.work();
    install_guarded(
        &target,
        &work,
        fixture_files(),
        &|| false,
        &|_, _| {},
        &|| Ok(()),
    )
    .unwrap();
    let receipts = completed_receipts(&target, &work).unwrap();
    recovery::Ledger::open(&work)
        .unwrap()
        .begin_removal(&receipts[0].2)
        .unwrap();
    assert!(recovery_pending(&work));
    assert_eq!(recover(&work).unwrap()["restored"], true);
    assert!(!target.join("Scripts/DCS-BIOS").exists());
    assert!(!target.join("Scripts/Export.lua").exists());
    assert!(!recovery_pending(&work));
}

#[test]
fn removed_unrelated_user_folder_does_not_block_owned_integration_removal() {
    let scratch = Scratch::new();
    let target = scratch.target();
    let work = scratch.work();
    install_guarded(
        &target,
        &work,
        fixture_files(),
        &|| false,
        &|_, _| {},
        &|| Ok(()),
    )
    .unwrap();
    fs::remove_dir_all(&target).unwrap();
    let other = scratch.0.join("DCS.other");
    fs::create_dir(&other).unwrap();
    install_guarded(
        &other,
        &work,
        fixture_files(),
        &|| false,
        &|_, _| {},
        &|| Ok(()),
    )
    .unwrap();
    remove(&other, &work, || false, |_, _| {}, || Ok(())).unwrap();
    assert!(!other.join("Scripts/DCS-BIOS").exists());
    assert!(!recovery_pending(&work));
}

fn mh6_mod(target: &Path) -> PathBuf {
    let root = target.join("Mods/aircraft/AH-6J");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("entry.lua"), "make_flyable('MH-6J')").unwrap();
    root
}

#[test]
fn optional_aircraft_install_repeat_remove_and_cancel_restore_owned_bytes() {
    let scratch = Scratch::new();
    let target = scratch.target();
    let work = scratch.work();
    let mod_root = mh6_mod(&target);
    let ready = install_guarded(
        &target,
        &work,
        fixture_files(),
        &|| false,
        &|_, _| {},
        &|| Ok(()),
    )
    .unwrap();
    assert_eq!(ready["ready"], true);
    assert_eq!(inspect(&target).unwrap()["ready"], true);
    assert_eq!(ready["aircraftExports"][0]["compatible"], true);
    let path = target.join("Scripts").join(aircraft::RELATIVE);
    let hook_before = read(&path, MAX_FILE).unwrap();
    let export_before = read(&target.join("Scripts/Export.lua"), MAX_EXPORT).unwrap();
    assert_eq!(
        setup(&target, &work, || false, |_, _| {}, || Ok(())).unwrap()["ready"],
        true
    );
    assert_eq!(read(&path, MAX_FILE).unwrap(), hook_before);
    fs::write(mod_root.join("entry.lua"), "-- removed").unwrap();
    let cancel = AtomicBool::new(false);
    let error = install_guarded(
        &target,
        &work,
        BTreeMap::new(),
        &|| cancel.load(Ordering::Relaxed),
        &|_, percent| {
            if percent == 95 {
                cancel.store(true, Ordering::Relaxed);
            }
        },
        &|| Ok(()),
    )
    .unwrap_err();
    assert!(error.contains("restored"), "{error}");
    assert_eq!(read(&path, MAX_FILE).unwrap(), hook_before);
    assert_eq!(
        read(&target.join("Scripts/Export.lua"), MAX_EXPORT).unwrap(),
        export_before
    );
    assert!(!recovery_pending(&work));
    assert_eq!(
        setup(&target, &work, || false, |_, _| {}, || Ok(())).unwrap()["ready"],
        true
    );
    assert!(!path.exists());
    assert!(
        !String::from_utf8(read(&target.join("Scripts/Export.lua"), MAX_EXPORT).unwrap())
            .unwrap()
            .contains("DCSAircraft")
    );
}

#[test]
fn aircraft_rollback_removes_new_hook_but_preserves_concurrent_edits() {
    for external_edit in [false, true] {
        let scratch = Scratch::new();
        let target = scratch.target();
        let work = scratch.work();
        mh6_mod(&target);
        let path = target.join("Scripts").join(aircraft::RELATIVE);
        let cancel = AtomicBool::new(false);
        let error = install_guarded(
            &target,
            &work,
            fixture_files(),
            &|| cancel.load(Ordering::Relaxed),
            &|_, percent| {
                if percent == 95 {
                    if external_edit {
                        fs::write(&path, b"external aircraft edit").unwrap();
                    }
                    cancel.store(true, Ordering::Relaxed);
                }
            },
            &|| Ok(()),
        )
        .unwrap_err();
        assert!(error.contains("cancelled"), "{error}");
        assert_eq!(recovery_pending(&work), external_edit);
        assert_eq!(path.exists(), external_edit);
        if external_edit {
            assert_eq!(fs::read(&path).unwrap(), b"external aircraft edit");
        }
        assert!(!target.join("Scripts/Export.lua").exists());
    }
}
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let p = std::env::temp_dir().canonicalize().unwrap().join(format!("osg-dcs-setup-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn target(&self) -> PathBuf {
        let p = self.0.join("DCS");
        fs::create_dir_all(&p).unwrap();
        p
    }
    fn work(&self) -> PathBuf {
        self.0.join("recovery")
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn discovers_multiple_and_custom_variant_without_selecting_ambiguously() {
    let s = Scratch::new();
    let saved = s.0.join("Saved Games");
    let install = s.0.join("game");
    fs::create_dir_all(&install).unwrap();
    for n in ["DCS", "DCS.openbeta", "DCS.custom", "Other"] {
        fs::create_dir_all(saved.join(n)).unwrap();
    }
    fs::write(install.join("dcs_variant.txt"), "custom\n").unwrap();
    assert_eq!(candidates(&saved, Some(&install)).unwrap().len(), 3);
    fs::write(install.join("dcs_variant.txt"), "../other").unwrap();
    assert!(candidates(&saved, Some(&install))
        .unwrap_err()
        .contains("ambiguous"));
    fs::write(install.join("dcs_variant.txt"), "missing").unwrap();
    assert!(candidates(&saved, Some(&install)).is_err());
    assert!(candidates(Path::new("relative"), None).is_err());
}
#[test]
fn absent_module_and_loader_are_setup_required_and_game_root_is_rejected() {
    let s = Scratch::new();
    let target = s.target();
    let state = inspect(&target).unwrap();
    assert_eq!(state["ready"], false);
    assert_eq!(state["repairable"], true);
    assert_eq!(state["modulePresent"], false);
    fs::create_dir_all(target.join("bin")).unwrap();
    fs::write(target.join("bin/DCS.exe"), []).unwrap();
    assert!(inspect(&target).unwrap_err().contains("game installation"));
    assert!(inspect(Path::new("relative")).is_err());
}
#[test]
fn appends_once_preserving_third_party_exports_bom_crlf_and_legacy_bytes() {
    for before in [
        b"\xef\xbb\xbf-- Tacview\r\ndofile('Scripts/TacviewGameExport.lua')\r\n".to_vec(),
        b"-- ANSI user comment \xff\ndofile('Scripts/Helios.lua')".to_vec(),
        Vec::new(),
    ] {
        let after = with_loader(&before).unwrap();
        assert!(after.starts_with(&before));
        assert_eq!(loader_count(&after).unwrap(), 1);
        assert_eq!(with_loader(&after).unwrap(), after);
        if before.windows(2).any(|p| p == b"\r\n") {
            assert!(after.ends_with(b"\r\n"));
        }
    }
}
#[test]
fn utf16_exports_are_blocked_without_mutation_or_false_readiness() {
    for little in [true, false] {
        let mut before = if little {
            vec![0xff, 0xfe]
        } else {
            vec![0xfe, 0xff]
        };
        for n in format!("-- Third party\r\n{LOADER}\r\n").encode_utf16() {
            before.extend_from_slice(&if little {
                n.to_le_bytes()
            } else {
                n.to_be_bytes()
            });
        }
        assert!(with_loader(&before).unwrap_err().contains("UTF-16"));
        let scratch = Scratch::new();
        let target = scratch.target();
        fixture_module(&target);
        let export = target.join("Scripts/Export.lua");
        fs::write(&export, &before).unwrap();
        let state = inspect(&target).unwrap();
        assert_eq!(state["ready"], false);
        assert_eq!(state["repairable"], false);
        assert!(
            setup(&target, &scratch.work(), || false, |_, _| {}, || Ok(()))
                .unwrap_err()
                .contains("UTF-16")
        );
        assert_eq!(fs::read(export).unwrap(), before);
    }
    assert!(with_loader(&[0xff, 0xfe, 1]).is_err());
    assert!(with_loader(&[0xfe, 0xff, 0xd8, 0]).is_err());
    assert!(with_loader(&[0]).is_err());
}
#[test]
fn loader_analysis_ignores_comments_strings_and_accepts_equivalent_quoted_paths() {
    for text in [
        LOADER,
        "dofile(lfs.writedir() .. 'Scripts/DCS-BIOS/BIOS.lua')",
        "dofile ( lfs.writedir ( ) .. \"Scripts\\\\DCS-BIOS\\\\BIOS.lua\" )",
    ] {
        assert_eq!(loader_count(text.as_bytes()).unwrap(), 1);
    }
    for text in [
        format!("-- {LOADER}\n"),
        format!("--[[{LOADER}]]\n"),
        "--[=[DCS-BIOS BIOS.lua]=]\n".into(),
        "local str = [[not a loader]]".into(),
    ] {
        assert_eq!(loader_count(text.as_bytes()).unwrap(), 0);
    }
    for text in [
        format!("{LOADER}\n{LOADER}"),
        "dofile('C:/custom/DCS-BIOS/BIOS.lua')".into(),
        "local reference = [[Scripts/DCS-BIOS/BIOS.lua]]".into(),
        "--[[incomplete".into(),
        "dofile('incomplete".into(),
    ] {
        assert!(with_loader(text.as_bytes()).is_err(), "{text}");
    }
}
#[test]
fn clean_install_is_ready_while_closed_and_repeated_setup_has_no_file_mutations() {
    let s = Scratch::new();
    let target = s.target();
    let work = s.work();
    let state = install(&target, &work, fixture_files(), &|| false, &|_, _| {}).unwrap();
    assert_eq!(state["ready"], true);
    assert_eq!(state["version"], VERSION);
    assert!(!recovery_pending(&work));
    let before = fs::read(target.join("Scripts/Export.lua")).unwrap();
    let ready = setup(&target, &work, || false, |_, _| {}, || Ok(())).unwrap();
    assert_eq!(ready["ready"], true);
    assert_eq!(before, fs::read(target.join("Scripts/Export.lua")).unwrap());
}
#[test]
fn compatible_existing_module_is_reused_and_only_missing_loader_repaired() {
    let s = Scratch::new();
    let target = s.target();
    let work = s.work();
    fixture_module(&target);
    let original = b"-- Tacview export\r\ndofile('Scripts/Tacview.lua')\r\n";
    fs::write(target.join("Scripts/Export.lua"), original).unwrap();
    let bios = fs::read(target.join("Scripts/DCS-BIOS/BIOS.lua")).unwrap();
    assert_eq!(inspect(&target).unwrap()["repairable"], true);
    let result = setup(&target, &work, || false, |_, _| {}, || Ok(())).unwrap();
    assert_eq!(result["ready"], true);
    assert_eq!(
        bios,
        fs::read(target.join("Scripts/DCS-BIOS/BIOS.lua")).unwrap()
    );
    assert!(fs::read(target.join("Scripts/Export.lua"))
        .unwrap()
        .starts_with(original));
}
#[test]
fn fork_config_duplicate_loader_and_malformed_catalogue_are_preserved() {
    let s = Scratch::new();
    let target = s.target();
    fixture_module(&target);
    let config = target.join("Scripts/DCS-BIOS/BIOSConfig.lua");
    fs::write(&config, b"version = 'unknown'").unwrap();
    assert_eq!(inspect(&target).unwrap()["repairable"], false);
    assert!(setup(&target, &s.work(), || false, |_, _| {}, || Ok(())).is_err());
    assert_eq!(fs::read(&config).unwrap(), b"version = 'unknown'");
    fixture_module(&target);
    fs::write(
        target.join("Scripts/Export.lua"),
        format!("{LOADER}\n{LOADER}"),
    )
    .unwrap();
    assert_eq!(inspect(&target).unwrap()["repairable"], false);
}
#[test]
fn malformed_unrelated_aircraft_catalogue_does_not_block_readiness() {
    let s = Scratch::new();
    let target = s.target();
    fixture_module(&target);
    fs::write(
        target.join("Scripts/DCS-BIOS/doc/json/MiG-29 Fulcrum.json"),
        b"broken",
    )
    .unwrap();
    let state = inspect_base(&target).unwrap();
    assert_eq!(state["moduleReady"], true, "{state}");
}
#[test]
fn malformed_required_shared_catalogue_metadata_blocks_readiness() {
    let s = Scratch::new();
    let target = s.target();
    fixture_module(&target);
    fs::write(
        target.join("Scripts/DCS-BIOS/doc/json/MetadataStart.json"),
        b"broken",
    )
    .unwrap();
    let state = inspect_base(&target).unwrap();
    assert_eq!(state["moduleReady"], false, "{state}");
}
#[test]
fn cancel_before_mutation_and_during_staging_restores_clean_target() {
    let s = Scratch::new();
    let target = s.target();
    let work = s.work();
    assert!(install(&target, &work, fixture_files(), &|| true, &|_, _| {}).is_err());
    assert!(!target.join("Scripts").exists());
    let cancelled = AtomicBool::new(false);
    let error = install(
        &target,
        &work,
        fixture_files(),
        &|| cancelled.load(Ordering::Acquire),
        &|_, percent| {
            if percent >= 55 {
                cancelled.store(true, Ordering::Release);
            }
        },
    )
    .unwrap_err();
    assert!(error.contains("cancelled"));
    assert!(!recovery_pending(&work));
    assert!(!target.join("Scripts/DCS-BIOS").exists());
    assert!(!target.join("Scripts/Export.lua").exists());
}
#[test]
fn cancelled_final_verification_restores_existing_export_and_removes_owned_module() {
    let s = Scratch::new();
    let target = s.target();
    let work = s.work();
    fs::create_dir(target.join("Scripts")).unwrap();
    let original = b"-- keep this third-party export\n";
    fs::write(target.join("Scripts/Export.lua"), original).unwrap();
    let cancelled = AtomicBool::new(false);
    let error = install(
        &target,
        &work,
        fixture_files(),
        &|| cancelled.load(Ordering::Acquire),
        &|_, percent| {
            if percent == 95 {
                cancelled.store(true, Ordering::Release);
            }
        },
    )
    .unwrap_err();
    assert!(error.contains("restored"));
    assert_eq!(
        fs::read(target.join("Scripts/Export.lua")).unwrap(),
        original
    );
    assert!(!target.join("Scripts/DCS-BIOS").exists());
    assert!(!recovery_pending(&work));
}
fn interrupted(s: &Scratch) -> (PathBuf, Value) {
    let target = s.target();
    let work = s.work();
    fs::create_dir_all(&work).unwrap();
    fs::create_dir(target.join("Scripts")).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    fs::create_dir(work.join(&id)).unwrap();
    let before = b"-- original export\n";
    let after = with_loader(before).unwrap();
    fs::write(work.join(&id).join("Export.lua.before"), before).unwrap();
    fs::write(target.join("Scripts/Export.lua"), &after).unwrap();
    let value = json!({"id":id,"target":target,"files":{},"exportBefore":hash(before),"exportAfter":hash(&after),"installModule":false,"createdScripts":false,"version":VERSION,"archiveSha256":ARCHIVE_SHA256,"aircraftBefore":null,"aircraftAfter":null,"createdAircraftDir":false,"physicsBefore":null,"physicsAfter":hash(b"none"),"createdPhysicsDir":false});
    save_journal(&work, &value).unwrap();
    (target, value)
}
#[test]
fn interrupted_recovery_is_repeatable_and_retains_export_backups() {
    let s = Scratch::new();
    let (target, value) = interrupted(&s);
    assert!(recovery_pending(&s.work()));
    let recovered = recover(&s.work()).unwrap();
    assert_eq!(recovered["restored"], true);
    assert_eq!(
        fs::read(target.join("Scripts/Export.lua")).unwrap(),
        b"-- original export\n"
    );
    assert!(s
        .work()
        .join(value["id"].as_str().unwrap())
        .join("Export.lua.before")
        .exists());
    assert_eq!(recover(&s.work()).unwrap()["restored"], true);
}
#[test]
fn external_export_change_is_preserved_and_recovery_remains_pending() {
    let s = Scratch::new();
    let (target, _) = interrupted(&s);
    fs::write(target.join("Scripts/Export.lua"), b"-- external change\n").unwrap();
    let recovered = recover(&s.work()).unwrap();
    assert_eq!(recovered["restored"], false);
    assert!(recovery_pending(&s.work()));
    assert_eq!(
        fs::read(target.join("Scripts/Export.lua")).unwrap(),
        b"-- external change\n"
    );
    assert!(setup(&target, &s.work(), || false, |_, _| {}, || Ok(()))
        .unwrap_err()
        .contains("Recover"));
}
#[test]
fn external_module_creation_during_staging_is_preserved() {
    let s = Scratch::new();
    let target = s.target();
    let work = s.work();
    let first = AtomicBool::new(true);
    let error = install(&target, &work, fixture_files(), &|| false, &|_, percent| {
        if percent >= 55 && first.swap(false, Ordering::SeqCst) {
            fs::create_dir_all(target.join("Scripts/DCS-BIOS")).unwrap();
            fs::write(target.join("Scripts/DCS-BIOS/external.txt"), b"keep").unwrap();
        }
    })
    .unwrap_err();
    assert!(error.contains("preserved"));
    assert_eq!(
        fs::read(target.join("Scripts/DCS-BIOS/external.txt")).unwrap(),
        b"keep"
    );
    assert!(recovery_pending(&work));
}
#[test]
fn archive_rejects_traversal_oversize_and_missing_module_and_cancels() {
    use zip::{write::SimpleFileOptions, ZipWriter};
    fn archive(name: &str, data: &[u8]) -> Vec<u8> {
        let mut w = ZipWriter::new(std::io::Cursor::new(Vec::new()));
        w.start_file(name, SimpleFileOptions::default()).unwrap();
        w.write_all(data).unwrap();
        w.finish().unwrap().into_inner()
    }
    assert!(unpack(&archive("../evil", b"bad"), &|| false).is_err());
    assert!(unpack(
        &archive("DCS-BIOS/BIOS.lua", b"missing other files"),
        &|| false
    )
    .is_err());
    assert!(unpack(&[], &|| false).is_err());
    assert!(unpack(&archive("DCS-BIOS/BIOS.lua", b"a"), &|| true)
        .unwrap_err()
        .contains("cancelled"));
    for p in [
        "../evil",
        "/absolute",
        "C:/path",
        "a\\b",
        "a//b",
        "file.",
        "file ",
        "a\0",
    ] {
        assert!(safe_relative(p).is_err(), "{p}");
    }
    assert!(safe_relative("lib/BIOSStateMachine.lua").is_ok());
}
#[test]
fn unknown_module_runtime_is_not_a_compatible_release() {
    let s = Scratch::new();
    let target = s.target();
    let module = target.join("Scripts/DCS-BIOS");
    fs::create_dir_all(&module).unwrap();
    fs::write(module.join("BIOS.lua"), b"-- unknown fork").unwrap();
    assert_eq!(inspect(&target).unwrap()["moduleReady"], false);
    assert_eq!(inspect(&target).unwrap()["repairable"], false);
}
#[cfg(unix)]
#[test]
fn links_in_game_side_paths_are_rejected() {
    use std::os::unix::fs::symlink;
    let s = Scratch::new();
    let target = s.target();
    let elsewhere = s.0.join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    symlink(&elsewhere, target.join("Scripts")).unwrap();
    assert!(inspect(&target).unwrap_err().contains("link"));
}

/// Reads the official downloaded archive only when explicitly requested.
/// This qualifies release packaging and software checks, not a real Windows/DCS mission.
#[test]
#[ignore = "requires the checksum-pinned official DCS-BIOS archive"]
fn official_pinned_archive_smoke() {
    let path = std::env::var_os("OSG_DCS_SETUP_ARCHIVE")
        .expect("Set OSG_DCS_SETUP_ARCHIVE to the official archive");
    let bytes = read(Path::new(&path), ARCHIVE_SIZE).unwrap();
    assert_eq!(bytes.len(), ARCHIVE_SIZE);
    assert_eq!(hash(&bytes), ARCHIVE_SHA256);
    let files = unpack(&bytes, &|| false).unwrap();
    let scratch = Scratch::new();
    let target = scratch.target();
    let ready = install(&target, &scratch.work(), files, &|| false, &|_, _| {}).unwrap();
    assert_eq!(ready["ready"], true);
    assert_eq!(ready["version"], VERSION);
    assert!(!target.join("Scripts/DCS-BIOS/Start.html").exists());
    assert!(!target.join("Scripts/Export.lua.before").exists());
    assert_eq!(
        setup(&target, &scratch.work(), || false, |_, _| {}, || Ok(())).unwrap()["ready"],
        true
    );
}

#[test]
fn running_game_at_write_boundaries_preserves_exports_and_restores_owned_staging() {
    use std::cell::Cell;
    for refused_boundary in [1, 2, 3] {
        let scratch = Scratch::new();
        let target = scratch.target();
        let scripts = target.join("Scripts");
        fs::create_dir(&scripts).unwrap();
        let original = b"-- Existing Tacview export\n";
        fs::write(scripts.join("Export.lua"), original).unwrap();
        let boundaries = Cell::new(0);
        let staged = Cell::new(false);
        let error = install_guarded(
            &target,
            &scratch.work(),
            fixture_files(),
            &|| false,
            &|message, _| {
                if message == "Staging DCS-BIOS module" {
                    staged.set(true);
                }
            },
            &|| {
                boundaries.set(boundaries.get() + 1);
                if boundaries.get() == refused_boundary {
                    if refused_boundary >= 2 {
                        assert!(staged.get(), "Guard must recheck after staging");
                    }
                    if refused_boundary == 2 {
                        assert!(!scripts.join("DCS-BIOS").exists());
                    }
                    if refused_boundary == 3 {
                        assert!(scripts.join("DCS-BIOS").exists());
                    }
                    Err(
                        "DCS started during setup. Close DCS before changing export scripts."
                            .into(),
                    )
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
        assert_eq!(boundaries.get(), refused_boundary);
        assert!(error.contains("Close DCS"), "{error}");
        assert!(error.contains("Previous files were restored"), "{error}");
        assert_eq!(fs::read(scripts.join("Export.lua")).unwrap(), original);
        assert!(!scripts.join("DCS-BIOS").exists());
        assert!(!recovery_pending(&scratch.work()));
        let remaining = fs::read_dir(&scripts)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(remaining, vec![std::ffi::OsString::from("Export.lua")]);
    }
}
