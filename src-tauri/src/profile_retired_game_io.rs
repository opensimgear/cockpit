//! Native cleanup composition for retained game ownership; never a start path.
use super::*;
use std::time::{Duration, Instant};
type Panel = Arc<dyn Fn(Value) -> Value + Send + Sync>;

fn owner(request: &Value) -> Result<&str, String> {
    request["owner"].as_str().filter(|owner| {
        uuid::Uuid::parse_str(owner).is_ok_and(|parsed| parsed.to_string() == *owner)
    }).ok_or_else(|| "Retained cleanup requires its exact native owner.".into())
}

fn restore_visibility(owner: &str, budget: Duration, mut operation: impl FnMut(&str, &Value) -> Result<Value, String>) -> Result<Value, String> {
    let request = json!({"owner":owner});
    // Submit restoration once. A lost reply remains an explicit recovery error.
    let mut status = operation("stop", &request)?;
    let deadline = Instant::now() + budget;
    while status["active"] == true {
        if Instant::now() >= deadline {
            return Err("Retained visibility restoration is still pending; inspect its exact owner before recovering again.".into());
        }
        std::thread::sleep(Duration::from_millis(25));
        status = operation("poll", &request)?;
    }
    // A completed worker is not sufficient: validate its actual clean journal
    // before forgetting owner tracking. This cannot start hiding or retry an
    // uncertain first Stop.
    let final_status = operation("stop", &request)?;
    if final_status["active"] != false || final_status["phase"] != "restored" {
        return Err("Retained visibility ownership did not finish exact restoration.".into());
    }
    Ok(final_status)
}

pub(super) fn panel(
    physical: Panel,
    recovery: Arc<crate::profile_retired_recovery::Recovery>,
    controllers: Arc<osg_controller_input::ControllerService>,
    companions: Arc<crate::companion_host::Runtime>,
) -> Panel {
    Arc::new(move |request| {
        let command = request["command"].as_str().unwrap_or("");
        let result: Result<Value, String> = match command {
            "_retired_mapping_inspect" => owner(&request).and_then(|owner| recovery.capture_mapping_owner(owner)).map(|reference| json!({"recovery":reference})),
            "_retired_mapping_cleanup" => (|| {
                let owner = owner(&request)?;
                let reference = request.get("recovery").ok_or("Retained cleanup requires its explicit captured reference, including a clean null.")?;
                let reference = serde_json::from_value::<Option<crate::profile_retired_recovery::JournalRef>>(reference.clone()).map_err(|error| error.to_string())?;
                recovery.recover_mapping_owner(owner, &reference)?;
                Ok(json!({}))
            })(),
            "_retired_mapping_abandon" => (|| {
                if recovery.capture_mapping_owner(owner(&request)?)?.is_some() {
                    return Err("Retained output is still pending; preserve its exact journal until explicit cleanup or acknowledgement.".into());
                }
                Ok(json!({}))
            })(),
            "_retired_mapping_visibility_cleanup" => owner(&request).and_then(|owner| restore_visibility(owner, Duration::from_secs(3), |operation, request| controllers.game_visibility(operation, request))),
            "_game_companion_restore" if request["recovery"] != true => Err("Retained companion cleanup cannot resume an old runtime.".into()),
            "_game_companion_restore" | "_game_companion_abandon" => companions.game_command(command, &request),
            _ if command.starts_with("_game_") || command.starts_with("_retired_") || command.starts_with("integration_") || command.starts_with("dcs_") => Err("Independent game configuration and output starts are retired. Apply a unified profile with its matching native context.".into()),
            _ => return physical(request),
        };
        match result {
            Ok(state) => json!({"ok":true,"state":state}),
            Err(error) => json!({"ok":false,"error":error}),
        }
    })
}

#[cfg(test)]
#[path = "profile_retired_game_io_tests.rs"]
mod tests;
