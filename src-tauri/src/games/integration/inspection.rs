use super::*;
impl Inner {
    pub(super) fn cancel(&self) {
        // Publication that already owns this gate completes before cancellation;
        // otherwise the worker observes cancellation before replacing the cache.
        let _publication = self.ac_cars.lock().unwrap_or_else(|p| p.into_inner());
        self.cancel.store(true, Ordering::Release);
    }
    pub(super) fn decorate_removal(&self, id: &str, status: &mut Value) {
        status["installed"] = json!(false);
        status["removable"] = json!(false);
        if id != "dcs" {
            let saved = self.saved.lock().unwrap_or_else(|p| p.into_inner());
            let installed = saved["games"][id]["connection"].is_object();
            status["installed"] = json!(installed);
            status["removable"] = json!(installed && self.native && self.settings_error.is_none());
            return;
        }
        let Some(target) = status["userFolder"].as_str() else {
            return;
        };
        let target = Path::new(target);
        let present = target.join("Scripts/DCS-BIOS").exists();
        match crate::dcs_setup::removal_available(target, &self.work) {
            Ok(owned) => {
                status["installed"] = json!(present || owned);
                status["removable"] =
                    json!(owned && !crate::dcs_setup::recovery_pending(&self.work));
                if present && !owned {
                    status["removalMessage"] = json!("This DCS-BIOS installation is managed outside Cockpit. Remove it with its original installer.");
                }
            }
            Err(error) => {
                status["installed"] = json!(present);
                status["removalMessage"] = json!(error);
            }
        }
    }
    #[cfg(test)]
    pub(super) fn accept_acr_proof(&self, proof: &Value) -> Result<(), String> {
        self.accept_assetto_proof("assetto-corsa-rally", proof)
    }
    pub(super) fn accept_assetto_proof(&self, id: &str, proof: &Value) -> Result<(), String> {
        let mut draft = self.saved.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let record = &mut draft["games"][id];
        if !record.is_object() || record["configuredFingerprint"] != proof["fingerprint"] {
            return Ok(());
        }
        if proof["nativeVerified"] == true {
            record["verifiedFingerprint"] = proof["fingerprint"].clone();
            record["verificationIssue"] = Value::Null;
        } else if blocking_acr_status(proof) {
            record["verificationIssue"] = proof["diagnostics"][0].clone();
            if matches!(
                proof["status"].as_str(),
                Some("unsupported-layout" | "executable-changed")
            ) {
                record["verifiedFingerprint"] = Value::Null;
            }
        } else {
            return Ok(());
        }
        write_settings(self, &draft)?;
        *self.saved.lock().unwrap_or_else(|p| p.into_inner()) = draft;
        Ok(())
    }

    pub(super) fn inspect_acr(&self, game: &Value) -> Value {
        let id = game["id"].as_str().unwrap_or("assetto-corsa-rally");
        let telemetry_game = match crate::game_telemetry::Game::from_id(id) {
            Ok(game) => game,
            Err(error) => return json!({"state":"needs-attention","message":error}),
        };
        let saved = self.saved.lock().unwrap_or_else(|p| p.into_inner())["games"][id].clone();
        let mut status = json!({"state":"setup-required","message":"Set up Assetto native telemetry. No extra module or game-file changes are required.","verified":false});
        if !saved["connection"].is_object() {
            return status;
        }
        status["connection"] = saved["connection"].clone();
        let inspected = (|| {
            validate_connection(id, &saved["connection"])?;
            let executable = game["executable"]
                .as_str()
                .ok_or("Choose the Assetto executable.")?;
            self.fingerprints
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .inspect(Path::new(executable), telemetry_game, false)
        })();
        match inspected {
            Err(error) => {
                status["state"] = json!("needs-attention");
                status["message"] = json!(error);
            }
            Ok(identity) => {
                status["version"] = identity["fileVersion"].clone();
                if saved["configuredFingerprint"] != identity {
                    status["state"] = json!("needs-attention");
                    status["message"]=json!("The Assetto executable/build changed. Set up telemetry again and verify a live driving session.");
                } else if let Some(issue) = saved["verificationIssue"].as_str() {
                    status["state"] = json!("needs-attention");
                    status["message"] = json!(issue);
                } else if saved["verifiedFingerprint"] == identity {
                    status["state"] = json!("ready");
                    status["verified"] = json!(true);
                    status["message"]=json!("Native Assetto telemetry was verified for this exact executable. Waiting for a live session when the game is closed.");
                } else {
                    status["state"] = json!("needs-attention");
                    status["message"]=json!("Telemetry is configured. This game build needs its first native verification in a live driving session.");
                }
            }
        }
        status
    }
    pub(super) fn inspect(&self, game: &Value, override_folder: Option<&Value>) -> Value {
        if let Some(error) = &self.settings_error {
            return json!({"state":"needs-attention","message":error,"settingsError":true,"recoveryNeeded":crate::dcs_setup::recovery_pending(&self.work)});
        }
        if !self.native {
            return json!({"state":"unsupported-platform","message":"Native game integration setup requires Windows. Profile editing and installation metadata remain available."});
        }
        if game["installed"] != true {
            return json!({"state":"not-installed","message":"Find the game installation first."});
        }
        if game["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("x-plane-"))
        {
            let id = game["id"].as_str().unwrap();
            let saved = self.saved.lock().unwrap_or_else(|p| p.into_inner())["games"][id].clone();
            if saved["executable"] != game["executable"]
                || validate_connection(id, &saved["connection"]).is_err()
            {
                return json!({"state":"setup-required","message":"Set up RREF UDP for this X-Plane installation. No game files are changed.","verified":false});
            }
            return json!({"state":"ready","connection":saved["connection"],"verified":false,"message":"RREF UDP configured. Start a flight in the selected installation. Check its UDP receive port (default 49000), firewall, pause/replay and the live status separately. Simulator/hardware qualification is pending."});
        }
        if game["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("assetto-corsa"))
        {
            return self.inspect_acr(game);
        }
        let saved = self.saved.lock().unwrap_or_else(|p| p.into_inner())["games"]["dcs"].clone();
        let candidates =
            crate::dcs_setup::discover_user_folders(game["installPath"].as_str().map(Path::new))
                .unwrap_or_default();
        let chosen = override_folder
            .filter(|v| !v.is_null())
            .cloned()
            .or_else(|| saved["userFolder"].as_str().map(|s| json!(s)))
            .or_else(|| (candidates.len() == 1).then(|| json!(candidates[0])));
        let mut status = json!({"state":"setup-required","message":"Choose the DCS Saved Games user folder, then set up integration.","candidates":candidates,"userFolder":chosen,"version":crate::dcs_setup::VERSION,"recoveryNeeded":crate::dcs_setup::recovery_pending(&self.work)});
        if status["recoveryNeeded"] == true {
            status["state"] = json!("needs-attention");
            status["message"] = json!("A previous setup was interrupted. Recover its owned file changes before continuing.");
            return status;
        }
        let Some(chosen) = chosen else {
            return status;
        };
        match folder(&chosen).and_then(|target| {
            reject_game_install_folder(&target, game)?;
            crate::dcs_setup::inspect_for_installation(
                &target,
                game["installPath"].as_str().map(Path::new),
            )
        }) {
            Err(error) => {
                status["message"] = json!(error);
            }
            Ok(inspected) => {
                status["cataloguePath"] = inspected["cataloguePath"].clone();
                status["diagnostics"] = inspected["diagnostics"].clone();
                status["repairable"] = inspected["repairable"].clone();
                status["aircraftExports"] = inspected["aircraftExports"].clone();
                for field in [
                    "physicsExportsReady",
                    "physicsChannelCount",
                    "physicsCatalogueRevision",
                    "physicsExportRevision",
                    "physicsPort",
                ] {
                    status[field] = inspected[field].clone();
                }
                if inspected["ready"] == true {
                    let connection = if saved["userFolder"] == chosen
                        && saved["connection"].is_object()
                    {
                        saved["connection"].clone()
                    } else {
                        dcs_connection(Path::new(inspected["cataloguePath"].as_str().unwrap_or("")))
                    };
                    status["connection"] = connection.clone();
                    match compatible_dcs_connection(&connection) {
                        Ok(()) => {
                            status["state"] = json!("ready");
                            status["message"] = json!("DCS-BIOS, ownship physics exports, catalogue and local connection settings are ready.");
                        }
                        Err(error) => {
                            status["state"] = json!("needs-attention");
                            status["message"] = json!(error);
                        }
                    }
                } else if inspected["repairable"] != true {
                    status["state"] = json!("needs-attention");
                    status["message"] = inspected["diagnostics"][0].clone();
                } else {
                    status["message"] = json!("DCS-BIOS or its ownship telemetry export needs setup. Existing scripts will be preserved.");
                }
            }
        }
        status
    }
}
