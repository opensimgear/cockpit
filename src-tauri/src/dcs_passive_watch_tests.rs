use super::*;
use crate::dcs_profile::TelemetryCatalogue;
use std::{fs, path::Path, thread, time::{Duration, Instant}};

fn copy_catalogue(path: &Path) {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dcs-bios");
    for name in ["A-10C.json", "AircraftAliases.json", "MetadataStart.json", "MetadataEnd.json"] {
        fs::copy(fixtures.join(name), path.join(name)).unwrap();
    }
}

#[test]
fn registering_a_watch_cannot_certify_a_previous_catalogue_capture() {
    let directory = crate::test_directory();
    copy_catalogue(directory.path());
    let capture = TelemetryCatalogue::load(directory.path(), "A-10C").unwrap();
    fs::write(directory.path().join("A-10C.json"), "{}").unwrap();
    assert!(matches!(PassiveCatalogueGuard::open(capture.guard()),
        Err(error) if error == SOURCE_CHANGED));
}

#[test]
fn exact_watch_wakes_for_a_selected_file_and_leaves_sibling_edits_quiet() {
    let directory = crate::test_directory();
    copy_catalogue(directory.path());
    let capture = TelemetryCatalogue::load(directory.path(), "A-10C").unwrap();
    let mut guard = PassiveCatalogueGuard::open(capture.guard()).unwrap();
    assert!(guard.watch.is_some());
    let wake = Arc::new(NativeWake::default());
    wake.bind_thread(thread::current()).unwrap();
    guard.subscribe(&wake);
    let initial = wake.observed();
    fs::write(directory.path().join("OtherAircraft.json"), "{}").unwrap();
    let quiet = Instant::now() + Duration::from_millis(150);
    while let Some(remaining) = quiet.checked_duration_since(Instant::now()) {
        assert_eq!(wake.observed(), initial);
        thread::park_timeout(remaining);
    }
    guard.validate().unwrap();

    fs::write(directory.path().join("A-10C.json"), "{}").unwrap();
    let until = Instant::now() + Duration::from_secs(3);
    while wake.observed() == initial {
        let remaining = until.checked_duration_since(Instant::now()).expect("Exact DCS file change did not wake the reader");
        thread::park_timeout(remaining);
    }
    assert_eq!(guard.validate().unwrap_err(), SOURCE_CHANGED);
    let replacement = Arc::new(NativeWake::default());
    let baseline = replacement.observed();
    guard.subscribe(&replacement);
    assert_ne!(replacement.observed(), baseline, "Binding after loss must still wake the reader");
    let capture = TelemetryCatalogue::load(directory.path(), "A-10C").unwrap();
    assert!(PassiveCatalogueGuard::open(capture.guard()).unwrap().check_watch().is_ok());
}
