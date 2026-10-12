//! Loopback browser fixture uses the same Rust service as the desktop application.
use std::io::{self, BufRead, Write};
fn main() {
    if !cfg!(debug_assertions) || std::env::var_os("OSG_NATIVE_EMULATOR").is_none() {
        eprintln!("The emulator service requires a debug build and OSG_NATIVE_EMULATOR.");
        std::process::exit(2);
    }
    let service = osg_cockpit::stm32::Stm32Service::new();
    for line in io::stdin().lock().lines() {
        let response = match line {
            Ok(line) if line.len() <= 16_384 => match serde_json::from_str(&line) {
                Ok(request) => service.request(request),
                Err(_) => serde_json::json!({"ok":false,"error":"Invalid JSON request."}),
            },
            Ok(_) => serde_json::json!({"ok":false,"error":"Request exceeds 16384 bytes."}),
            Err(_) => break,
        };
        println!("{response}");
        let _ = io::stdout().flush();
    }
    service.shutdown();
}
