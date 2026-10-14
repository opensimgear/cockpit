//! Read-only inspection is isolated from the UI and from feeder DLL lifetimes.
use crate::{supervisor::Worker, vjoy::Slot};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};
pub const ARGUMENT: &str = "--osg-inspect-vjoy";
pub fn inspect_with(program: &Path, args: &[String]) -> Result<Vec<Slot>, String> {
    let worker = Worker::spawn(program, args, "vjoy-inspect", Duration::from_secs(3))?;
    let response = &worker.handshake()["result"];
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
pub fn current() -> Result<Vec<Slot>, String> {
    inspect_with(
        &std::env::current_exe().map_err(|e| e.to_string())?,
        &[ARGUMENT.into()],
    )
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
    if args.len() != 1 || args[0] != ARGUMENT {
        return None;
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
