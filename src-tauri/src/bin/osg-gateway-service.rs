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

// Profile documents have a separately bounded envelope. GatewayManager still
// enforces the original 64 KiB bound for unrelated commands after parsing.
const REQUEST_ENVELOPE_LIMIT: usize = osg_cockpit::gateways::PROFILE_EDITOR_REQUEST_LIMIT + 1_024;

// Read incrementally; an oversized line is discarded without retaining it.
fn read_frame(input: &mut impl BufRead) -> io::Result<Option<Result<Vec<u8>, ()>>> {
    let mut bytes = Vec::new();
    let mut oversized = false;
    loop {
        let buffer = input.fill_buf()?;
        if buffer.is_empty() {
            return Ok(if oversized {
                Some(Err(()))
            } else if bytes.is_empty() {
                None
            } else {
                Some(Ok(bytes))
            });
        }
        let newline = buffer.iter().position(|b| *b == b'\n');
        let count = newline.map_or(buffer.len(), |i| i + 1);
        let payload = newline.unwrap_or(count);
        if !oversized {
            if payload > REQUEST_ENVELOPE_LIMIT - bytes.len() {
                oversized = true;
                bytes.clear();
            } else {
                bytes.extend_from_slice(&buffer[..payload]);
            }
        }
        input.consume(count);
        if newline.is_some() {
            return Ok(Some(if oversized { Err(()) } else { Ok(bytes) }));
        }
    }
}
fn emit(output: &Mutex<io::Stdout>, response: Value) {
    let mut output = output.lock().unwrap_or_else(|p| p.into_inner());
    let _ = writeln!(output, "{response}");
    let _ = output.flush();
}
fn main() {
    if let Some(code) = osg_mapping_host::logic_helper_main() {
        std::process::exit(code);
    }
    #[cfg(windows)]
    if let Some(code) = osg_virtual_controller::windows_provision_entry::helper_main() {
        std::process::exit(code);
    }
    if let Some(code) = osg_virtual_controller::entry::helper_main() {
        std::process::exit(code);
    }
    if let Some(code) = osg_virtual_controller::inspection::helper_main() {
        std::process::exit(code);
    }
    if !cfg!(debug_assertions) {
        eprintln!("The browser gateway service requires a debug build.");
        std::process::exit(2);
    }
    let service = Arc::new(GatewayManager::new());
    let output = Arc::new(Mutex::new(io::stdout()));
    let active = Arc::new(AtomicUsize::new(0));
    let mut requests: Vec<thread::JoinHandle<()>> = Vec::new();
    let mut input = io::stdin().lock();
    while let Ok(Some(line)) = read_frame(&mut input) {
        let request: Value = match line {
            Ok(line) => match serde_json::from_slice(&line) {
                Ok(request) => request,
                Err(_) => {
                    emit(&output, json!({"ok":false,"error":"Invalid JSON request."}));
                    continue;
                }
            },
            Err(()) => {
                emit(
                    &output,
                    json!({"ok":false,"error":"Request exceeds its size limit."}),
                );
                continue;
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frames_bound_memory_before_parsing_and_recover_after_oversize() {
        let mut data = vec![b' '; REQUEST_ENVELOPE_LIMIT];
        data.push(b'\n');
        data.extend(vec![b'x'; REQUEST_ENVELOPE_LIMIT * 3]);
        data.extend_from_slice(b"\n{}\n");
        let mut input = io::BufReader::with_capacity(97, io::Cursor::new(data));
        assert_eq!(
            read_frame(&mut input).unwrap().unwrap().unwrap().len(),
            REQUEST_ENVELOPE_LIMIT
        );
        assert!(read_frame(&mut input).unwrap().unwrap().is_err());
        assert_eq!(read_frame(&mut input).unwrap().unwrap().unwrap(), b"{}");
        assert!(read_frame(&mut input).unwrap().is_none());
        let mut input = io::Cursor::new(vec![0; REQUEST_ENVELOPE_LIMIT + 1]);
        assert!(read_frame(&mut input).unwrap().unwrap().is_err());
        let mut input = io::Cursor::new(b"{}");
        assert_eq!(read_frame(&mut input).unwrap().unwrap().unwrap(), b"{}");
    }
}
