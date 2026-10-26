use super::*;
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

fn temp_root(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("opensimgear-platform-{label}-{nonce}"))
}

#[test]
fn client_catalogue_has_executable_presence_signatures() {
    for client in ["steam", "epic", "battlenet", "ea", "ubisoft", "gog", "xbox"] {
        assert!(
            !exes_for(client).is_empty(),
            "missing executable signatures for {client}"
        );
        assert!(exes_for(client)
            .iter()
            .all(|exe| exe.to_ascii_lowercase().ends_with(".exe")));
    }
    assert!(exes_for("disk").is_empty());
}

#[test]
fn launcher_presence_requires_a_real_executable_file() {
    let root = temp_root("presence");
    let exe = root.join(exes_for("gog")[0]);
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    assert!(
        !has_client_exe(&root, exes_for("gog")),
        "stale install record alone is not enough"
    );
    fs::create_dir(&exe).unwrap();
    assert!(
        !has_client_exe(&root, exes_for("gog")),
        "a directory ending in .exe is not an executable"
    );
    fs::remove_dir(&exe).unwrap();
    fs::write(&exe, b"launcher").unwrap();
    assert!(has_client_exe(&root, exes_for("gog")));
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn symlinked_launcher_is_not_accepted() {
    use std::os::unix::fs::symlink;
    let root = temp_root("symlink");
    fs::create_dir_all(&root).unwrap();
    let target = root.join("real.exe");
    fs::write(&target, b"launcher").unwrap();
    symlink(&target, root.join("GalaxyClient.exe")).unwrap();
    assert!(!has_client_exe(&root, exes_for("gog")));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn custom_install_root_is_detected_without_game_data() {
    let root = temp_root("custom");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("XboxPcApp.exe"), b"launcher").unwrap();
    assert!(has_client_exe(&root, exes_for("xbox")));
    assert!(
        fs::read_dir(&root).unwrap().count() == 1,
        "no game records are required"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn only_local_paths_are_unconditionally_included() {
    assert_eq!(installed_clients().last(), Some(&("disk", "Local paths")));
    assert!(CLIENTS
        .iter()
        .filter(|(id, _)| *id != "disk")
        .all(|(id, _)| !exes_for(id).is_empty()));
}
