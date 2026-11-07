use crate::{supervisor::Worker, vjoy::Status};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(any(windows, test))]
use std::path::Path;
use std::{
    io::{BufRead, Write},
    time::{Duration, Instant},
};

pub(super) const ARGUMENT: &str = "--osg-read-vjoy-status";
const BACKEND: &str = "vjoy-status-ready";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
const QUERY_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SlotStatus {
    pub id: u8,
    pub status: Status,
    pub owner_pid: Option<u32>,
}

/// Retained read-only status process. The caller owns the sampling cadence.
pub struct StatusReader {
    worker: Worker,
}

fn decode_reply(value: Value, nonce: &str) -> Result<Vec<SlotStatus>, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Reply {
        nonce: String,
        result: Value,
    }
    let reply: Reply = serde_json::from_value(value).map_err(|e| e.to_string())?;
    if reply.nonce != nonce {
        return Err("Stale vJoy status acknowledgement.".into());
    }
    if reply.result["ok"] != true {
        return Err(reply.result["error"]
            .as_str()
            .unwrap_or("vJoy status query failed.")
            .chars()
            .take(512)
            .collect());
    }
    let slots: Vec<SlotStatus> =
        serde_json::from_value(reply.result["slots"].clone()).map_err(|e| e.to_string())?;
    if slots.iter().map(|slot| slot.id).collect::<Vec<_>>() != (1..=16).collect::<Vec<_>>() {
        return Err("vJoy status query did not return exactly slots 1..16.".into());
    }
    Ok(slots)
}

impl StatusReader {
    pub fn open() -> Result<Self, String> {
        #[cfg(windows)]
        {
            let program = std::env::current_exe().map_err(|e| e.to_string())?;
            Self::open_with(&program, &[ARGUMENT.into()])
        }
        #[cfg(not(windows))]
        {
            Err("vJoy status inspection requires Windows and the qualified signed 2.2.2.0 x64 driver.".into())
        }
    }

    #[cfg(any(windows, test))]
    fn open_with(program: &Path, args: &[String]) -> Result<Self, String> {
        let worker = Worker::spawn(program, args, BACKEND, STARTUP_TIMEOUT)?;
        Ok(Self { worker })
    }

    pub fn read(&mut self) -> Result<Vec<SlotStatus>, String> {
        let nonce = uuid::Uuid::new_v4().to_string();
        let deadline = Instant::now() + QUERY_TIMEOUT;
        match self
            .worker
            .status_once(&nonce, deadline)
            .and_then(|reply| decode_reply(reply, &nonce))
        {
            Ok(slots) => Ok(slots),
            Err(error) => {
                self.worker.terminate();
                Err(error)
            }
        }
    }
}

fn status_response(result: Result<Vec<SlotStatus>, String>) -> Value {
    match result {
        Ok(slots) => json!({"ok":true,"slots":slots}),
        Err(error) => json!({"ok":false,"error":error.chars().take(512).collect::<String>()}),
    }
}

fn serve_status(
    input: &mut impl BufRead,
    output: &mut impl Write,
    mut query: impl FnMut(u8) -> Result<SlotStatus, String>,
) -> Result<(), String> {
    writeln!(
        output,
        "{}",
        json!({"ok":true,"protocol":1,"backend":BACKEND})
    )
    .and_then(|_| output.flush())
    .map_err(|e| e.to_string())?;
    loop {
        let Some(bytes) = crate::worker::read_frame(input).map_err(|e| e.to_string())? else {
            return Ok(());
        };
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Request {
            nonce: String,
        }
        let request: Request = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let parsed = uuid::Uuid::parse_str(&request.nonce).map_err(|e| e.to_string())?;
        if parsed.is_nil() || parsed.to_string() != request.nonce {
            return Err("Invalid vJoy status nonce.".into());
        }
        let result = status_response((|| {
            (1..=16)
                .map(|id| {
                    let slot = query(id)?;
                    if slot.id != id {
                        return Err("vJoy status query returned another slot identity.".into());
                    }
                    Ok(slot)
                })
                .collect::<Result<Vec<_>, String>>()
        })());
        writeln!(output, "{}", json!({"nonce":request.nonce,"result":result}))
            .and_then(|_| output.flush())
            .map_err(|e| e.to_string())?;
    }
}

pub(super) fn helper_main() -> Option<i32> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 1 || args[0] != ARGUMENT {
        return None;
    }
    #[cfg(windows)]
    let result = crate::windows::Native::open().and_then(|mut native| {
        serve_status(
            &mut std::io::stdin().lock(),
            &mut std::io::stdout().lock(),
            |id| native.status_only(id),
        )
    });
    #[cfg(not(windows))]
    let result: Result<(), String> = Err(
        "vJoy status inspection requires Windows and the qualified signed 2.2.2.0 x64 driver."
            .into(),
    );
    Some(i32::from(result.is_err()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_protocol_serves_repeated_exact_nonce_queries_without_output_commands() {
        let first = uuid::Uuid::new_v4().to_string();
        let second = uuid::Uuid::new_v4().to_string();
        let requests = format!("{{\"nonce\":\"{first}\"}}\n{{\"nonce\":\"{second}\"}}\n");
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = calls.clone();
        let mut output = Vec::new();
        serve_status(
            &mut std::io::Cursor::new(requests),
            &mut output,
            move |id| {
                observed.lock().unwrap().push(id);
                Ok(SlotStatus {
                    id,
                    status: Status::Free,
                    owner_pid: None,
                })
            },
        )
        .unwrap();
        let frames: Vec<Value> = output
            .split(|b| *b == b'\n')
            .filter(|b| !b.is_empty())
            .map(|b| serde_json::from_slice(b).unwrap())
            .collect();
        assert_eq!(frames[0], json!({"ok":true,"protocol":1,"backend":BACKEND}));
        assert_eq!(frames[1]["nonce"], first);
        assert_eq!(frames[2]["nonce"], second);
        assert_eq!(frames[1]["result"]["slots"].as_array().unwrap().len(), 16);
        assert_eq!(
            *calls.lock().unwrap(),
            (1..=16).chain(1..=16).collect::<Vec<_>>()
        );
    }

    #[test]
    fn replies_require_the_exact_nonce_and_complete_ordered_slot_set() {
        let nonce = uuid::Uuid::new_v4().to_string();
        let slots: Vec<_> = (1..=16)
            .map(|id| json!({"id":id,"status":"free","ownerPid":null}))
            .collect();
        let reply =
            |nonce: &str, slots: Value| json!({"nonce":nonce,"result":{"ok":true,"slots":slots}});
        assert_eq!(
            decode_reply(reply(&nonce, json!(slots.clone())), &nonce)
                .unwrap()
                .len(),
            16
        );
        assert!(decode_reply(reply("stale", json!(slots.clone())), &nonce).is_err());
        assert!(decode_reply(reply(&nonce, json!(&slots[..15])), &nonce).is_err());
        let mut unordered = slots;
        unordered.swap(0, 1);
        assert!(decode_reply(reply(&nonce, json!(unordered)), &nonce).is_err());
        assert_eq!(
            decode_reply(
                json!({"nonce":nonce,"result":status_response(Err("query failed".into()))}),
                &nonce
            )
            .unwrap_err(),
            "query failed"
        );
    }

    #[test]
    fn helper_rejects_invalid_nonce_and_fails_an_inexact_native_result() {
        let mut output = Vec::new();
        let malformed = format!("{{\"nonce\":\"{}\"}}\n", uuid::Uuid::nil());
        assert!(serve_status(
            &mut std::io::Cursor::new(malformed),
            &mut output,
            |_| panic!("invalid nonce queried vJoy")
        )
        .is_err());
        output.clear();
        let nonce = uuid::Uuid::new_v4().to_string();
        let request = format!("{{\"nonce\":\"{nonce}\"}}\n");
        serve_status(&mut std::io::Cursor::new(request), &mut output, |_| {
            Ok(SlotStatus {
                id: 2,
                status: Status::Missing,
                owner_pid: None,
            })
        })
        .unwrap();
        let reply =
            serde_json::from_slice(output.split(|byte| *byte == b'\n').nth(1).unwrap()).unwrap();
        assert!(decode_reply(reply, &nonce)
            .unwrap_err()
            .contains("another slot identity"));
    }

    #[test]
    fn one_status_helper_serves_multiple_queries_and_retires_on_a_stale_reply() {
        let python = std::env::var("OSG_TEST_PYTHON").unwrap_or_else(|_| {
            if cfg!(windows) {
                "python".into()
            } else {
                "python3".into()
            }
        });
        let program = r#"
import json, sys
print(json.dumps({'ok': True, 'protocol': 1, 'backend': 'vjoy-status-ready'}), flush=True)
for index, line in enumerate(sys.stdin):
    request = json.loads(line)
    slots = [{'id': slot, 'status': 'busy', 'ownerPid': 100 + index} for slot in range(1, 17)]
    nonce = request['nonce'] if index < 2 else 'obsolete'
    print(json.dumps({'nonce': nonce, 'result': {'ok': True, 'slots': slots}}), flush=True)
"#;
        let mut reader =
            StatusReader::open_with(Path::new(&python), &["-c".into(), program.into()]).unwrap();
        let pid = reader.worker.process_id();
        assert_eq!(reader.read().unwrap()[0].owner_pid, Some(100));
        assert_eq!(reader.read().unwrap()[0].owner_pid, Some(101));
        assert_eq!(reader.worker.process_id(), pid);
        assert!(reader.read().unwrap_err().contains("Stale vJoy status"));
        assert!(!reader.worker.alive());
    }
}
