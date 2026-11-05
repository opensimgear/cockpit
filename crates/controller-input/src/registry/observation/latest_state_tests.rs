use super::test_support::wait_processed;
use super::*;
use crate::tests::fixture;

#[test]
fn slow_preview_reader_coalesces_to_current_state_without_stalling_fast_reader_or_capture_journal() {
    let (mut registry, broker, id, mut slow) = setup();
    slow.recv().unwrap();
    baseline(&mut registry, &id);
    slow.recv().unwrap();
    slow.recv().unwrap();

    let mut fast = subscribe(&mut registry, &broker, &id);
    let baseline = fast.recv().unwrap();
    assert_eq!(baseline.kind, "snapshot");

    const REPORTS: u32 = 32;
    for value in 1..=REPORTS {
        registry
            .report(&id, 1, &BTreeMap::from([(0, value % 2), (1, value)]))
            .unwrap();
        let preview = fast.recv().unwrap();
        assert_eq!(preview.kind, "snapshot");
        assert_eq!(
            preview.payload.as_ref().unwrap()["controls"][1]["value"]["native"],
            value
        );
    }

    wait_processed(&registry, &id);
    let current = slow.recv().unwrap();
    assert_eq!(current.kind, "snapshot");
    assert_eq!(
        current.payload.as_ref().unwrap()["controls"][1]["value"]["native"],
        REPORTS
    );
    assert!(slow.try_recv().unwrap().is_none(), "Unread absolute states collapse to one current snapshot.");

    let page = registry.stream_page(&registry.session, 0);
    assert_eq!(page.events.len(), REPORTS as usize + 1, "The baseline and every ordinary report remain ordered for routing.");
    assert!(page.events.windows(2).all(|pair| pair[0].sequence < pair[1].sequence));
    assert_eq!(page.events.iter().skip(1).map(|event| event.values["hid:0009:0001:1:0:0"].native)
        .collect::<Vec<_>>(), (1..=REPORTS).map(|value| (value % 2) as i64).collect::<Vec<_>>(),
        "Preview coalescing must never remove routing button transitions.");
    assert_eq!(page.events.last().unwrap().values["hid:0001:0030:1:0:1"].native, REPORTS as i64);
}

#[test]
fn unread_preview_coalescing_preserves_disconnect_and_replacement_boundaries() {
    let (mut registry, _, id, mut reader) = setup();
    reader.recv().unwrap();
    baseline(&mut registry, &id);
    reader.recv().unwrap();
    reader.recv().unwrap();

    for value in 1..=20 {
        registry
            .report(&id, 1, &BTreeMap::from([(1, value)]))
            .unwrap();
    }
    registry.disconnect(&id);
    wait_processed(&registry, &id);
    let latest = reader.recv().unwrap();
    assert_eq!(latest.kind, "snapshot");
    assert_eq!(latest.payload.as_ref().unwrap()["controls"][1]["value"]["native"], 20);
    let offline = reader.recv().unwrap();
    assert_eq!(offline.kind, "change");
    assert_eq!(offline.payload.as_ref().unwrap()["reason"], "disconnect");
    assert_eq!(offline.payload.as_ref().unwrap()["source"]["online"], false);
    assert_eq!(offline.payload.as_ref().unwrap()["values"], json!({}));
    assert!(reader.try_recv().unwrap().is_none());

    registry.connect(fixture("controller", None)).unwrap();
    let loss = reader.recv().unwrap();
    assert_eq!(loss.kind, "loss");
    assert_eq!(loss.reason.as_deref(), Some("generation-replaced"));
    let replacement = reader.recv().unwrap();
    assert_eq!(replacement.kind, "snapshot");
    assert_eq!(replacement.payload.as_ref().unwrap()["source"]["baselineReady"], false);
    assert!(replacement.payload.as_ref().unwrap()["controls"]
        .as_array()
        .unwrap()
        .iter()
        .all(|control| control["value"].is_null()));
}
