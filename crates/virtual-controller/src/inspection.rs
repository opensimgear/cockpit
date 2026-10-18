//! Read-only inspection is isolated from the UI and from feeder DLL lifetimes.
use crate::{supervisor::Worker, vjoy::Slot};
use serde_json::{json, Value};
use std::{
    path::Path,
    time::{Duration, Instant},
};
pub const ARGUMENT: &str = "--osg-inspect-vjoy";
const SAMPLE_ARGUMENT: &str = "--osg-sample-vjoy";
/// The timestamp precedes the complete inventory query, after driver readiness.
/// It is volatile and must still be checked at the consuming ownership fence.
pub struct Sample {
    slots: Vec<Slot>,
    observed: Instant,
}
impl Sample {
    pub fn into_parts(self) -> (Vec<Slot>, Instant) {
        (self.slots, self.observed)
    }
}
fn slots(response: &Value) -> Result<Vec<Slot>, String> {
    if response["ok"] != true {
        return Err(response["error"]
            .as_str()
            .unwrap_or("vJoy inspection failed.")
            .chars()
            .take(512)
            .collect());
    }
    let slots: Vec<Slot> =
        serde_json::from_value(response["slots"].clone()).map_err(|e| e.to_string())?;
    if slots.len() > 16
        || slots.iter().any(|s| {
            !(1..=16).contains(&s.id)
                || s.axes.len() > 16
                || s.buttons > 128
                || s.continuous_hats > 4
                || s.discrete_hats > 4
        })
    {
        return Err("Invalid vJoy inspection capability bounds.".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    if slots.iter().any(|s| !ids.insert(s.id)) {
        return Err("Duplicate vJoy inspection slots.".into());
    }
    Ok(slots)
}
pub fn inspect_with(program: &Path, args: &[String]) -> Result<Vec<Slot>, String> {
    let worker = Worker::spawn(program, args, "vjoy-inspect", Duration::from_secs(3))?;
    slots(&worker.handshake()["result"])
}
pub fn current() -> Result<Vec<Slot>, String> {
    inspect_with(
        &std::env::current_exe().map_err(|e| e.to_string())?,
        &[ARGUMENT.into()],
    )
}
fn validate_slot_ids(ids: &[u8]) -> Result<(), String> {
    let unique: std::collections::BTreeSet<_> = ids.iter().copied().collect();
    if ids.is_empty()
        || ids.len() > 16
        || unique.len() != ids.len()
        || ids.iter().any(|id| !(1..=16).contains(id))
    {
        return Err("Read-only inspection needs unique selected slot IDs within 1..16.".into());
    }
    Ok(())
}
pub fn sample_with(program: &Path, args: &[String], ids: &[u8]) -> Result<Sample, String> {
    validate_slot_ids(ids)?;
    let mut worker = Worker::spawn(program, args, "vjoy-inspect-ready", Duration::from_secs(3))?;
    let nonce = uuid::Uuid::new_v4().to_string();
    let observed = Instant::now();
    let deadline = observed + Duration::from_micros(crate::feeder::LEASE_US);
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Reply {
        nonce: String,
        result: Value,
    }
    let reply: Reply = serde_json::from_value(worker.inspect_once(&nonce, ids, deadline)?)
        .map_err(|e| e.to_string())?;
    if reply.nonce != nonce {
        return Err("Stale read-only inspection acknowledgement.".into());
    }
    let slots = slots(&reply.result)?;
    let requested: std::collections::BTreeSet<_> = ids.iter().copied().collect();
    if slots
        .iter()
        .map(|slot| slot.id)
        .collect::<std::collections::BTreeSet<_>>()
        != requested
    {
        return Err("The read-only inspection did not return exactly the selected slots.".into());
    }
    // Include parsing and child teardown. A queued reply never renews freshness.
    drop(worker);
    if Instant::now() >= deadline {
        return Err("The fresh virtual-slot inspection exceeded its response deadline.".into());
    }
    Ok(Sample { slots, observed })
}
pub fn sample_current(ids: &[u8]) -> Result<Sample, String> {
    sample_with(
        &std::env::current_exe().map_err(|e| e.to_string())?,
        &[SAMPLE_ARGUMENT.into()],
        ids,
    )
}
fn serve_sample(
    mut driver: impl crate::vjoy::Driver,
    input: &mut impl std::io::BufRead,
    output: &mut impl std::io::Write,
) -> Result<(), String> {
    writeln!(
        output,
        "{}",
        json!({"ok":true,"protocol":1,"backend":"vjoy-inspect-ready"})
    )
    .and_then(|_| output.flush())
    .map_err(|e| e.to_string())?;
    let bytes = crate::worker::read_frame(input)
        .map_err(|e| e.to_string())?
        .ok_or("Read-only inspection request is missing.")?;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Request {
        nonce: String,
        slot_ids: Vec<u8>,
    }
    let request: Request = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let parsed = uuid::Uuid::parse_str(&request.nonce).map_err(|e| e.to_string())?;
    if parsed.is_nil() || parsed.to_string() != request.nonce {
        return Err("Invalid read-only inspection nonce.".into());
    }
    validate_slot_ids(&request.slot_ids)?;
    // There is exactly one query, after readiness and after receiving the nonce.
    // Do not scan unrelated slots. Even Missing selected slots must be returned
    // so the consumer can reject their exact native state rather than omit it.
    let result = response((|| {
        driver.check()?;
        request
            .slot_ids
            .iter()
            .map(|id| {
                let slot = driver.inspect(*id)?;
                if slot.id != *id {
                    return Err("Read-only inspection returned another slot identity.".into());
                }
                Ok(slot)
            })
            .collect::<Result<Vec<_>, String>>()
    })());
    writeln!(
        output,
        "{}",
        json!({"nonce":request.nonce,"result":result["result"]})
    )
    .and_then(|_| output.flush())
    .map_err(|e| e.to_string())
}
pub fn response(result: Result<Vec<Slot>, String>) -> Value {
    let result = match result {
        Ok(slots) => json!({"ok":true,"slots":slots}),
        Err(error) => json!({"ok":false,"error":error.chars().take(512).collect::<String>()}),
    };
    json!({"ok":true,"protocol":1,"backend":"vjoy-inspect","result":result})
}
/// Called before native UI/platform services start. Inspect never acquires a slot.
pub fn helper_main() -> Option<i32> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 1 || (args[0] != ARGUMENT && args[0] != SAMPLE_ARGUMENT) {
        return None;
    }
    if args[0] == SAMPLE_ARGUMENT {
        #[cfg(windows)]
        let result = crate::windows::Native::open().and_then(|driver| {
            serve_sample(
                driver,
                &mut std::io::stdin().lock(),
                &mut std::io::stdout().lock(),
            )
        });
        #[cfg(not(windows))]
        let result: Result<(), String> =
            Err("vJoy requires Windows and the qualified signed 2.2.2.0 x64 driver.".into());
        return Some(if result.is_ok() { 0 } else { 1 });
    }
    #[cfg(windows)]
    let result =
        crate::windows::Native::open().and_then(|mut driver| crate::vjoy::inventory(&mut driver));
    #[cfg(not(windows))]
    let result = Err("vJoy requires Windows and the qualified signed 2.2.2.0 x64 driver.".into());
    use std::io::Write;
    let ok = writeln!(std::io::stdout().lock(), "{}", response(result)).is_ok();
    Some(if ok { 0 } else { 1 })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn python() -> String {
        std::env::var("OSG_TEST_PYTHON").unwrap_or_else(|_| {
            if cfg!(windows) {
                "python".into()
            } else {
                "python3".into()
            }
        })
    }
    fn probe(value: Value) -> Result<Vec<Slot>, String> {
        let encoded = value.to_string();
        let program = format!(
            "print({},flush=True)",
            serde_json::to_string(&encoded).unwrap()
        );
        inspect_with(Path::new(&python()), &["-c".into(), program])
    }
    fn sampled(startup_ms: u32, query_ms: u32, result: &str) -> Result<Sample, String> {
        let slot=serde_json::to_string(&serde_json::to_string(&json!({"id":3,"status":"missing","ownerPid":null,"buttons":0,"axes":[],"continuousHats":0,"discreteHats":0,"forceFeedback":false})).unwrap()).unwrap();
        let code=format!("import json,sys,time\nslot=json.loads({slot})\ntime.sleep({startup_ms}/1000)\nprint(json.dumps({{'ok':True,'protocol':1,'backend':'vjoy-inspect-ready','result':{{'ok':True,'slots':[{{'id':99}}]}}}}),flush=True)\nq=json.loads(sys.stdin.readline())\nassert q['slotIds']==[3]\ntime.sleep({query_ms}/1000)\n{result}\n");
        sample_with(
            Path::new(&python()),
            &["-u".into(), "-c".into(), code],
            &[3],
        )
    }
    #[test]
    fn slow_readiness_is_separate_from_the_unchanged_fresh_sample_deadline() {
        let started = Instant::now();
        let sample = sampled(
            350,
            0,
            "print(json.dumps({'nonce':q['nonce'],'result':{'ok':True,'slots':[slot]}}),flush=True)",
        )
        .unwrap();
        let (slots, observed) = sample.into_parts();
        assert_eq!(slots.len(), 1); // Handshake's invalid cached slots were never used.
        assert_eq!(slots[0].id, 3);
        assert_eq!(slots[0].status, crate::vjoy::Status::Missing);
        assert!(started.elapsed() >= Duration::from_millis(350));
        assert!(observed.duration_since(started) >= Duration::from_millis(350));
        assert!(observed.elapsed() < Duration::from_micros(crate::feeder::LEASE_US));
        assert!(sampled(
            0,
            350,
            "print(json.dumps({'nonce':q['nonce'],'result':{'ok':True,'slots':[slot]}}),flush=True)"
        )
        .err()
        .unwrap()
        .contains("deadline"));
    }
    #[test]
    fn sampled_replies_require_the_exact_query_and_bounded_inventory() {
        for reply in [
            "print(json.dumps({'nonce':'another-query','result':{'ok':True,'slots':[]}}),flush=True)",
            "print(json.dumps({'nonce':q['nonce'],'result':{'ok':True,'slots':[]},'extra':True}),flush=True)",
            "print(json.dumps({'nonce':q['nonce'],'result':{'ok':True,'slots':None}}),flush=True)",
            "print(json.dumps({'nonce':q['nonce'],'result':{'ok':False,'error':'native unavailable'}}),flush=True)",
            "print(json.dumps({'nonce':q['nonce'],'result':{'ok':True,'slots':[]}}),flush=True)",
            "print(json.dumps({'nonce':q['nonce'],'result':{'ok':True,'slots':[dict(slot,id=4)]}}),flush=True)",
            "print(json.dumps({'nonce':q['nonce'],'result':{'ok':True,'slots':[slot,dict(slot,id=4)]}}),flush=True)",
            "print(json.dumps({'nonce':q['nonce'],'result':{'ok':True,'slots':[slot,slot]}}),flush=True)",
        ] {
            assert!(sampled(0,0,reply).is_err());
        }
        assert!(sample_with(
            Path::new(&python()),
            &["-c".into(), "print('{}',flush=True)".into()],
            &[3],
        )
        .is_err());
    }
    #[test]
    fn invalid_selected_slots_fail_before_starting_any_helper() {
        for ids in [vec![], vec![0], vec![17], vec![3, 3], (1..=17).collect()] {
            assert!(sample_with(Path::new("must-not-be-launched"), &[], &ids)
                .err()
                .unwrap()
                .contains("unique selected slot IDs"));
        }
        validate_slot_ids(&(1..=16).collect::<Vec<_>>()).unwrap();
    }
    #[test]
    fn the_ready_helper_reads_one_exact_nonce_before_any_readonly_inventory() {
        struct ReadOnlyDriver(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl crate::vjoy::Driver for ReadOnlyDriver {
            fn check(&mut self) -> Result<(), String> {
                self.0.lock().unwrap().push(0);
                Ok(())
            }
            fn inspect(&mut self, id: u8) -> Result<Slot, String> {
                self.0.lock().unwrap().push(id);
                Ok(Slot {
                    id,
                    status: crate::vjoy::Status::Missing,
                    owner_pid: None,
                    buttons: 0,
                    axes: vec![],
                    continuous_hats: 0,
                    discrete_hats: 0,
                    force_feedback: false,
                })
            }
            fn acquire(&mut self, _: u8) -> Result<(), String> {
                panic!("inspection cannot acquire")
            }
            fn submit(&mut self, _: u8, _: &crate::vjoy::Position) -> Result<(), String> {
                panic!("inspection cannot submit")
            }
            fn release(&mut self, _: u8) {
                panic!("inspection cannot release")
            }
        }
        let nonce = uuid::Uuid::new_v4().to_string();
        for (request, valid) in [
            (
                format!(
                    "{{\"nonce\":\"{nonce}\",\"slotIds\":[3,8]}}\n{{\"nonce\":\"second query\"}}\n"
                ),
                true,
            ),
            ("".into(), false),
            (
                "{\"nonce\":\"00000000-0000-0000-0000-000000000000\",\"slotIds\":[3]}\n".into(),
                false,
            ),
            ("{\"nonce\":\"invalid\",\"slotIds\":[3]}\n".into(), false),
            (
                format!("{{\"nonce\":\"{nonce}\",\"slotIds\":[3],\"command\":\"acquire\"}}\n"),
                false,
            ),
            (format!("{{\"nonce\":\"{nonce}\",\"slotIds\":[]}}\n"), false),
            (
                format!("{{\"nonce\":\"{nonce}\",\"slotIds\":[3,3]}}\n"),
                false,
            ),
            (
                format!("{{\"nonce\":\"{nonce}\",\"slotIds\":[0]}}\n"),
                false,
            ),
            (
                format!("{{\"nonce\":\"{nonce}\",\"slotIds\":[17]}}\n"),
                false,
            ),
        ] {
            let calls = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
            let mut output = vec![];
            let result = serve_sample(
                ReadOnlyDriver(calls.clone()),
                &mut std::io::Cursor::new(request),
                &mut output,
            );
            assert_eq!(result.is_ok(), valid);
            let values = output
                .split(|b| *b == b'\n')
                .filter(|b| !b.is_empty())
                .map(|b| serde_json::from_slice::<Value>(b).unwrap())
                .collect::<Vec<_>>();
            assert_eq!(
                values[0],
                json!({"ok":true,"protocol":1,"backend":"vjoy-inspect-ready"})
            );
            if valid {
                assert_eq!(values.len(), 2);
                assert_eq!(
                    values[1],
                    json!({"nonce":nonce,"result":{"ok":true,"slots":[
                        {"id":3,"status":"missing","ownerPid":null,"buttons":0,"axes":[],"continuousHats":0,"discreteHats":0,"forceFeedback":false},
                        {"id":8,"status":"missing","ownerPid":null,"buttons":0,"axes":[],"continuousHats":0,"discreteHats":0,"forceFeedback":false}
                    ]}})
                );
                assert_eq!(*calls.lock().unwrap(), vec![0, 3, 8]);
            } else {
                assert_eq!(values.len(), 1);
                assert!(calls.lock().unwrap().is_empty());
            }
        }
    }
    #[test]
    fn inspection_is_bounded_and_preserves_driver_errors() {
        assert!(probe(response(Ok(vec![]))).unwrap().is_empty());
        assert!(probe(response(Err("driver unavailable".into())))
            .unwrap_err()
            .contains("unavailable"));
        assert!(probe(json!({"ok":true,"protocol":1,"backend":"vjoy-inspect","result":{"ok":true,"slots":null}})).is_err());
        let slot = Slot {
            id: 1,
            status: crate::vjoy::Status::Free,
            owner_pid: None,
            buttons: 8,
            axes: vec![],
            discrete_hats: 0,
            continuous_hats: 0,
            force_feedback: false,
        };
        assert_eq!(probe(response(Ok(vec![slot.clone()]))).unwrap().len(), 1);
        assert!(probe(response(Ok(vec![slot.clone(); 17]))).is_err());
        assert!(probe(response(Ok(vec![slot.clone(); 2]))).is_err());
        let mut invalid = slot;
        invalid.id = 0;
        assert!(probe(response(Ok(vec![invalid]))).is_err());
        assert!(helper_main().is_none());
    }
}
