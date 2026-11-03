use super::*;
use std::{
    sync::mpsc::{self, Receiver},
    time::Duration,
};

fn watched(paths: &[PathBuf]) -> (Watch, Receiver<Result<(), String>>) {
    let (send, receive) = mpsc::channel();
    let mut watch = Watch::new(Arc::new(move |change| {
        let _ = send.send(change);
    }))
    .unwrap();
    watch.replace(paths).unwrap();
    (watch, receive)
}

fn changed(receive: &Receiver<Result<(), String>>) {
    receive
        .recv_timeout(Duration::from_secs(5))
        .expect("native source notification")
        .unwrap();
}

fn quiet(receive: &Receiver<Result<(), String>>) {
    assert!(matches!(
        receive.recv_timeout(Duration::from_millis(150)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
}

fn settled(receive: &Receiver<Result<(), String>>) {
    while let Ok(change) = receive.recv_timeout(Duration::from_millis(100)) {
        change.unwrap();
    }
}

#[test]
fn native_writes_creation_rename_and_deletion_wake() {
    let directory = crate::test_directory();
    let target = directory.path().join("selected.png");
    let (mut watch, receive) = watched(std::slice::from_ref(&target));
    fs::write(&target, b"first").unwrap();
    changed(&receive);
    watch.replace(std::slice::from_ref(&target)).unwrap();
    settled(&receive);
    fs::write(&target, b"updated").unwrap();
    changed(&receive);
    settled(&receive);
    let staged = directory.path().join("staged.png");
    fs::write(&staged, b"replacement").unwrap();
    // Windows rename cannot overwrite the destination; remove before rename.
    fs::remove_file(&target).unwrap();
    changed(&receive);
    settled(&receive);
    fs::rename(&staged, &target).unwrap();
    changed(&receive);
    watch.replace(std::slice::from_ref(&target)).unwrap();
    settled(&receive);
    fs::remove_file(&target).unwrap();
    changed(&receive);
}

#[test]
fn unrelated_database_and_artwork_files_do_not_wake() {
    let directory = crate::test_directory();
    let target = directory.path().join("selected.png");
    // App root is an exact target, not permission to invalidate every descendant.
    let (_watch, receive) = watched(&[directory.path().into(), target]);
    fs::write(directory.path().join("state.sqlite"), b"unrelated").unwrap();
    fs::write(directory.path().join("other.png"), b"unrelated").unwrap();
    quiet(&receive);
}

#[test]
fn deleted_and_recreated_parent_is_rearmed_before_next_baseline() {
    let directory = crate::test_directory();
    let parent = directory.path().join("artwork");
    fs::create_dir(&parent).unwrap();
    let target = parent.join("selected.png");
    let paths = vec![parent.clone(), target.clone()];
    let (mut watch, receive) = watched(&paths);
    fs::remove_dir(&parent).unwrap();
    changed(&receive);
    watch.replace(&paths).unwrap();
    settled(&receive);
    fs::create_dir(&parent).unwrap();
    changed(&receive);
    watch.replace(&paths).unwrap();
    settled(&receive);
    fs::write(target, b"recreated").unwrap();
    changed(&receive);
}

#[test]
fn missing_nested_parents_advance_from_nearest_existing_ancestor() {
    let directory = crate::test_directory();
    let first = directory.path().join("generated");
    let second = first.join("images");
    let target = second.join("selected.png");
    let (mut watch, receive) = watched(std::slice::from_ref(&target));
    fs::create_dir(&first).unwrap();
    changed(&receive);
    watch.replace(std::slice::from_ref(&target)).unwrap();
    settled(&receive);
    fs::create_dir(&second).unwrap();
    changed(&receive);
    watch.replace(std::slice::from_ref(&target)).unwrap();
    settled(&receive);
    fs::write(target, b"new").unwrap();
    changed(&receive);
}

#[test]
fn replacement_drops_old_targets_and_empty_configuration_stops_changes() {
    let directory = crate::test_directory();
    let old = directory.path().join("old.png");
    let new = directory.path().join("new.png");
    let (mut watch, receive) = watched(std::slice::from_ref(&old));
    assert!(watch.replace(&[PathBuf::from("relative.png")]).is_err());
    fs::write(&old, b"retained after failed configuration").unwrap();
    changed(&receive);
    settled(&receive);
    watch.replace(std::slice::from_ref(&new)).unwrap();
    fs::write(&old, b"old").unwrap();
    quiet(&receive);
    fs::write(&new, b"new").unwrap();
    changed(&receive);
    watch.replace(&[]).unwrap();
    // Drain native changes already delivered before replacement completed.
    while receive.try_recv().is_ok() {}
    fs::write(new, b"changed again").unwrap();
    quiet(&receive);
}

#[test]
fn configuration_rejects_invalid_paths_and_explicit_bounds_without_truncation() {
    let directory = crate::test_directory();
    assert!(configuration(&[PathBuf::from("relative.png")]).is_err());
    assert!(configuration(&[directory.path().join("../escape.png")]).is_err());
    assert!(
        configuration(&vec![directory.path().into(); MAX_TARGETS + 1])
            .unwrap_err()
            .contains("target limit")
    );
    let paths = (0..MAX_DIRECTORIES + 1)
        .map(|index| {
            let parent = directory.path().join(format!("art-{index}"));
            fs::create_dir(&parent).unwrap();
            parent.join("selected.png")
        })
        .collect::<Vec<_>>();
    assert!(configuration(&paths)
        .unwrap_err()
        .contains("directory limit"));
    let file = directory.path().join("file");
    fs::write(&file, b"not a folder").unwrap();
    assert!(configuration(&[file.join("selected.png")]).is_err());
}

#[test]
fn callback_reports_loss_ignores_access_and_releases_filter_lock_before_waking() {
    use notify::event::{AccessKind, CreateKind, Flag};
    let directory = crate::test_directory();
    let target = directory.path().join("selected.png");
    let targets = Arc::new(Mutex::new(Arc::new(TargetFilter {
        paths: BTreeSet::from([comparison_path(&target)]),
        directories: BTreeSet::new(),
    })));
    let (send, receive) = mpsc::channel();
    let callback_targets = targets.clone();
    let callback: Changed = Arc::new(move |change| {
        assert!(callback_targets.try_lock().is_ok());
        send.send(change).unwrap();
    });
    dispatch(
        Ok(Event::new(EventKind::Access(AccessKind::Any)).add_path(target.clone())),
        &targets,
        &callback,
    );
    assert!(receive.try_recv().is_err());
    dispatch(
        Ok(Event::new(EventKind::Create(CreateKind::File)).add_path(target)),
        &targets,
        &callback,
    );
    assert!(receive.try_recv().unwrap().is_ok());
    dispatch(
        Ok(Event::new(EventKind::Any).set_flag(Flag::Rescan)),
        &targets,
        &callback,
    );
    assert!(receive.try_recv().unwrap().unwrap_err().contains("lost"));
    dispatch(
        Err(notify::Error::generic(&"é".repeat(2000))),
        &targets,
        &callback,
    );
    let message = receive.try_recv().unwrap().unwrap_err();
    assert!(message.len() <= MAX_ERROR_BYTES);
    assert!(message.contains("lost"));
}

#[test]
fn directory_content_changes_are_ignored_but_files_and_parent_replacements_wake() {
    use notify::event::{CreateKind, DataChange, RemoveKind, RenameMode};
    let root = crate::test_directory();
    let missing_parent = root.path().join("generated");
    let file = missing_parent.join("selected.png");
    let (filter, _) =
        configuration(&[root.path().into(), missing_parent.clone(), file.clone()]).unwrap();
    let targets = Arc::new(Mutex::new(Arc::new(filter)));
    let (send, receive) = mpsc::channel();
    let callback: Changed = Arc::new(move |change| {
        send.send(change).unwrap();
    });
    let content_changes = [
        ModifyKind::Any,
        ModifyKind::Data(DataChange::Any),
        ModifyKind::Metadata(MetadataKind::WriteTime),
    ];
    for kind in content_changes {
        for parent in [root.path(), missing_parent.as_path()] {
            dispatch(
                Ok(Event::new(EventKind::Modify(kind)).add_path(parent.into())),
                &targets,
                &callback,
            );
            assert!(
                receive.try_recv().is_err(),
                "directory modification must not wake: {kind:?}"
            );
        }
        dispatch(
            Ok(Event::new(EventKind::Modify(kind)).add_path(file.clone())),
            &targets,
            &callback,
        );
        assert!(
            receive.try_recv().unwrap().is_ok(),
            "selected file modification must wake: {kind:?}"
        );
    }
    for kind in [
        EventKind::Create(CreateKind::Folder),
        EventKind::Remove(RemoveKind::Folder),
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
    ] {
        for parent in [root.path(), missing_parent.as_path()] {
            dispatch(
                Ok(Event::new(kind).add_path(parent.into())),
                &targets,
                &callback,
            );
            assert!(
                receive.try_recv().unwrap().is_ok(),
                "parent replacement must wake: {kind:?}"
            );
        }
    }
}

#[cfg(windows)]
#[test]
fn windows_native_and_canonical_prefixes_and_case_compare_equally() {
    assert_eq!(
        comparison_path(Path::new(r"C:\Artwork\Selected.PNG")),
        comparison_path(Path::new(r"\\?\c:\artwork\selected.png"))
    );
    assert_eq!(
        comparison_path(Path::new(r"\\server\Share\Selected.PNG")),
        comparison_path(Path::new(r"\\?\UNC\SERVER\share\selected.png"))
    );
}
