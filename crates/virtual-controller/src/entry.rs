//! Private subprocess entry point. The application qualification gate is checked
//! before constructing a live session; ordinary startup never loads the SDK.
use crate::session::Factory;
#[cfg(any(windows, target_os = "linux", test))]
use crate::vjoy::Driver;
use std::io::{self, Write};
#[cfg(any(windows, target_os = "linux", test))]
use std::io::{BufRead, BufReader};
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub const ARGUMENT: &str = "--osg-vjoy-feeder";
#[cfg(target_os = "linux")]
pub const ARGUMENT: &str = "--osg-uinput-feeder";
#[cfg(target_os = "macos")]
pub const ARGUMENT: &str = crate::apple_helper::ARGUMENT;
pub const BACKEND: &str = if cfg!(target_os = "linux") {
    "uinput"
} else if cfg!(target_os = "macos") {
    "macos-hid"
} else {
    "vjoy"
};
#[cfg(not(target_os = "macos"))]
use {
    crate::{session::Channel, supervisor::Worker},
    std::{sync::Arc, time::Duration},
};
pub fn factory() -> Factory {
    #[cfg(target_os = "macos")]
    {
        crate::apple_helper::factory()
    }
    #[cfg(not(target_os = "macos"))]
    Arc::new(|| {
        let mut worker = Worker::spawn(
            &std::env::current_exe().map_err(|e| e.to_string())?,
            &[ARGUMENT.into()],
            BACKEND,
            Duration::from_secs(3),
        )?;
        worker.response_timeout(Duration::from_millis(100));
        Ok(Box::new(worker) as Box<dyn Channel>)
    })
}
pub fn helper_main() -> Option<i32> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 1 || args[0] != ARGUMENT {
        return None;
    }
    Some(run())
}
fn failure(mut output: impl Write, error: &str) -> i32 {
    let message = serde_json::json!({"ok":false,"backend":BACKEND,"protocol":1,"error":error.chars().take(512).collect::<String>()});
    let _ = writeln!(output, "{message}");
    1
}
#[cfg(any(windows, target_os = "linux", test))]
fn serve<D: Driver, R: BufRead + Send + 'static, W: Write + Send + 'static>(
    driver: D,
    input: R,
    output: W,
) -> i32 {
    match crate::worker::serve(driver, input, output, BACKEND) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}
fn run() -> i32 {
    #[cfg(windows)]
    match crate::windows::Native::open() {
        Ok(driver) => serve(driver, BufReader::new(io::stdin()), io::stdout()),
        Err(error) => failure(io::stdout().lock(), &error),
    }
    #[cfg(target_os = "linux")]
    match crate::linux_uinput::Native::open() {
        Ok(backend) => serve(
            crate::uinput::Output::new(backend),
            BufReader::new(io::stdin()),
            io::stdout(),
        ),
        Err(error) => failure(io::stdout().lock(), &error),
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        failure(io::stdout().lock(), crate::apple_helper::REASON)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Unavailable;
    impl Driver for Unavailable {
        fn check(&mut self) -> Result<(), String> {
            Err("not installed".into())
        }
        fn inspect(&mut self, _: u8) -> Result<crate::vjoy::Slot, String> {
            unreachable!()
        }
        fn acquire(&mut self, _: u8) -> Result<(), String> {
            unreachable!()
        }
        fn submit(&mut self, _: u8, _: &crate::vjoy::Position) -> Result<(), String> {
            unreachable!()
        }
        fn release(&mut self, _: u8) {
            unreachable!()
        }
    }
    #[test]
    fn entry_keeps_ordinary_startup_inert_and_reports_worker_failures() {
        assert!(helper_main().is_none());
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        assert_eq!(run(), 1);
        // A test harness is not a feeder; reject it before any reports.
        assert!(factory()().is_err());
        let mut bytes = Vec::new();
        assert_eq!(failure(&mut bytes, &"x".repeat(700)), 1);
        let error: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(error["error"].as_str().unwrap().len(), 512);
        assert_eq!(error["ok"], false);
        assert_eq!(
            serve(
                Unavailable,
                BufReader::new(io::Cursor::new(vec![])),
                io::sink()
            ),
            0
        );
        assert_eq!(
            serve(
                Unavailable,
                BufReader::new(io::Cursor::new(b"{}\n")),
                io::sink()
            ),
            1
        );
    }
}
