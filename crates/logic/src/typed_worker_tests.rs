use super::*;
use crate::typed_test_support::*;
fn completed(request: &Invocation) -> Reply {
    Reply::Completed {
        ticket: request.ticket.clone(),
        result: Ok(ScriptTransaction::default()),
    }
}
#[test]
fn absolute_typed_deadline_rejects_queued_and_mid_admission_success() {
    let request = request(true);
    let definition = definition();
    let start = Instant::now();
    for crossed in [false, true] {
        let (tx, rx) = mpsc::sync_channel(1);
        tx.send(Ok(completed(&request))).unwrap();
        let mut clock = [
            if crossed {
                start
            } else {
                start + RESPONSE_DEADLINE
            },
            start + RESPONSE_DEADLINE,
        ]
        .into_iter();
        assert_eq!(
            response(&rx, &request, &definition, start, || clock.next().unwrap())
                .unwrap()
                .unwrap_err(),
            RESPONSE_TIMEOUT
        );
        assert_eq!(rx.try_recv().is_ok(), !crossed);
    }
}
#[test]
fn typed_reply_admission_never_accepts_wrong_tokens_or_malformed_outputs() {
    let request = request(true);
    let definition = definition();
    let start = Instant::now();
    let mut wrong = request.ticket.clone();
    wrong.sequence.0 += 1;
    for reply in [
        Ok(Reply::Completed {
            ticket: wrong,
            result: Ok(Default::default()),
        }),
        Ok(Reply::Compiled),
        Err("decode failed".into()),
        Ok(Reply::Completed {
            ticket: request.ticket.clone(),
            result: Ok(ScriptTransaction {
                outputs: std::collections::BTreeMap::from([(
                    "unknown".into(),
                    osg_mapping::program::SignalValue::Scalar(
                        osg_mapping::signal::PortValue::Boolean(true),
                    ),
                )]),
                ..Default::default()
            }),
        }),
        Ok(Reply::Completed {
            ticket: request.ticket.clone(),
            result: Err("evaluation failed".into()),
        }),
    ] {
        let (tx, rx) = mpsc::sync_channel(1);
        tx.send(reply).unwrap();
        assert!(response(&rx, &request, &definition, start, || start)
            .unwrap()
            .is_err());
    }
    let (tx, rx) = mpsc::sync_channel(1);
    assert!(response(&rx, &request, &definition, start, || start).is_none());
    drop(tx);
    assert!(response(&rx, &request, &definition, start, || start)
        .unwrap()
        .is_err());
    let (tx, rx) = mpsc::sync_channel(1);
    tx.send(Ok(completed(&request))).unwrap();
    assert!(response(&rx, &request, &definition, start, || start)
        .unwrap()
        .is_ok());
    let _: osg_mapping::engine::typed::ScriptInvocation = invocation(request);
}
