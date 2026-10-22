use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

// Cancellation and the first write share a lock: accepting Cancel prevents writes.
pub(crate) fn begin_writing(state: &Arc<Mutex<Value>>) -> Result<(), String> {
    let mut state = state.lock().unwrap_or_else(|p| p.into_inner());
    if state["cancelRequested"] == true {
        return Err("cancelled".into());
    }
    state["waitingForBoot"] = json!(false);
    state["stage"] = json!("Installing firmware");
    Ok(())
}
pub(crate) fn wait_for_boot<T>(
    state: &Arc<Mutex<Value>>,
    waiting: bool,
    mut connect: impl FnMut() -> Result<T, String>,
) -> Result<T, String> {
    let started = std::time::Instant::now();
    loop {
        if state.lock().unwrap_or_else(|p| p.into_inner())["cancelRequested"] == true {
            return Err("cancelled".into());
        }
        match connect() {
            Ok(connection) => return Ok(connection),
            Err(error)
                if waiting
                    && error.starts_with("bootloader-required:")
                    && started.elapsed() < Duration::from_secs(180) => {}
            Err(error) => return Err(error),
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boot_wait_retries_then_starts_and_cancellation_prevents_writes() {
        let state = Arc::new(Mutex::new(json!({"waitingForBoot":true})));
        let mut attempts = 0;
        assert_eq!(
            wait_for_boot(&state, true, || {
                attempts += 1;
                if attempts == 1 {
                    Err("bootloader-required:sync".into())
                } else {
                    Ok(7)
                }
            }),
            Ok(7)
        );
        assert_eq!(attempts, 2);
        begin_writing(&state).unwrap();
        assert_eq!(state.lock().unwrap()["waitingForBoot"], false);
        state.lock().unwrap()["cancelRequested"] = json!(true);
        assert_eq!(begin_writing(&state), Err("cancelled".into()));
        assert_eq!(
            wait_for_boot(&state, true, || Ok(7)),
            Err("cancelled".into())
        );
        state.lock().unwrap()["cancelRequested"] = json!(false);
        assert_eq!(
            wait_for_boot::<()>(&state, true, || Err("secured board".into())),
            Err("secured board".into())
        );
        assert!(
            wait_for_boot::<()>(&state, false, || Err("bootloader-required:sync".into())).is_err()
        );
    }
}
