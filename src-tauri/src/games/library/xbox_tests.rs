use super::xbox;
use super::Scan;
use std::{fs, path::Path};

fn game(root: &Path, folder: &str, config: &str, exe: Option<&str>) {
    let content = root.join(folder).join("Content");
    fs::create_dir_all(&content).unwrap();
    fs::write(content.join("MicrosoftGame.config"), config).unwrap();
    if let Some(exe) = exe {
        let path = content.join(exe);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"fixture executable").unwrap();
    }
}

const CONFIG: &str = r#"<Game configVersion="1"><Identity Name="Publisher.Racer" Publisher="CN=Publisher"/><ExecutableList><Executable Name="bin/Racer.exe" Id="Game" TargetDeviceFamily="PC"/></ExecutableList><ShellVisuals DefaultDisplayName="Racer &amp; Rally"/></Game>"#;

#[test]
fn reads_identity_display_name_and_executable_from_config() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("XboxGames");
    game(&root, "Racer", CONFIG, Some("bin/Racer.exe"));
    let mut scan = Scan::default();
    xbox::scan_roots(&[root], &mut scan);
    assert_eq!(scan.candidates.len(), 1);
    let candidate = &scan.candidates[0];
    assert_eq!(candidate.id, "xbox:Publisher.Racer");
    assert_eq!(candidate.client, "xbox");
    assert_eq!(candidate.app_id, "Publisher.Racer");
    assert_eq!(candidate.name, "Racer & Rally");
    assert_eq!(candidate.kind, "game");
    assert_eq!(candidate.game_id, None);
    assert_eq!(candidate.executable_candidates.len(), 1);
    assert!(candidate.executable_candidates[0].ends_with("Racer.exe"));
    assert!(candidate.launcher.is_none());
    assert!(candidate.artwork.is_none());
}

#[test]
fn malformed_config_unsafe_identity_and_traversal_runtime_are_rejected_safely() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("XboxGames");
    game(
        &root,
        "Malformed",
        "<Game><Identity Name=oops></Game>",
        None,
    );
    game(
        &root,
        "UnsafeIdentity",
        "<Game><Identity Name=\"../escape\"/></Game>",
        None,
    );
    game(
        &root,
        "Traversal",
        "<Game><Identity Name=\"Publisher.Traversal\"/><Executable Name=\"../outside.exe\"/></Game>",
        None,
    );
    fs::write(
        directory.path().join("outside.exe"),
        b"must not be suggested",
    )
    .unwrap();
    let mut scan = Scan::default();
    xbox::scan_roots(&[root], &mut scan);
    assert_eq!(scan.candidates.len(), 1);
    assert_eq!(scan.candidates[0].app_id, "Publisher.Traversal");
    assert!(scan.candidates[0].executable_candidates.is_empty());
    assert!(scan
        .diagnostics
        .iter()
        .any(|message| message.contains("malformed")));
}

#[test]
fn parses_namespace_comments_numeric_entities_and_only_pc_executables() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("XboxGames");
    let xml = r#"<?xml version="1.0"?>
        <g:Game xmlns:g="urn:games">
            <!-- <Identity Name="Commented.Out"/> -->
            <g:Identity Name="Publisher.NamespaceGame"/>
            <g:ShellVisuals DefaultDisplayName="Racer &#38; Rally"/>
            <g:Executable Name="console/Console.exe" TargetDeviceFamily="XboxOne"/>
            <g:Executable Name="bin/PC.exe" TargetDeviceFamily="PC"/>
        </g:Game>"#;
    game(&root, "Namespace", xml, Some("bin/PC.exe"));
    let mut scan = Scan::default();
    xbox::scan_roots(&[root], &mut scan);
    assert_eq!(scan.candidates.len(), 1);
    assert_eq!(scan.candidates[0].app_id, "Publisher.NamespaceGame");
    assert_eq!(scan.candidates[0].name, "Racer & Rally");
    assert_eq!(scan.candidates[0].executable_candidates.len(), 1);
    assert!(scan.candidates[0].executable_candidates[0].ends_with("PC.exe"));
}

#[test]
fn rejects_duplicate_attributes_and_unclosed_xml() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("XboxGames");
    game(
        &root,
        "DuplicateAttribute",
        "<Game><Identity Name=\"Publisher.Bad\" Name=\"Publisher.Other\"/></Game>",
        None,
    );
    game(
        &root,
        "Unclosed",
        "<Game><Identity Name=\"Publisher.Unclosed\"/>",
        None,
    );
    let mut scan = Scan::default();
    xbox::scan_roots(&[root], &mut scan);
    assert!(scan.candidates.is_empty());
}

#[test]
fn deduplicates_identity_and_ignores_stale_non_game_entries() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("XboxGames");
    game(&root, "First", CONFIG, None);
    game(&root, "Duplicate", CONFIG, None);
    fs::create_dir_all(root.join("Stale/Content")).unwrap();
    fs::write(
        root.join("Stale/Content/MicrosoftGame.config"),
        b"<Game><Identity Name=\"Stale\"/></Game>",
    )
    .unwrap();
    let mut scan = Scan::default();
    xbox::scan_roots(&[root], &mut scan);
    assert_eq!(scan.candidates.len(), 1);
    assert_eq!(scan.candidates[0].app_id, "Publisher.Racer");
}

#[test]
fn only_inspects_a_bounded_number_of_roots() {
    let directory = tempfile::tempdir().unwrap();
    let valid = directory.path().join("XboxGames");
    game(&valid, "Game", CONFIG, None);
    let mut roots = (0..26)
        .map(|index| directory.path().join(format!("missing-{index}")))
        .collect::<Vec<_>>();
    roots.push(valid);
    let mut scan = Scan::default();
    xbox::scan_roots(&roots, &mut scan);
    assert!(scan.candidates.is_empty());
}
