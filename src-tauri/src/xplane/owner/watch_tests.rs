use super::*;

#[test]
fn passive_watch_reports_native_commits_and_expiry_without_retaining_a_retired_reader() {
    let (server, mut source) = fixture();
    let wake = Arc::new(osg_streams::NativeWake::default());
    wake.bind_thread(thread::current()).unwrap();
    let watch = source.watch(wake.clone()).unwrap();
    let initial = wake.observed();
    let indices = requests(&server, GUARDS.len() + PATH_BYTES + 1);
    send(&server, &source, &indices, 1., 0.);
    observed(&mut source, |snapshot| snapshot.capture_sequence == 1);
    send(&server, &source, &indices, 2., 1.);
    let live = observed(&mut source, |snapshot| snapshot.capture_sequence == 2 && snapshot.fresh);
    assert!(wake.observed() > initial);
    let live_epoch = wake.observed();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let observed = wake.observed();
        let snapshot = source.native_snapshot().unwrap();
        if !snapshot.fresh {
            assert!(wake.observed() > live_epoch);
            assert_eq!(snapshot.capture_sequence, live.capture_sequence);
            assert_eq!(snapshot.captured_at, live.captured_at);
            break;
        }
        assert!(Instant::now() < deadline, "Native expiry did not notify its passive observer");
        wake.wait(observed, deadline.saturating_duration_since(Instant::now()));
    }
    source.retire_test();
    assert!(source.watch(wake).is_err());
    drop(source);
    assert!(watch.is_closed(), "A passive watch must not retain the producer");
}
