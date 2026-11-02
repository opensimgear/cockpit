use super::*;

fn wait_exit(child: &mut Helper) {
    if child.0.try_wait().unwrap().is_none() {
        let process = Handle::open(child.0.id()).unwrap();
        runtime().block_on(async {
            timeout(
                Duration::from_secs(10),
                wait::Exit::new(process).unwrap().wait(),
            )
            .await
            .unwrap()
            .unwrap();
        });
    }
}

#[test]
fn cancelled_or_closed_request_never_invokes_the_launcher() {
    let selected = Selected::new();
    let (sender, receiver) = watch::channel(true);
    assert!(
        start_controlled(selected.identity.clone(), Some(receiver), |_| panic!(
            "must not launch"
        ))
        .is_err()
    );
    drop(sender);
    let (sender, receiver) = watch::channel(false);
    drop(sender);
    assert!(
        start_controlled(selected.identity.clone(), Some(receiver), |_| panic!(
            "closed request must not launch"
        ))
        .is_err()
    );
}

#[test]
fn cancellation_during_blocked_launch_prevents_begin_and_factory_after_release() {
    let selected = Selected::new();
    let expected = selected.identity.clone();
    let (cancel, cancellation) = watch::channel(false);
    let (entered_sender, entered) = mpsc::sync_channel(1);
    let (release, release_receiver) = mpsc::sync_channel(1);
    let (result_sender, result) = mpsc::sync_channel(1);
    let (ordinary, child_receiver) = ordinary_launch("observe");
    let starter = std::thread::spawn(move || {
        let result = start_controlled(expected, Some(cancellation), move |request| {
            entered_sender.send(()).unwrap();
            release_receiver
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            ordinary(request)
        });
        let _ = result_sender.send(result);
    });
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    cancel.send_replace(true);
    release.send(()).unwrap();
    let mut child = Helper(
        child_receiver
            .recv_timeout(Duration::from_secs(10))
            .unwrap(),
    );
    let result = result.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(result.err().unwrap().contains("cancelled"));
    starter.join().unwrap();
    wait_exit(&mut child);
    let mut output = String::new();
    std::io::Read::read_to_string(&mut child.0.stdout.take().unwrap(), &mut output).unwrap();
    assert!(!output.contains("factory-entered"));
}

#[test]
fn connected_startup_cancel_exits_child_with_a_stalled_factory() {
    let selected = Selected::new();
    let expected = selected.identity.clone();
    let (cancel, cancellation) = watch::channel(false);
    let (ordinary, child_receiver) = ordinary_launch("stall");
    let (result_sender, result) = mpsc::sync_channel(1);
    let starter = std::thread::spawn(move || {
        let _ = result_sender.send(start_controlled(expected, Some(cancellation), ordinary));
    });
    let mut child = Helper(
        child_receiver
            .recv_timeout(Duration::from_secs(10))
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let (entered_sender, entered) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut sender = Some(entered_sender);
        for text in std::io::BufRead::lines(std::io::BufReader::new(stdout)).map_while(Result::ok) {
            if text.contains("factory-entered") {
                if let Some(sender) = sender.take() {
                    let _ = sender.send(());
                }
            }
        }
    });
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    let started = std::time::Instant::now();
    cancel.send_replace(true);
    assert!(result
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .err()
        .unwrap()
        .contains("cancelled"));
    starter.join().unwrap();
    child.wait();
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn request_sender_closure_revokes_a_ready_bridge_and_exits_its_child() {
    let selected = Selected::new();
    let (cancel, cancellation) = watch::channel(false);
    let (ordinary, child_receiver) = ordinary_launch("steady");
    let (mut failure, worker) =
        start_controlled(selected.identity.clone(), Some(cancellation), ordinary).unwrap();
    let mut child = Helper(
        child_receiver
            .recv_timeout(Duration::from_secs(10))
            .unwrap(),
    );
    drop(cancel);
    let reason = runtime().block_on(async {
        timeout(
            Duration::from_secs(10),
            super::super::super::wait_failure(&mut failure),
        )
        .await
        .unwrap()
    });
    assert!(reason.contains("cancelled"));
    drop(worker);
    child.wait();
}
