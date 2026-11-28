use super::*;

#[test]
fn byte_eviction_reports_a_gap_even_when_one_record_cannot_fit() {
    let mut history = History::default();
    history.push(1, &[1], json!({"opaque":"x".repeat(8*1024*1024)}), None);
    let page = history.page(0).unwrap();
    assert_eq!(page["reports"], json!([]));
    assert_eq!(page["gap"], true);
    assert_eq!(page["oldest"], "2");
    assert_eq!(page["latest"], "1");
}
#[test]
fn sequence_exhaustion_seals_history_instead_of_wrapping() {
    let mut history = History {
        sequence: u64::MAX,
        ..History::default()
    };
    history.push(1, &[1], json!({"value":1}), None);
    assert_eq!(history.sequence, u64::MAX);
    assert_eq!(history.terminal.as_ref().unwrap()["kind"], "overflow");
}
#[test]
fn serialized_report_pages_never_exceed_one_mebibyte() {
    let mut history = History::default();
    for at in 0..150 {
        history.push(at, &[1], json!({"opaque":"x".repeat(8192)}), None);
    }
    let mut after = 0;
    let mut count = 0;
    loop {
        let page = history.page(after).unwrap();
        assert!(page.to_string().len() < 1024 * 1024);
        let reports = page["reports"].as_array().unwrap();
        assert!(reports.len() <= 128);
        if reports.is_empty() {
            break;
        }
        count += reports.len();
        after = reports.last().unwrap()["sequence"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
    }
    assert_eq!(count, 150);
    assert!(history.page(151).is_err());
}

#[test]
fn a_report_on_the_page_byte_boundary_still_advances_the_cursor() {
    let mut empty = History::default();
    empty.push(1, &[0], json!({"opaque":""}), None);
    let overhead = empty.entries.back().unwrap().json.len();
    let mut history = History::default();
    history.push(
        1,
        &[0],
        json!({"opaque":"x".repeat(900*1024-overhead)}),
        None,
    );
    assert_eq!(history.entries.back().unwrap().json.len(), 900 * 1024);
    let page = history.page(0).unwrap();
    assert_eq!(page["reports"].as_array().unwrap().len(), 1);
    assert_eq!(page["next"], "1");
}

#[test]
fn a_late_get_completion_cannot_replace_a_newer_native_result() {
    let mut history = History {
        get_token: Some("new".into()),
        ..History::default()
    };
    history.retain_get("new-request", "new", json!({"wireHex":"01"}), false);
    let late = history.retain_get(
        "old-request",
        "old",
        json!({"samples":{"opaque":"x".repeat(1_000_000)}}),
        true,
    );
    assert_eq!(late["getSlice"]["unavailable"], "stale-request");
    assert!(history
        .get_slice(&json!({"requestId":"old-request"}))
        .is_err());
    assert!(history
        .get_slice(&json!({"requestId":"new-request"}))
        .is_ok());
}

#[test]
fn retained_get_results_share_the_history_byte_budget_and_expose_report_eviction() {
    let mut history = History::default();
    for at in 0..14 {
        history.push(at, &[1], json!({"opaque":"x".repeat(512*1024)}), None);
    }
    history.get_token = Some("receipt".into());
    history.retain_get(
        "request",
        "receipt",
        json!({"samples":{"opaque":"x".repeat(1_500_000)}}),
        true,
    );
    assert!(history.bytes + history.get_bytes() <= 8 * 1024 * 1024);
    assert_eq!(history.page(0).unwrap()["gap"], true);
    assert!(history.get_slice(&json!({"requestId":"request"})).is_ok());
    history.end(json!({"kind":"removed"}));
    assert!(history.get_slice(&json!({"requestId":"request"})).is_err());
}
