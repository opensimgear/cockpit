use super::*;
use notify::event::{CreateKind, DataChange, RemoveKind, RenameMode};
use std::ffi::OsStr;

#[test]
fn dcs_filesystem_filters_select_only_provider_inputs() {
    let create = EventKind::Create(CreateKind::Folder);
    let remove = EventKind::Remove(RemoveKind::Folder);
    let rename = EventKind::Modify(ModifyKind::Name(RenameMode::Both));
    let write = EventKind::Modify(ModifyKind::Data(DataChange::Content));
    assert!(dcs_profile_directory(create, OsStr::new("DCS")));
    assert!(dcs_profile_directory(remove, OsStr::new("dcs.openbeta")));
    assert!(dcs_profile_directory(rename, OsStr::new("DCS.custom")));
    assert!(!dcs_profile_directory(create, OsStr::new("DCSbackup")));
    assert!(!dcs_profile_directory(write, OsStr::new("DCS")));

    assert!(catalogue_file(write, OsStr::new("AircraftAliases.JSON")));
    assert!(catalogue_file(rename, OsStr::new("A-10C.json")));
    assert!(!catalogue_file(write, OsStr::new("README.txt")));
    assert!(!catalogue_file(write, OsStr::new("json")));
}
