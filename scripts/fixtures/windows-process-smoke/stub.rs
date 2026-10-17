#![cfg_attr(windows, windows_subsystem = "windows")]
use std::{
    fs,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
fn main() {
    let args = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if args.len() != 2 {
        return;
    }
    let Some(root) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent()?.parent()?.parent().map(PathBuf::from))
    else {
        return;
    };
    let Ok(root) = fs::canonicalize(root) else {
        return;
    };
    if args.iter().any(|p| {
        !p.is_absolute()
            || p.parent().and_then(|p| fs::canonicalize(p).ok()).as_ref() != Some(&root)
    }) {
        return;
    }
    if fs::write(&args[1], std::process::id().to_string()).is_err() {
        return;
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    while !args[0].exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
}
