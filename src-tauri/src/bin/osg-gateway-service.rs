//! Development browser transport using the same gateway manager as Cockpit desktop.
use osg_cockpit::gateways::GatewayManager;
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, Write},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
};

fn emit(output: &Mutex<io::Stdout>, response: Value) {
    let mut output = output.lock().unwrap_or_else(|p| p.into_inner());
    let _ = writeln!(output, "{response}");
    let _ = output.flush();
}
fn main() {
    if !cfg!(debug_assertions) {
        eprintln!("The browser gateway service requires a debug build.");
        std::process::exit(2);
    }
    let service = Arc::new(GatewayManager::new());
    let output = Arc::new(Mutex::new(io::stdout()));
    let active = Arc::new(AtomicUsize::new(0));
    let mut requests: Vec<thread::JoinHandle<()>> = Vec::new();
    for line in io::stdin().lock().lines() {
        let request: Value = match line {
            Ok(line) if line.len() <= 17_408 => match serde_json::from_str(&line) {
                Ok(request) => request,
                Err(_) => {
                    emit(&output, json!({"ok":false,"error":"Invalid JSON request."}));
                    continue;
                }
            },
            Ok(_) => {
                emit(
                    &output,
                    json!({"ok":false,"error":"Request exceeds its size limit."}),
                );
                continue;
            }
            Err(_) => break,
        };
        if let Some(id) = request.get("requestId").cloned() {
            if !id.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 80)
                || !request["request"].is_object()
            {
                emit(
                    &output,
                    json!({"requestId":id,"response":{"ok":false,"error":"Invalid gateway request envelope."}}),
                );
                continue;
            }
            if active.fetch_add(1, Ordering::AcqRel) >= 32 {
                active.fetch_sub(1, Ordering::AcqRel);
                emit(
                    &output,
                    json!({"requestId":id,"response":{"ok":false,"error":"Gateway service is busy; wait for an active request."}}),
                );
                continue;
            }
            let (service, output, active) = (service.clone(), output.clone(), active.clone());
            let request = request["request"].clone();
            requests.retain(|r| !r.is_finished());
            requests.push(thread::spawn(move || {
                let response = service.request(request);
                emit(&output, json!({"requestId":id,"response":response}));
                active.fetch_sub(1, Ordering::AcqRel);
            }));
        } else {
            // Keep the original sequential protocol for CLI consumers.
            emit(&output, service.request(request));
        }
    }
    service.shutdown();
    for request in requests {
        let _ = request.join();
    }
}
