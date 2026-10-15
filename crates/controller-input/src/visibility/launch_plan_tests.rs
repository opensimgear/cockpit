use super::*;
fn node(path: &str, group: &str, id: &str, virt: bool) -> Node {
    Node {
        path: path.into(),
        group: group.into(),
        identity: id.into(),
        virtual_device: virt,
        device: 1,
        inode: 2,
        filesystem: 3,
    }
}
fn source() -> Source {
    Source {
        id: uuid::Uuid::new_v4().to_string(),
        session: uuid::Uuid::new_v4().to_string(),
        generation: 1,
        sequence: 1,
        interface: "selected".into(),
    }
}
#[test]
fn hides_every_selected_sibling_but_preserves_other_and_virtual_nodes() {
    let nodes = vec![
        node("/dev/input/event0", "usb-one", "selected", false),
        node("/dev/input/js0", "usb-one", "", false),
        node("/dev/hidraw0", "usb-one", "", false),
        node("/dev/input/event1", "usb-two", "other", false),
        node("/dev/input/event2", "virtual", "virtual", true),
    ];
    let kept = selection(&nodes, &[source()]).unwrap();
    assert_eq!(
        kept.iter().map(|n| n.path.as_str()).collect::<Vec<_>>(),
        ["/dev/input/event1", "/dev/input/event2"]
    );
    assert_eq!(selection(&nodes, &[source()]).unwrap(), kept);
    let mut bad = nodes.clone();
    bad[4].group = "usb-one".into();
    assert!(selection(&bad, &[source()]).is_err());
    let mut bad = nodes.clone();
    bad[3].identity = "selected".into();
    assert!(selection(&bad, &[source()]).is_err());
    let mut bad = nodes.clone();
    bad[0].identity.clear();
    assert!(selection(&bad, &[source()]).is_err());
    let mut bad = nodes.clone();
    bad[0].virtual_device = true;
    assert!(selection(&bad, &[source()]).is_err());
    let mut bad = nodes.clone();
    bad.push(nodes[0].clone());
    assert!(selection(&bad, &[source()]).is_err());
    assert!(selection(&vec![nodes[0].clone(); 257], &[source()]).is_err());
    assert!(selection(&nodes, &[]).is_err());
    for path in [
        "/dev/input/event",
        "/dev/input/../event1",
        "/dev/input/event1/extra",
        "/dev/bus/usb/001/001",
        "/tmp/event1",
    ] {
        assert!(!device_path(path));
    }
    let mut bad = nodes;
    bad[0].group.clear();
    assert!(selection(&bad, &[source()]).is_err());
}
#[test]
fn command_has_no_shell_or_host_device_broker_mounts() {
    let request = Request {
        launch_kind: "direct-child".into(),
        executable: "/games/app".into(),
        directory: "/games".into(),
        arguments: vec!["literal $(touch nope); --dev-bind /dev /dev".into()],
    };
    let args = arguments(
        &request,
        "/app/app",
        "/games",
        &[("/dev/input/event5".into(), 11)],
        12,
        13,
        14,
    );
    for flag in [
        "--unshare-user",
        "--disable-userns",
        "--unshare-all",
        "--die-with-parent",
        "--new-session",
        "--clearenv",
        "--seccomp",
    ] {
        assert!(args.iter().any(|a| a == flag));
    }
    assert!(args
        .windows(3)
        .any(|a| a == ["--dev-bind", "/proc/self/fd/11", "/dev/input/event5"]));
    assert!(!args
        .windows(3)
        .any(|a| a == ["--ro-bind", "/", "/"] || a == ["--dev-bind", "/dev", "/dev"]));
    assert_eq!(
        &args[args.len() - 3..],
        [
            "--",
            "/app/app",
            "literal $(touch nope); --dev-bind /dev /dev"
        ]
    );
}
