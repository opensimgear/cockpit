use crate::game_discovery::{ProcessIdentity, ProcessLookup, ProcessLookupError};
use crate::xplane::process_events::{ExitEvidence, ExitStatus, ExitWatch};

enum ProcessEvidence {
    Verified(Instant),
    Unavailable(String),
}

fn observe_preparation_inner(inner: &Inner, proof: &GamePreparation,
    verify: impl FnOnce(&Value, &ProcessIdentity) -> Result<bool, ProcessLookupError>,
) -> Result<ProcessEvidence, String> {
    let game = validate_preparation_state(inner, proof)?;
    inner.setup.validate_native_build(&game, false)?;
    // Expensive catalogue/build work precedes this exact process observation.
    // Its own duration still counts against the unchanged process lease.
    let observed = Instant::now();
    let evidence = match verify(&game, &proof.process) {
        Ok(true) => ProcessEvidence::Verified(observed),
        Ok(false) => return Err("The prepared game process exited, was replaced or became ambiguous.".into()),
        Err(ProcessLookupError::Ambiguous(message)) => return Err(message),
        Err(ProcessLookupError::Unavailable(message)) => ProcessEvidence::Unavailable(message),
    };
    // A completed observation cannot publish over settings, catalogue, aircraft
    // or preparation changes that occurred while native process I/O was pending.
    if validate_preparation_state(inner, proof)? != game {
        return Err("The selected game installation changed.".into());
    }
    Ok(evidence)
}

impl Inner {
    fn current_process_epoch(&self) -> Option<(u64, u64)> {
        let library = self.changes.context_wake.observed();
        let processes = self.changes.context_process_wake.observed();
        (!self.closed.load(Ordering::Acquire) && self.changes.context_process_live(library))
            .then_some((library, processes))
    }

    fn process_current(&self, process: ProcessLookup, expected: &ProcessIdentity) -> Result<bool, ProcessLookupError> {
        match process {
            Ok(process) if process.as_ref() == Some(expected) => {
                self.platform.alive(expected).map_err(ProcessLookupError::Unavailable)
            }
            Ok(_) => Ok(false),
            Err(ProcessLookupError::Unavailable(_)) if self.platform.alive(expected) == Ok(false) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn exact_process_current(&self, game: &Value, expected: &ProcessIdentity) -> Result<bool, ProcessLookupError> {
        let process = self.platform.integration_processes(&[game]).into_iter().next()
            .ok_or_else(|| ProcessLookupError::Unavailable("Game process inventory was incomplete.".into()))?;
        self.process_current(process, expected)
    }

    fn observe_profile_process(&self, game: &Value, expected: &ProcessIdentity,
        exit: Option<&ExitEvidence>,
    ) -> Result<bool, ProcessLookupError> {
        if let Some(epoch) = self.current_process_epoch() {
            let snapshot = self.context_process_inventory.lock().unwrap_or_else(|p| p.into_inner()).clone();
            if let Some(process) = snapshot.as_ref().and_then(|snapshot| snapshot.process(game, epoch))
                .filter(|process| !matches!(process, Err(ProcessLookupError::Unavailable(_))))
            {
                // The inventory proves uniqueness. A wait on the retained
                // exact process proves liveness without reopening the PID.
                let live = match exit.map(|exit| exit.status(expected)) {
                    Some(ExitStatus::Watching) if matches!(&process, Ok(Some(identity)) if identity == expected) => Ok(true),
                    Some(ExitStatus::Exited) => Ok(false),
                    _ => self.process_current(process, expected),
                };
                if self.current_process_epoch() == Some(epoch) { return live; }
            }
        }
        self.exact_process_current(game, expected)
    }
}

fn validate_preparation_state(inner: &Inner, proof: &GamePreparation) -> Result<Value, String> {
    inner.guard_installation_metadata()?;
    let game = inner.settings.lock().unwrap_or_else(|p| p.into_inner()).catalogue.iter()
        .find(|game| game["id"] == proof.game_id).map(preparation_installation)
        .ok_or("The selected game installation changed.")?;
    if inner.contexts.lock().unwrap_or_else(|p| p.into_inner()).validate_preparation(proof)? != &game {
        return Err("The selected game installation changed.".into());
    }
    if inner.runtime_vehicle_id(&proof.game_id) != proof.vehicle_id {
        return Err("The loaded DCS aircraft changed; acquire its current module before using this branch.".into());
    }
    let integration = crate::game_discovery::integration_key(&game)
        .ok_or("The game integration binding changed.")?;
    if inner.setup.provider_setup_revision(integration) != proof.provider_setup_revision
        || inner.setup.current_catalogue_revision_for_vehicle(integration, proof.vehicle_id.as_deref())
            .as_ref() != Some(&proof.catalogue_revision)
    {
        return Err("The prepared provider setup or catalogue changed.".into());
    }
    Ok(game)
}

/// Capture only native process/build inputs and installation eligibility. Library
/// presentation and unselected installation metadata cannot revoke this reader.
fn preparation_installation(game: &Value) -> Value {
    const FIELDS: &[&str] = &[
        "id", "provider", "integrationKey", "integrationCompatible", "executable",
        "installPath", "processExecutables", "selectedInstallationId", "installed",
        "processMonitoringSupported", "launchSupported",
    ];
    Value::Object(FIELDS.iter().filter_map(|key| game.get(*key)
        .map(|value| ((*key).to_owned(), value.clone()))).collect())
}
