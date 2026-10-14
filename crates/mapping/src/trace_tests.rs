use super::*;
use crate::{engine::ReportReason, fixtures::*};
#[test]
fn authored_trace_preserves_edges_and_replays_identically() {
    let trace = Trace::parse(include_bytes!("../fixtures/direct-trace.json")).unwrap();
    let reports = trace.replay().unwrap();
    assert_eq!(reports, trace.replay().unwrap());
    assert_eq!(reports.len(), 11);
    let main: Vec<_> = reports
        .iter()
        .filter(|r| r.report.target == "main")
        .map(|r| {
            (
                r.report.at_us.0,
                r.report.state.buttons[0],
                r.report.state.axes[0],
                r.report.state.hats[0],
                r.report.reason,
            )
        })
        .collect();
    assert_eq!(
        main,
        vec![
            (0, false, -123, None, ReportReason::Start),
            (0, false, 10, Some(1), ReportReason::Baseline),
            (10, true, 10, Some(1), ReportReason::Change),
            (20, false, 10, Some(1), ReportReason::Change),
            (30_000, true, 10, Some(1), ReportReason::Timer),
            (60_000, false, 10, Some(1), ReportReason::Timer),
            (100_000, false, -123, None, ReportReason::Stop)
        ]
    );
    assert_eq!(reports.last().unwrap().report.target, "aux");
}
#[test]
fn malformed_replay_and_replacement_are_explicit() {
    assert!(Trace::parse(&vec![b' '; TRACE_LIMIT + 1]).is_err());
    assert!(Trace::parse(b"{}").is_err());
    let mut trace = Trace {
        schema: 1,
        profile: profile(),
        assignments: assignments(),
        steps: vec![
            Step::Replace {
                profile: profile(),
                assignments: assignments(),
            },
            Step::Stop,
        ],
    };
    assert_eq!(trace.replay().unwrap().len(), 8);
    trace.schema = 2;
    assert!(trace.replay().is_err());
    assert!(Trace::parse(&serde_json::to_vec(&trace).unwrap()).is_err());
    trace.schema = 1;
    trace.steps = vec![Step::Stop; 4097];
    assert!(trace.replay().is_err());
    assert!(Trace::parse(&serde_json::to_vec(&trace).unwrap()).is_err());
    trace.steps = vec![Step::Timer {
        node: "left-trigger".into(),
        id: "unarmed".into(),
        at: Counter(30_000),
        value: Value::Button(true),
    }];
    assert!(trace.replay().is_err());
    trace.steps = vec![
        Step::Stop,
        Step::Input {
            frame: baseline("left", 1, false),
        },
    ];
    assert!(trace.replay().is_err());
}
