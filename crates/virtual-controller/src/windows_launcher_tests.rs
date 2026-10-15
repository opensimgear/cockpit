use super::*;
use std::{
    thread,
    time::{Duration, Instant},
};
fn request() -> Request {
    Request {
        operation: uuid::Uuid::new_v4().to_string(),
        owner: crate::windows_process::current().unwrap(),
        action: Action::Create(2),
    }
}
#[test]
fn request_arguments_are_bounded_canonical_and_cannot_inject_a_command() {
    let mut req = request();
    let args = req.arguments().unwrap();
    let args: Vec<String> = args.split(' ').map(str::to_owned).collect();
    let parsed = Request::parse(&args).unwrap();
    assert_eq!(parsed.operation, req.operation);
    assert_eq!(parsed.owner, req.owner);
    assert_eq!(parsed.action, Action::Create(2));
    req.action = Action::Recover;
    assert_eq!(
        Request::parse(
            &req.arguments()
                .unwrap()
                .split(' ')
                .map(str::to_owned)
                .collect::<Vec<_>>()
        )
        .unwrap()
        .action,
        Action::Recover
    );
    for id in [0, 17, 255] {
        req.action = Action::Create(id);
        assert!(req.arguments().is_err());
    }
    req.action = Action::Create(2);
    req.operation = "x;anything".into();
    assert!(req.arguments().is_err());
    for (index, value) in [
        (0, "other"),
        (1, "x"),
        (2, "01"),
        (2, "x"),
        (3, "01"),
        (4, "02"),
        (4, "x"),
        (4, "17"),
    ] {
        let mut bad = args.clone();
        bad[index] = value.into();
        assert!(Request::parse(&bad).is_err(), "{bad:?}");
    }
    let mut bad = args.clone();
    bad.push("extra".into());
    assert!(Request::parse(&bad).is_err());
    let mut bad = args;
    bad[1] = "x".repeat(65);
    assert!(Request::parse(&bad).is_err());
    assert!(launch(Action::Create(2), None).is_err());
    assert!(launch(Action::Recover, Some(request().operation)).is_err());
}
#[test]
fn shell_handle_observes_harmless_child_completion_without_elevation_or_driver_changes() {
    use std::os::windows::process::CommandExt;
    // ShellExecute needs an absolute path. Canonicalizing the literal "python"
    // searches the current directory, not PATH; resolve through the interpreter
    // for ordinary cargo test as well as the explicitly configured coverage run.
    let python = std::env::var("OSG_TEST_PYTHON").unwrap_or_else(|_| {
        let result = std::process::Command::new("python")
            .args(["-c", "import sys; print(sys.executable)"])
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .output()
            .expect("Python must be installed for the harmless child-process fixture");
        assert!(result.status.success());
        String::from_utf8(result.stdout).unwrap().trim().to_owned()
    });
    let python = Path::new(&python).canonicalize().unwrap();
    let child = execute(
        &python,
        "-c \"import time; time.sleep(0.1); raise SystemExit(7)\"",
        "open",
    )
    .unwrap();
    let req = request();
    let event = Cancellation::create(&req.operation).unwrap();
    let reader = Cancellation::open(&req.operation).unwrap();
    let ticket = Ticket {
        request: req,
        child,
        cancellation: std::sync::Arc::new(event),
    };
    let until = Instant::now() + Duration::from_secs(5);
    while ticket.poll().unwrap().is_none() {
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(ticket.poll().unwrap(), Some(7));
    ticket.cancel().unwrap();
    assert!(reader.cancelled().unwrap());
    assert_eq!(ticket.request.action, Action::Create(2));
    drop(ticket);
    assert!(reader.cancelled().unwrap());
    assert!(execute(Path::new("relative.exe"), "", "open").is_err());
    assert!(execute(&python, "invalid\0argument", "open").is_err());
    assert!(Child(ptr::null_mut()).poll().is_err());
}

#[test]
fn custom_layout_request_roundtrips_and_refuses_noncanonical_or_overbound_input() {
    let mut req = request();
    req.action = Action::CreateLayout {
        slot: 16,
        layout: crate::descriptor::Layout::STARTER,
    };
    let args: Vec<String> = req
        .arguments()
        .unwrap()
        .split(' ')
        .map(str::to_owned)
        .collect();
    assert_eq!(Request::parse(&args).unwrap().action, req.action);
    for bad in [
        "16:255:32:1:0",
        "016:255:32:1",
        "16:65536:32:1",
        "16:0:0:0",
        "16:255:129:1",
        "16:255:32:5",
        "17:255:32:1",
        "16:x:32:1",
        "16:255:x:1",
        "16:255:32:x",
        "x:255:32:1",
    ] {
        let mut changed = args.clone();
        changed[4] = bad.into();
        assert!(Request::parse(&changed).is_err(), "{bad}");
    }
}
