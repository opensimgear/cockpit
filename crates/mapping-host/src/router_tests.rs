use super::*;
use osg_mapping::engine::{ControlAssignment, ReportReason};
fn fixture(physical: bool) -> (Profile, Vec<Assignment>, stream::Snapshot) {
    let mut profile =
        Profile::parse(include_bytes!("../../mapping/fixtures/direct-profile.json")).unwrap();
    if physical {
        profile.roles[0].origin = Origin::HostController;
    }
    let assignments = profile
        .roles
        .iter()
        .map(|r| Assignment {
            role: r.id.clone(),
            source: Source {
                origin: r.origin,
                id: r.id.clone(),
            },
            controls: r
                .controls
                .iter()
                .map(|c| ControlAssignment {
                    control: c.id.clone(),
                    source_control: c.id.clone(),
                })
                .collect(),
        })
        .collect();
    let mut snapshot = stream::Snapshot {
        session: "capture-session".into(),
        cursor: 1,
        captured_us: 10,
        sources: vec![],
    };
    if physical {
        let mut axis = Control::new(1, 0x30, 0, 0, 1);
        axis.id = "x".into();
        axis.min = -32768;
        axis.max = 32767;
        let mut button = Control::new(9, 1, 0, 0, 2);
        button.id = "trigger".into();
        let mut hat = Control::new(1, 0x39, 0, 0, 3);
        hat.id = "hat".into();
        snapshot.sources.push(stream::SourceSnapshot {
            id: "left".into(),
            controls: vec![axis, button, hat],
            baseline: event(1, 1, stream::Reason::Baseline, false),
        });
    }
    (profile, assignments, snapshot)
}
fn event(generation: u64, sequence: u64, reason: stream::Reason, pressed: bool) -> stream::Event {
    stream::Event {
        cursor: sequence,
        source: "left".into(),
        generation,
        sequence,
        captured_us: generation * 100 + sequence,
        reason,
        values: BTreeMap::from([
            (
                "trigger".into(),
                Sample {
                    native: pressed as i64,
                    normalized: Some(pressed as i64),
                },
            ),
            (
                "x".into(),
                Sample {
                    native: 123,
                    normalized: Some(123),
                },
            ),
            (
                "hat".into(),
                Sample {
                    native: 8,
                    normalized: None,
                },
            ),
        ]),
    }
}
fn page(events: Vec<stream::Event>) -> stream::Page {
    stream::Page {
        session: "capture-session".into(),
        cursor: 100,
        captured_us: 1000,
        gap: false,
        events,
    }
}
#[test]
fn physical_edges_are_preserved_and_quiet_sources_keep_their_state() {
    let (profile, assignments, snapshot) = fixture(true);
    let mut r = Router::default();
    r.start(profile, assignments, snapshot).unwrap();
    assert_eq!(r.status()["ready"], true);
    let before = r.runtime.recording.cursor();
    r.pump(page(vec![
        event(1, 2, stream::Reason::Change, true),
        event(1, 3, stream::Reason::Change, false),
        event(1, 4, stream::Reason::Change, true),
    ]))
    .unwrap();
    let records = r.runtime.recording.page(before, 32).unwrap().records;
    let buttons: Vec<bool> = records
        .iter()
        .filter(|x| x.report.target == "main")
        .map(|x| x.report.state.buttons[0])
        .collect();
    assert_eq!(buttons, vec![true, false, true]);
    let cursor = r.runtime.recording.cursor();
    r.pump(page(vec![])).unwrap();
    assert_eq!(r.runtime.recording.cursor(), cursor);
    assert_eq!(r.cursor(), ("capture-session", 100));
    // Source loss releases just that source; an explicit fresh baseline can recover.
    r.pump(page(vec![event(2, 0, stream::Reason::Gap, false)]))
        .unwrap();
    assert_eq!(r.status()["ready"], false);
    r.pump(page(vec![event(2, 1, stream::Reason::Baseline, true)]))
        .unwrap();
    assert_eq!(r.status()["ready"], true);
    let records = r.runtime.recording.page(cursor, 32).unwrap().records;
    assert!(records
        .iter()
        .any(|x| x.report.reason == ReportReason::SourceLost));
    assert!(
        !records
            .iter()
            .filter(|x| x.report.target == "main")
            .last()
            .unwrap()
            .report
            .state
            .buttons[0]
    );
    r.pump(page(vec![event(2, 2, stream::Reason::Disconnect, false)]))
        .unwrap();
    r.stop();
    assert_eq!(r.status()["running"], false);
}
#[test]
fn overflow_stale_capture_and_invalid_values_fail_closed() {
    for fault in 0..6 {
        let (p, a, s) = fixture(true);
        let mut r = Router::default();
        r.start(p, a, s).unwrap();
        let mut p = page(vec![event(1, 2, stream::Reason::Change, true)]);
        match fault {
            0 => p.gap = true,
            1 => p.session = "old-session".into(),
            2 => p.cursor = 0,
            3 => p.captured_us = 1,
            4 => {
                p.events[0].values.remove("x");
            }
            _ => p.events[0].sequence = 4,
        }
        assert!(r.pump(p).is_err());
        assert_eq!(r.status()["running"], false);
        assert!(r.status()["fault"].is_string());
        r.pump(page(vec![])).unwrap();
    }
    assert!(value(
        Kind::Button,
        &Sample {
            native: 2,
            normalized: Some(2)
        }
    )
    .is_err());
    assert!(value(
        Kind::Axis {
            range: AxisRange::Unipolar
        },
        &Sample {
            native: -1,
            normalized: Some(-1)
        }
    )
    .is_err());
    assert!(value(
        Kind::Axis {
            range: AxisRange::Bipolar
        },
        &Sample {
            native: 0,
            normalized: None
        }
    )
    .is_err());
    assert!(value(
        Kind::Hat,
        &Sample {
            native: 300,
            normalized: Some(300)
        }
    )
    .is_err());
    assert!(value(
        Kind::Hat,
        &Sample {
            native: 8,
            normalized: Some(8)
        }
    )
    .is_err());
    let mut diagnostic = Control::new(0xffff, 1, 0, 0, 1);
    assert_eq!(native_kind(&diagnostic), None);
    diagnostic.kind = NativeKind::Axis;
    assert_eq!(
        native_kind(&diagnostic),
        Some(Kind::Axis {
            range: AxisRange::Unipolar
        })
    );
}
#[test]
fn invalid_replacements_preserve_the_active_run_and_synthetic_is_explicit() {
    let (p, a, s) = fixture(false);
    let mut r = Router::default();
    let token = r.start(p, a, s).unwrap();
    let source = Source {
        origin: Origin::Synthetic,
        id: "left".into(),
    };
    r.simulate(
        &token,
        &source,
        vec![Change {
            control: "trigger".into(),
            value: Value::Button(true),
        }],
    )
    .unwrap();
    let mut stale = token.clone();
    stale.generation = Counter(8);
    assert!(r.simulate(&stale, &source, vec![]).is_err());
    assert!(r
        .simulate(
            &token,
            &Source {
                origin: Origin::HostController,
                id: "left".into()
            },
            vec![]
        )
        .is_err());
    assert!(r
        .simulate(
            &token,
            &source,
            vec![Change {
                control: "x".into(),
                value: Value::Button(true)
            }]
        )
        .is_err());
    for fault in 0..6 {
        let (mut p, a, mut s) = fixture(true);
        match fault {
            0 => s.sources.clear(),
            1 => s.session.clear(),
            2 => s.sources[0].controls.clear(),
            3 => s.sources[0].baseline.sequence = 0,
            4 => {
                s.sources[0].baseline.values.clear();
            }
            _ => p.roles[0].origin = Origin::Synthetic,
        }
        assert!(r.start(p, a, s).is_err());
        assert_eq!(r.runtime.token().unwrap(), token);
    }
    let (mut p, mut a, s) = fixture(false);
    p.roles[0].origin = Origin::OsgPanelEvents;
    a[0].source.origin = Origin::OsgPanelEvents;
    assert!(r.start(p, a, s).is_err());
    let (p, a, s) = fixture(false);
    let next = r.start(p, a, s).unwrap();
    assert_ne!(next, token);
    r.pump(page(vec![event(1, 2, stream::Reason::Change, true)]))
        .unwrap(); // unassigned physical ignored
    assert!(r.simulate(&token, &source, vec![]).is_err());
}

#[test]
fn panel_subscription_preserves_edges_and_overflow_releases_only_its_source() {
    let (mut p, mut a, snapshot) = fixture(true);
    p.roles[0].origin = Origin::OsgPanelEvents;
    a[0].source.origin = Origin::OsgPanelEvents;
    let mut router = Router::default();
    assert!(router
        .start(
            p.clone(),
            a.clone(),
            stream::Snapshot {
                session: snapshot.session.clone(),
                cursor: snapshot.cursor,
                captured_us: snapshot.captured_us,
                sources: snapshot.sources.clone()
            }
        )
        .is_err());
    let (feed, sender) = crate::panels::Feed::channel();
    let overflow = feed.overflow.clone();
    let stop = feed.stop.clone();
    router
        .start_with_panels(p, a.clone(), snapshot, BTreeMap::new(), Some(feed))
        .unwrap();
    for (sequence, pressed) in [(2, true), (3, false), (4, true)] {
        sender
            .send(Frame {
                source: a[0].source.clone(),
                generation: Counter(1),
                sequence: Counter(sequence),
                captured_us: Counter(110 + sequence),
                reason: engine::Reason::Change,
                changes: vec![
                    Change {
                        control: "trigger".into(),
                        value: Value::Button(pressed),
                    },
                    Change {
                        control: "x".into(),
                        value: Value::Axis(0),
                    },
                    Change {
                        control: "hat".into(),
                        value: Value::Hat(None),
                    },
                ],
            })
            .unwrap();
    }
    router.pump(page(vec![])).unwrap();
    assert!(router
        .runtime
        .recording
        .page(Counter(0), 32)
        .unwrap()
        .records
        .iter()
        .any(|r| r.report.state.buttons[0]));
    overflow.store(true, std::sync::atomic::Ordering::Release);
    router.pump(page(vec![])).unwrap();
    assert_eq!(router.status()["running"], true);
    assert_eq!(router.status()["ready"], false);
    assert!(stop.load(std::sync::atomic::Ordering::Acquire));
    router.stop();
}
