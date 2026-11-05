use super::*;

#[test]
fn activity_wakes_for_metadata_commits_but_not_raw_telemetry() {
    let signal = Signal::default();
    let wake = Arc::new(osg_streams::NativeWake::default());
    let initial = wake.observed();
    signal.watch_activity(&wake);
    assert_ne!(wake.observed(), initial);
    assert_eq!(Arc::strong_count(&wake), 1);
    for domain in [Domain::Library, Domain::Discovery, Domain::Runtime, Domain::Artwork, Domain::Setup, Domain::Permissions] {
        let before = wake.observed();
        signal.mark(domain);
        assert_ne!(wake.observed(), before);
    }
    let before = wake.observed();
    signal.mark_telemetry();
    signal.take_telemetry();
    signal.mark_telemetry();
    assert_eq!(wake.observed(), before);
    drop(wake);
    signal.mark(Domain::Runtime);
    assert_eq!(signal.activity_wake.lock().unwrap().weak_count(), 0);
}

#[test]
fn activity_job_duplicates_stay_quiet_and_terminal_overflow_and_close_wake() {
    let signal = Signal::default();
    let wake = Arc::new(osg_streams::NativeWake::default());
    signal.watch_activity(&wake);
    signal.start_job(Domain::Discovery, "scan", "run", "running", "start", None);
    let before = wake.observed();
    signal.start_job(Domain::Discovery, "scan", "run", "running", "start", None);
    signal.job(Domain::Discovery, "scan", "old", "running", "late", None, false);
    assert_eq!(wake.observed(), before);
    signal.job(Domain::Discovery, "scan", "run", "ready", "done", None, true);
    assert_ne!(wake.observed(), before);
    let before = wake.observed();
    signal.job(Domain::Discovery, "scan", "run", "ready", "done", None, true);
    assert_eq!(wake.observed(), before);
    signal.start_job(Domain::Discovery, "scan", "next", "running", "start", None);
    for index in 0..MAX_EVENTS {
        let before = wake.observed();
        signal.job(Domain::Discovery, "scan", "next", "running", &index.to_string(), None, false);
        assert_ne!(wake.observed(), before);
    }
    assert!(signal.state.lock().unwrap().overflow);
    let before = wake.observed();
    signal.close();
    assert_ne!(wake.observed(), before);
}

#[test]
fn activity_revision_and_active_job_exhaustion_wake() {
    for revision_exhausted in [true, false] {
        let signal = Signal::default();
        let wake = Arc::new(osg_streams::NativeWake::default());
        signal.watch_activity(&wake);
        if revision_exhausted {
            signal.state.lock().unwrap().revision = u64::MAX;
        } else {
            for index in 0..MAX_JOBS {
                signal.start_job(Domain::Setup, &format!("setup:{index}"), "run", "running", "start", None);
            }
        }
        let before = wake.observed();
        if revision_exhausted { signal.mark(Domain::Runtime); }
        else { signal.start_job(Domain::Setup, "overflow", "run", "running", "start", None); }
        assert_ne!(wake.observed(), before);
        assert!(signal.take(false).is_err());
    }
}
