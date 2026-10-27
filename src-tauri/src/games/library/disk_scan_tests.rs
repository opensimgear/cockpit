use super::*;
fn exe(root:&Path,relative:&str) {
    let file=root.join(relative);fs::create_dir_all(file.parent().unwrap()).unwrap();fs::write(file,b"fixture").unwrap();
}
fn candidates(roots:&[PathBuf],occupied:&[PathBuf])->Scan {
    let mut result=Scan::default();scan(roots,occupied,&mut result);result
}

#[test]
fn only_explicit_container_children_become_games_with_nested_runtime_lookup() {
    let directory=crate::test_directory();let root=directory.path();
    exe(root,"RootUtility.exe");exe(root,"Racer/bin/win64/Racer.exe");exe(root,"Racer/bin/win32/Helper.exe");
    fs::create_dir_all(root.join("Empty folder")).unwrap();
    assert!(candidates(&[],&[]).candidates.is_empty());
    let found=candidates(&[root.to_owned()],&[]);
    assert_eq!(found.candidates.len(),1);assert_eq!(found.candidates[0].name,"Racer");
    assert_eq!(found.candidates[0].executable_candidates.len(),2);
    assert!(found.candidates[0].install_path.ends_with("Racer"));
}

#[test]
fn program_files_skips_launcher_trees_and_used_installations_without_hiding_neighbor_games() {
    let directory=crate::test_directory();let root=directory.path();
    exe(root,"Steam/steam.exe");exe(root,"Steam/steamapps/common/SteamVR/bin/win64/vrurlhandler.exe");
    exe(root,"Used game/bin/Used.exe");exe(root,"Standalone/bin/Standalone.exe");exe(root,"SteamBackup/Backup.exe");
    let occupied=vec![root.join("Steam"),root.join("Used game")];
    let found=candidates(&[root.to_owned()],&occupied);
    let names=found.candidates.iter().map(|game|game.name.as_str()).collect::<BTreeSet<_>>();
    assert_eq!(names,BTreeSet::from(["Standalone","SteamBackup"]));
    // A source record nested inside a child also reserves that whole branch.
    let nested=candidates(&[root.to_owned()],&[root.join("Steam/steamapps/common/SteamVR"),root.join("Used game")]);
    assert_eq!(nested.candidates.len(),2);
    assert!(candidates(&[root.join("Steam/steamapps/common")],&occupied).candidates.is_empty());
}

#[test]
fn duplicate_overlapping_roots_do_not_promote_bin_folders_to_extra_games() {
    let directory=crate::test_directory();let root=directory.path();exe(root,"Racer/bin/win64/Racer.exe");
    let found=candidates(&[root.join("Racer"),root.to_owned(),root.to_owned()],&[]);
    assert_eq!(found.candidates.len(),1);assert_eq!(found.candidates[0].name,"Racer");
    assert!(candidates(&[root.join("Racer/bin/win64/Racer.exe")],&[]).candidates.is_empty());
}
