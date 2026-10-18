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
