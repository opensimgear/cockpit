use super::*;
impl GameIntegrationSetup {
    pub(super) fn remove(
        &self,
        request: &Value,
        game: &Value,
        platform: Arc<dyn Platform>,
    ) -> Result<(), String> {
        let id = game["id"]
            .as_str()
            .ok_or("Choose an available telemetry integration.")?;
        if self.pending() {
            return Err(
                "Recover the interrupted DCS integration operation before removing integration."
                    .into(),
            );
        }
        if id != "dcs" {
            if !(id.starts_with("assetto-corsa") || matches!(id, "x-plane-11" | "x-plane-12")) {
                return Err("Choose an available telemetry integration to remove.".into());
            }
            // These providers use the game's native APIs and install no game files.
            let base = settings_snapshot(&self.inner);
            let mut draft = base.clone();
            draft["games"].as_object_mut().unwrap().remove(id);
            commit_settings(&self.inner, &base, draft)?;
            self.refresh_admitted(std::slice::from_ref(game));
            return Ok(());
        }
        if platform.find(game)?.is_some() {
            return Err("Close DCS before removing its integration files.".into());
        }
        let checked = self.inner.inspect(game, request.get("userFolder"));
        let target = folder(&checked["userFolder"])?;
        reject_game_install_folder(&target, game)?;
        if !crate::dcs_setup::removal_available(&target, &self.inner.work)? {
            return Err("This DCS-BIOS installation was not installed by Cockpit. Its files were preserved; remove it with its original installer.".into());
        }
        self.inner.cancel.store(false, Ordering::Release);
        self.inner.busy.store(true, Ordering::Release);
        let run = self.inner.publish_start(game, json!({"state":"setting-up","message":"Removing DCS integration.","progress":"Restoring export scripts and removing owned files","userFolder":target,"version":crate::dcs_setup::VERSION,"installed":true,"removable":true}));
        let inner = self.inner.clone();
        let game = game.clone();
        let mut worker = self.worker.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(previous) = worker.take() {
            let _ = previous.join();
        }
        *worker = Some(thread::spawn(move || {
            let result = crate::dcs_setup::remove(
                &target,
                &inner.work,
                || inner.cancel.load(Ordering::Acquire),
                |message, percent| {
                    inner.publish_progress(&game, message, Some(percent));
                },
                || {
                    if platform.find(&game)?.is_some() {
                        Err(
                            "DCS started during integration removal. Close DCS before continuing."
                                .into(),
                        )
                    } else {
                        Ok(())
                    }
                },
            )
            .and_then(|_| {
                let base = settings_snapshot(&inner);
                let mut draft = base.clone();
                draft["games"].as_object_mut().unwrap().remove("dcs");
                commit_settings(&inner, &base, draft)?;
                inner.publish_progress(&game, "Finalizing DCS integration removal", None);
                Ok(())
            });
            let (record, mut status) = inner.inspect_scoped(&game);
            if status["userFolder"].is_null() {
                // Preserve post-removal ownership information for the removed
                // target without using an override folder as a readiness proof.
                status["userFolder"] = json!(target);
                inner.decorate_removal("dcs", &mut status);
            }
            if let Err(error) = result {
                status["state"] = json!("needs-attention");
                status["message"] = json!(error);
            }
            inner.publish_setup(&game, &record, status.clone());
            inner.finish_job(&run, &status);
            inner.busy.store(false, Ordering::Release);
            inner.changed();
        }));
        Ok(())
    }
    pub(super) fn begin_acr(
        &self,
        request: &Value,
        game: &Value,
        platform: Arc<dyn Platform>,
        configure: bool,
    ) -> Result<(), String> {
        let id = game["id"]
            .as_str()
            .ok_or("Choose an Assetto game.")?
            .to_owned();
        let telemetry_game = crate::game_telemetry::Game::from_id(&id)?;
        if game["installed"] != true {
            return Err("Find the Assetto installation before setting up telemetry.".into());
        }
        let previous =
            self.inner.saved.lock().unwrap_or_else(std::sync::PoisonError::into_inner)["games"][&id].clone();
        let mut connection = if previous["connection"].is_object() {
            previous["connection"].clone()
        } else {
            json!({"source":"shared-memory","freshnessMs":1500})
        };
        if configure {
            if let Some(overrides) = request.get("connection") {
                for (k, v) in overrides
                    .as_object()
                    .ok_or("Invalid Assetto connection overrides.")?
                {
                    connection[k] = v.clone();
                }
            }
        }
        validate_connection(&id, &connection)?;
        let executable = game["executable"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("Choose the Assetto executable.")?;
        let executable = PathBuf::from(executable);
        if !executable.is_absolute() || !executable.is_file() {
            return Err("Choose an existing absolute Assetto executable.".into());
        }
        if self
            .inner
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err("An integration setup check is already running.".into());
        }
        self.inner.cancel.store(false, Ordering::Release);
        let publication = (configure && id == "assetto-corsa")
            .then(|| Arc::new(car_publication::Publication::default()));
        *self
            .inner
            .ac_cars_publication
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = publication.clone();
        let run = self.inner.publish_start(game, json!({"state":"setting-up","message":"Checking Assetto native telemetry. No plugin or game-file changes are required.","progress":"Checking executable and native telemetry","connection":connection,"verified":false}));
        let inner = self.inner.clone();
        let game = game.clone();
        let mut worker = self.worker.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(previous_worker) = worker.take() {
            let _ = previous_worker.join();
        }
        *worker = Some(thread::spawn(move || {
            let result: Result<Value, String> = (|| {
                let identity = inner
                    .fingerprints
                    .inspect(&executable, telemetry_game, true)?;
                if inner.cancel.load(Ordering::Acquire) {
                    return Err("Assetto telemetry check was cancelled.".into());
                }
                if configure {
                    let base = settings_snapshot(&inner);
                    let mut draft = base.clone();
                    let verified = if previous["verifiedFingerprint"] == identity {
                        identity.clone()
                    } else {
                        Value::Null
                    };
                    let issue = if previous["configuredFingerprint"] == identity {
                        previous["verificationIssue"].clone()
                    } else {
                        Value::Null
                    };
                    draft["games"][&id] = json!({"connection":connection,"configuredFingerprint":identity,"verifiedFingerprint":verified,"verificationIssue":issue});
                    commit_settings(&inner, &base, draft)?;
                    inner.publish_progress(&game, "Checking executable and native telemetry", None);
                }
                let cars = if configure && id == "assetto-corsa" {
                    inner.publish_progress(&game, "Scanning installed Assetto Corsa cars", None);
                    let root = game["installPath"]
                        .as_str()
                        .ok_or("Choose the Assetto Corsa installation before scanning cars.")?;
                    Some(crate::ac_car_catalogue::scan(Path::new(root))?)
                } else {
                    None
                };
                if inner.cancel.load(Ordering::Acquire) {
                    return Err("Assetto telemetry check was cancelled.".into());
                }
                // Tests pause the same pre-probe boundary on every host; the
                // production probe itself correctly avoids Windows process I/O
                // on other platforms.
                #[cfg(test)]
                if let Some((entered, release)) = inner
                    .native_probe_pause
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
                {
                    entered.send(()).unwrap();
                    release
                        .recv_timeout(std::time::Duration::from_secs(15))
                        .unwrap();
                }
                let wake = Arc::new(osg_streams::NativeWake::default());
                *inner.assetto_setup_wake.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(wake.clone());
                let proof = crate::acr_setup::probe(
                    &game,
                    platform.as_ref(),
                    wake,
                    || inner.cancel.load(Ordering::Acquire),
                    |message, percent| {
                        inner.publish_progress(&game, message, Some(percent));
                    },
                );
                inner.assetto_setup_wake.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
                let proof = proof?;
                inner.accept_assetto_proof(&id, &proof)?;
                inner.publish_progress(&game, "Finalizing native telemetry verification", None);
                if let Some(cars) = cars {
                    if inner.cancel.load(Ordering::Acquire) {
                        return Err("Assetto telemetry check was cancelled.".into());
                    }
                    #[cfg(test)]
                    if let Some((entered, release)) = inner
                        .ac_cars_publication_pause
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .take()
                    {
                        entered
                            .send(())
                            .map_err(|_| "Catalogue publication test disconnected.")?;
                        release
                            .recv_timeout(std::time::Duration::from_secs(5))
                            .map_err(|_| "Catalogue publication test timed out.")?;
                    }
                    inner.ac_cars.replace_if(&game, cars, || {
                        publication
                            .as_ref()
                            .expect("AC setup owns its publication token")
                            .begin_commit()
                    })?;
                }
                Ok(proof)
            })();
            if let Some(publication) = publication {
                publication.finish();
            }
            let (record, mut status) = inner.inspect_scoped(&game);
            match result {
                Ok(proof) => {
                    let channels = proof["channels"].as_object().map(|rows| rows.iter().map(|(name,channel)| (name.clone(),json!({"status":if channel["status"]=="ready" {"available"} else {"unknown"}}))).collect::<serde_json::Map<String,Value>>()).unwrap_or_default();
                    status["probeChannels"] = json!(channels);
                    status["optionalMaxRpm"] = json!(proof["maxRpmAvailable"] == true);
                    if proof["nativeVerified"] != true && status["verified"] != true {
                        status["message"] = proof["diagnostics"][0].as_str().map(|s| json!(s)).unwrap_or_else(|| json!("Telemetry is configured. Start a live Assetto stage to verify this game build."));
                    }
                    if blocking_acr_status(&proof) {
                        status["state"] = json!("needs-attention");
                        status["verified"] = json!(false);
                        status["message"] = proof["diagnostics"][0].clone();
                    }
                }
                Err(error) => {
                    status["state"] = json!("needs-attention");
                    status["message"] = json!(error);
                }
            }
            inner.publish_setup(&game, &record, status.clone());
            inner.finish_job(&run, &status);
            inner.busy.store(false, Ordering::Release);
            inner.changed();
        }));
        Ok(())
    }
}
