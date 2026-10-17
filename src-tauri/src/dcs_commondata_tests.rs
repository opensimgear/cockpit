use super::*;

struct Fixture(tempfile::TempDir);
impl Fixture {
    fn new() -> Self {
        let directory = crate::test_directory();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dcs-bios");
        for name in [
            "A-10C.json",
            "AircraftAliases.json",
            "MetadataStart.json",
            "MetadataEnd.json",
        ] {
            fs::copy(fixtures.join(name), directory.path().join(name)).unwrap();
        }
        fs::copy(
            fixtures.join("CommonData-altitude.json"),
            directory.path().join("CommonData.json"),
        )
        .unwrap();
        let fixture = Self(directory);
        fixture.edit("AircraftAliases.json", |value| {
            value["A-10C"]
                .as_array_mut()
                .unwrap()
                .push(json!("CommonData"));
        });
        fixture
    }
    fn edit(&self, name: &str, edit: impl FnOnce(&mut Value)) {
        let path = self.0.path().join(name);
        let mut value = dcs_bios::read_json(&path, dcs_bios::MAX_DOCUMENT).unwrap();
        edit(&mut value);
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    fn resource(&self) -> Result<Value, String> {
        resource_catalogue(&json!({"cataloguePath":self.0.path(),"aircraftModule":"A-10C"}))
    }
    fn capture(&self) -> TelemetryCatalogue {
        TelemetryCatalogue::load(self.0.path(), "A-10C").unwrap()
    }
    fn guard(&self) -> DcsCatalogueGuard {
        self.capture().guard()
    }
}
fn rows() -> Vec<Value> {
    vec![json!({"id":"aircraft/altitude","channel":"common:CommonData:ALT_MSL_FT","output":0})]
}

fn assert_mh6_commondata_only(path: &Path) {
    // The repo example registers a genuine empty read-only module. Its official
    // JSON writer emits this object and adds only this exact aircraft alias.
    fs::write(path.join("MH-6J.json"), b"{\n\n}").unwrap();
    let alias_path = path.join("AircraftAliases.json");
    let original = dcs_bios::read_json(&alias_path, dcs_bios::MAX_DOCUMENT).unwrap();
    assert!(original.get("MH-6J").is_none());
    let mut aliases = original.clone();
    aliases["MH-6J"] = json!(["MH-6J", "CommonData"]);
    fs::write(&alias_path, serde_json::to_vec(&aliases).unwrap()).unwrap();
    for (name, modules) in original.as_object().unwrap() {
        assert_eq!(&aliases[name], modules, "Existing aliases are preserved");
    }

    let request = json!({"cataloguePath":path,"aircraftModule":"MH-6J"});
    let result = resource_catalogue(&request).unwrap();
    let controls = result["controls"].as_array().unwrap();
    assert!(!controls.is_empty());
    assert!(controls.iter().all(|control| {
        control["sourceModule"] == "CommonData" && control["inputs"] == json!([])
    }));
    let altitude = controls
        .iter()
        .find(|control| control["id"] == "common:CommonData:ALT_MSL_FT")
        .unwrap();
    assert_eq!(altitude["sourceControl"], "ALT_MSL_FT");
    assert!(altitude["reportedQuality"]
        .as_str()
        .unwrap()
        .contains("not advertised"));
    let commands = query_catalogue(&json!({
        "cataloguePath":path,"aircraftModule":"MH-6J","kind":"input"
    }))
    .unwrap();
    assert_eq!(commands["total"], 0);
    assert_eq!(commands["controls"], json!([]));
    let ports = crate::profile_resource_ports::integration(&result).unwrap();
    assert!(ports
        .iter()
        .all(|port| port.direction == osg_profiles::Direction::Output));
    let port = ports
        .iter()
        .find(|port| port.id == "channel:common:CommonData:ALT_MSL_FT:0")
        .unwrap();
    assert!(matches!(
        &port.contract,
        osg_profiles::resources_ports::PortContract::State {
            schema: osg_profiles::resources_ports::ScalarSchema::Integer {
                min: 0,
                max: 65535,
                unit: None
            },
            fallback: None,
            ..
        }
    ));

    let revision = result["sourceRevision"].as_str().unwrap();
    let (sources, mut guard, metadata) =
        guarded_telemetry_sources(path, "MH-6J", &rows(), revision).unwrap();
    assert_eq!(metadata.aircraft_names, HashSet::from(["MH-6J".into()]));
    guard.validate(true).unwrap();
    let mut decoder = ExportSession::from_sources(metadata, 100, sources).unwrap();
    let frame = |name: &str, counter: u8| {
        let mut bytes = vec![0x55; 4];
        let mut aircraft = [0; 24];
        aircraft[..name.len()].copy_from_slice(name.as_bytes());
        for (address, data) in [
            (0u16, aircraft.to_vec()),
            (1076, 10000u16.to_le_bytes().to_vec()),
            (65534, vec![counter, 0]),
        ] {
            bytes.extend(address.to_le_bytes());
            bytes.extend((data.len() as u16).to_le_bytes());
            bytes.extend(data);
        }
        bytes
    };
    decoder.feed(&frame("MH-6J", 1), 0);
    decoder.feed(&frame("MH-6J", 2), 1);
    assert!(decoder.snapshot(1).fresh);
    assert_eq!(
        decoder.snapshot(1).values["aircraft/altitude"],
        json!(10000)
    );
    for (counter, wrong) in [(3, "AH-6"), (4, "AH-6J"), (5, "BlackHawk")] {
        decoder.feed(&frame(wrong, counter), u64::from(counter));
        assert!(!decoder.snapshot(u64::from(counter)).fresh, "{wrong}");
    }

    // A user cannot turn a known MH-6J reader into the incompatible older
    // command module by editing the alias after its immutable capture.
    aliases["MH-6J"] = json!(["CommonData", "AH-6J"]);
    fs::write(alias_path, serde_json::to_vec(&aliases).unwrap()).unwrap();
    assert!(guard.validate(true).is_err());
    assert!(guarded_telemetry_sources(path, "MH-6J", &rows(), revision).is_err());
    assert!(resource_catalogue(&request).is_err());
}

#[test]
fn mh6_self_named_read_only_catalogue_preserves_identity_and_has_no_ah6_commands() {
    let fixture = Fixture::new();
    assert_mh6_commondata_only(fixture.0.path());
}

#[test]
fn only_alias_authorized_common_outputs_are_namespaced_and_legacy_commands_are_unchanged() {
    let fixture = Fixture::new();
    fixture.edit("CommonData.json", |value| {
        value["Altitude"]["ALT_MSL_FT"]["inputs"] =
            json!([{"interface":"set_state","max_value":65535}]);
    });
    let result = fixture.resource().unwrap();
    let common = result["controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["id"] == "common:CommonData:ALT_MSL_FT")
        .unwrap();
    assert_eq!(common["sourceModule"], "CommonData");
    assert_eq!(common["sourceControl"], "ALT_MSL_FT");
    assert_eq!(common["inputs"], json!([]));
    assert!(common["reportedQuality"]
        .as_str()
        .unwrap()
        .contains("not advertised"));
    let ports = crate::profile_resource_ports::integration(&result).unwrap();
    let port = ports
        .iter()
        .find(|port| port.id == "channel:common:CommonData:ALT_MSL_FT:0")
        .unwrap();
    assert!(matches!(
        &port.contract,
        osg_profiles::resources_ports::PortContract::State {
            schema: osg_profiles::resources_ports::ScalarSchema::Integer {
                min: 0,
                max: 65535,
                unit: None
            },
            fallback: None,
            ..
        }
    ));
    assert!(ports
        .iter()
        .all(|port| !port.id.starts_with("command:common:")));
    let legacy =
        query_catalogue(&json!({"cataloguePath":fixture.0.path(),"aircraftModule":"A-10C"}))
            .unwrap();
    assert!(legacy["controls"]
        .as_array()
        .unwrap()
        .iter()
        .all(|value| value["sourceModule"].is_null()));
    fixture.edit("AircraftAliases.json", |value| {
        value["A-10C"] = json!(["A-10C"])
    });
    assert!(fixture.resource().unwrap()["controls"]
        .as_array()
        .unwrap()
        .iter()
        .all(|value| value["sourceModule"].is_null()));
    assert!(fixture.capture().common.is_none());
    assert!(guarded_telemetry_sources(
        fixture.0.path(),
        "A-10C",
        &rows(),
        &fixture.capture().revision
    )
    .is_err());
    assert!(TelemetryCatalogue::load(fixture.0.path(), "CommonData").is_err());
    assert!(TelemetryCatalogue::load(fixture.0.path(), "../A-10C").is_err());
}

#[test]
fn exact_official_reported_values_need_received_bytes_and_selected_aircraft_progress() {
    let fixture = Fixture::new();
    let capture = fixture.capture();
    let (sources, _, metadata) =
        guarded_telemetry_sources(fixture.0.path(), "A-10C", &rows(), &capture.revision).unwrap();
    let mut decoder = ExportSession::from_sources(metadata, 100, sources).unwrap();
    fn frame(name: &str, altitude: Option<u16>, counter: u8) -> Vec<u8> {
        fn block(bytes: &mut Vec<u8>, address: u16, data: &[u8]) {
            bytes.extend(address.to_le_bytes());
            bytes.extend((data.len() as u16).to_le_bytes());
            bytes.extend(data);
        }
        let mut bytes = vec![0x55; 4];
        let mut aircraft = [b' '; 24];
        aircraft[..name.len()].copy_from_slice(name.as_bytes());
        block(&mut bytes, 0, &aircraft);
        if let Some(altitude) = altitude {
            block(&mut bytes, 1076, &altitude.to_le_bytes());
        }
        block(&mut bytes, 65534, &[counter, 0]);
        bytes
    }
    decoder.feed(&frame("A-10C", None, 1), 0);
    assert!(decoder
        .snapshot(0)
        .values
        .get("aircraft/altitude")
        .is_none());
    decoder.feed(&frame("A-10C", Some(0), 2), 1);
    assert_eq!(
        decoder.snapshot(1).values["aircraft/altitude"],
        json!(0),
        "Wire zero is a reported value, never guessed absence"
    );
    decoder.feed(&frame("A-10C", Some(12000), 3), 2);
    assert_eq!(
        decoder.snapshot(2).values["aircraft/altitude"],
        json!(12000)
    );
    decoder.tick(103);
    assert!(!decoder.snapshot(103).fresh);
    decoder.feed(&frame("F-16C_50", Some(12000), 4), 104);
    assert!(!decoder.snapshot(104).fresh);
    decoder.feed(&frame("NONE", Some(12000), 5), 105);
    assert!(!decoder.snapshot(105).mission_active);
    assert!(guarded_telemetry_sources(fixture.0.path(), "A-10C", &rows(), "sha256:wrong").is_err());
    let mut other = rows();
    other[0]["channel"] = json!("common:Other:ALT_MSL_FT");
    assert!(
        guarded_telemetry_sources(fixture.0.path(), "A-10C", &other, &capture.revision).is_err()
    );
}

#[test]
fn missing_common_is_optional_but_present_malformed_or_linked_is_not() {
    let fixture = Fixture::new();
    let path = fixture.0.path().join("CommonData.json");
    fs::remove_file(&path).unwrap();
    let mut absent = fixture.guard();
    assert!(fixture.resource().unwrap()["controls"]
        .as_array()
        .unwrap()
        .iter()
        .all(|v| v["sourceModule"].is_null()));
    fs::write(&path, b"{").unwrap();
    assert!(fixture.resource().is_err());
    assert!(absent.validate(true).is_err());
    for bad in [
        json!([]),
        json!({"Bad":[]}),
        json!({"Bad":{"X":{"identifier":"wrong","inputs":[],"outputs":[]}}}),
        json!({"Bad":{"X":{"identifier":"X","inputs":[],"outputs":[{"type":"integer"}]}}}),
        json!({"Bad":{"X":{"identifier":"X","inputs":[],"outputs":[{"type":"string","address":65534,"max_length":4}]}}}),
    ] {
        fs::write(&path, serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(fixture.resource().is_err());
    }
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(fixture.resource().is_err());
    fs::remove_dir(&path).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(fixture.0.path().join("A-10C.json"), &path).unwrap();
        assert!(fixture.resource().is_err());
    }
}

#[test]
fn every_captured_document_and_an_address_only_change_revoke_live_and_prepared_readers() {
    for name in [
        "CommonData.json",
        "AircraftAliases.json",
        "A-10C.json",
        "MetadataStart.json",
        "MetadataEnd.json",
    ] {
        let fixture = Fixture::new();
        let old = fixture.capture();
        let mut guard = old.guard();
        fixture.edit(name, |value| {
            if name == "CommonData.json" {
                value["Altitude"]["ALT_MSL_FT"]["outputs"][0]["address"] = json!(1078);
            } else {
                value["capturedChange"] = json!({});
            }
        });
        assert!(guard.validate(true).is_err(), "{name}");
        assert!(
            guarded_telemetry_sources(fixture.0.path(), "A-10C", &rows(), &old.revision).is_err(),
            "{name}"
        );
    }
    let fixture = Fixture::new();
    let mut guard = fixture.guard();
    guard.validate(false).unwrap();
    guard.checked -= Duration::from_millis(251);
    guard.full_checked -= Duration::from_secs(3);
    guard.validate(false).unwrap();
    fixture.edit("CommonData.json", |value| {
        value["Altitude"]["ALT_MSL_FT"]["outputs"][0]["address"] = json!(1078)
    });
    guard.checked -= Duration::from_millis(251);
    assert!(guard.validate(false).is_err());
}

#[test]
fn same_length_and_restored_timestamp_edits_are_caught_by_the_bounded_full_digest() {
    let fixture = Fixture::new();
    fixture.edit("CommonData.json", |_| {});
    let path = fixture.0.path().join("CommonData.json");
    let before = catalogue_file(&path).unwrap();
    let mut guard = fixture.guard();
    fixture.edit("CommonData.json", |value| {
        value["Altitude"]["ALT_MSL_FT"]["outputs"][0]["address"] = json!(1078);
    });
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(before.as_ref().unwrap().modified.unwrap()))
        .unwrap();
    assert_eq!(catalogue_file(&path).unwrap(), before);
    // Immediate admissions check actual metadata; cryptographic content checks
    // have an explicit two-second bound, not a claim of atomic filesystem I/O.
    guard.validate(true).unwrap();
    guard.checked -= Duration::from_millis(251);
    guard.full_checked -= Duration::from_secs(3);
    assert!(guard.validate(false).is_err());
}

#[test]
#[ignore = "Requires the checksum-pinned official DCS-BIOS archive in OSG_DCS_SETUP_ARCHIVE; no simulator or hardware"]
fn pinned_official_archive_commondata_normalization_and_native_guard() {
    use std::io::Read;
    let archive =
        fs::read(std::env::var_os("OSG_DCS_SETUP_ARCHIVE").expect("Set OSG_DCS_SETUP_ARCHIVE"))
            .unwrap();
    assert_eq!(archive.len(), 8_234_988);
    assert_eq!(
        format!("{:x}", Sha256::digest(&archive)),
        crate::dcs_setup::ARCHIVE_SHA256
    );
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive)).unwrap();
    let directory = crate::test_directory();
    for name in [
        "A-10C.json",
        "AircraftAliases.json",
        "MetadataStart.json",
        "MetadataEnd.json",
        "CommonData.json",
    ] {
        let suffix = format!("doc/json/{name}");
        let indices = (0..zip.len())
            .filter(|index| zip.by_index(*index).unwrap().name().ends_with(&suffix))
            .collect::<Vec<_>>();
        assert_eq!(indices.len(), 1, "{name}");
        let mut file = zip.by_index(indices[0]).unwrap();
        assert!(file.size() <= dcs_bios::MAX_DOCUMENT);
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        fs::write(directory.path().join(name), bytes).unwrap();
    }
    let result =
        resource_catalogue(&json!({"cataloguePath":directory.path(),"aircraftModule":"A-10C"}))
            .unwrap();
    let common = result["controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|control| control["id"] == "common:CommonData:ALT_MSL_FT")
        .unwrap();
    assert_eq!(
        common["outputs"][0],
        json!({"type":"integer","maxValue":65535,"index":0})
    );
    let (sources, mut guard, _) = guarded_telemetry_sources(
        directory.path(),
        "A-10C",
        &rows(),
        result["sourceRevision"].as_str().unwrap(),
    )
    .unwrap();
    let OutputSource::Integer(output) = &sources[0].1 else {
        panic!()
    };
    assert_eq!((output.address, output.maximum), (1076, 65535));
    let mut memory = Memory::new();
    memory.apply(&[crate::dcs_bios::Block {
        address: 1076,
        data: 0xa55au16.to_le_bytes().to_vec(),
    }]);
    assert_eq!(
        sources[0].1.read(&memory),
        Some(json!(0xa55a)),
        "Actual parsed mask/shift preserve the full reported word"
    );
    guard.validate(true).unwrap();
    // Repeat the extension contract with complete checksum-pinned upstream
    // documents, preserving every official alias and CommonData control.
    assert_mh6_commondata_only(directory.path());
}
