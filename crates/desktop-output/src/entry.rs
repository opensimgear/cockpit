//! Private same-executable mode. Ordinary startup never opens an injector.
use std::io::Write;
pub const ARGUMENT: &str = "--osg-desktop-input-worker";
pub fn helper_main() -> Option<i32> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 1 || args[0] != ARGUMENT {
        return None;
    }
    Some(run())
}
fn failed(error: &str) -> i32 {
    let _ = writeln!(
        std::io::stdout(),
        "{}",
        serde_json::json!({"protocol":crate::protocol::PROTOCOL,"backend":"desktop-input","ok":false,"error":error.chars().take(512).collect::<String>()})
    );
    1
}
fn run() -> i32 {
    crate::audit::initialize(crate::audit::Role::Worker);
    let result = run_inner();
    // Drain diagnostics only after service ownership cleanup and its ACK writer
    // have completed. This bounded wait grants no additional input authority.
    let report = crate::audit::finish(
        if result == 0 { crate::audit::ExitReason::Normal } else { crate::audit::ExitReason::Error },
        std::time::Duration::from_millis(100),
    );
    audit_exit_code(result, &report)
}
pub(crate) fn audit_exit_code(result: i32, report: &crate::audit::FlushReport) -> i32 {
    if result == 0 && report.configured && (!report.complete || !report.valid || !report.finished) {
        87
    } else {
        result
    }
}
fn run_inner() -> i32 {
    if !crate::qualification_gate() {
        return failed(crate::QUALIFICATION_REASON);
    }
    #[cfg(windows)]
    {
        match crate::windows::Native::open() {
            Ok(adapter) => match crate::worker::serve(
                adapter,
                std::io::BufReader::new(std::io::stdin()),
                std::io::stdout(),
                crate::windows::now_us,
            ) {
                Ok(()) => 0,
                Err(_) => 1,
            },
            Err(error) => failed(&error),
        }
    }
    #[cfg(not(windows))]
    {
        failed("Native desktop output is supported on Windows only.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_audit_completion_failure_changes_only_an_otherwise_successful_exit() {
        let clean = crate::audit::FlushReport {
            configured: true, enabled: true, complete: true, valid: true, finished: true,
            ..Default::default()
        };
        assert_eq!(audit_exit_code(0, &clean), 0);
        for field in 0..3 {
            let mut incomplete = clean.clone();
            match field { 0 => incomplete.complete = false, 1 => incomplete.valid = false,
                _ => incomplete.finished = false }
            assert_eq!(audit_exit_code(0, &incomplete), 87);
            for original in [1, 86, -1] {
                assert_eq!(audit_exit_code(original, &incomplete), original);
            }
            incomplete.configured = false;
            assert_eq!(audit_exit_code(0, &incomplete), 0);
        }
        assert_eq!(audit_exit_code(0, &crate::audit::FlushReport::default()), 0);
        assert_eq!(audit_exit_code(0, &crate::audit::FlushReport {
            configured: true, ..Default::default()
        }), 87);
    }
}
