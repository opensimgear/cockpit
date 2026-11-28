use super::*;

#[test]
fn button_count_is_only_available_with_v2_while_value_count_is_unchanged() {
    basic();
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.caps.NumberInputButtonCaps = 1;
        s.buttons[0] = vec![HIDP_BUTTON_CAPS {
            ReportID: 1,
            ReportCount: 37,
            ..Default::default()
        }];
    });
    for version in [Some(1), Some(2), Some(3), None] {
        STATE.with(|s| s.borrow_mut().version = version);
        let snapshot = unsafe { snapshot_with(&api(), 1) };
        let supported = version.is_some_and(|v| v >= 2);
        assert_eq!(snapshot.api_version.number, version);
        assert_eq!(
            snapshot.reports[0].button_caps[0].report_count,
            supported.then_some(37)
        );
        assert_eq!(snapshot.reports[0].value_caps[0].report_count, 1);
        let json = serde_json::to_value(&snapshot).unwrap();
        if supported {
            assert_eq!(json["reports"][0]["buttonCaps"][0]["reportCount"], 37);
        } else {
            assert!(json["reports"][0]["buttonCaps"][0]["reportCount"].is_null());
        }
        assert_eq!(json["reports"][0]["valueCaps"][0]["reportCount"], 1);
        if version.is_none() {
            assert!(snapshot.failures.iter().any(|f| f.stage == "api-version"));
        }
    }
}
