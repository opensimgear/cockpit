use super::*;

fn process() -> ProcessIdentity {
    ProcessIdentity {
        executable: "C:\\Fixture\\Game.exe".into(),
        pid: 42,
        created: "123".into(),
    }
}

#[test]
fn missing_foreground_window_is_an_ineligible_scope_without_process_inspection() {
    let observed = foreground_identity_for(ptr::null_mut()).unwrap();
    assert!(observed.is_none());
    for scope in [
        Scope::DesktopWide,
        Scope::Application {
            identity: process(),
        },
    ] {
        assert!(
            !eligible_foreground(&scope, Ok(observed.clone()), |_| panic!(
                "No foreground window means there is no process to inspect"
            ))
            .unwrap()
        );
    }
}

#[test]
fn actual_identity_access_and_integrity_failures_remain_errors() {
    let error = "Cannot read process executable: access denied";
    assert_eq!(
        eligible_foreground(&Scope::DesktopWide, Err(error.into()), |_| panic!(
            "Identity observation failed first"
        ))
        .unwrap_err(),
        error
    );
    let error = "Foreground application has higher integrity; run the target unelevated.";
    assert_eq!(
        eligible_foreground(&Scope::DesktopWide, Ok(Some(process())), |_| Err(
            error.into()
        ))
        .unwrap_err(),
        error
    );
}

#[test]
fn foreground_identity_still_has_to_match_the_exact_application() {
    let process = process();
    assert!(
        eligible_foreground(&Scope::DesktopWide, Ok(Some(process.clone())), |_| Ok(())).unwrap()
    );
    let scope = Scope::Application {
        identity: process.clone(),
    };
    assert!(eligible_foreground(&scope, Ok(Some(process.clone())), |_| Ok(())).unwrap());
    let mut replacement = process;
    replacement.created = "456".into();
    assert!(!eligible_foreground(&scope, Ok(Some(replacement)), |_| Ok(())).unwrap());
}
