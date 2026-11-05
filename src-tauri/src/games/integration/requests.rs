use super::*;
impl GameIntegrationSetup {
    pub(crate) fn request(
        &self,
        request: &Value,
        game: &Value,
        platform: Arc<dyn Platform>,
    ) -> Result<(), String> {
        let bound = bound_game(game)?;
        let game = &bound;
        let _admission = self.admission.lock().unwrap_or_else(|p| p.into_inner());
        if self.inner.closed.load(Ordering::Acquire) {
            return Err("The integration setup runtime has closed.".into());
        }
        let command = request["command"].as_str().unwrap_or("");
        if command == "game_integration_cancel" {
            let key = format!("setup:{}", game["id"].as_str().unwrap_or(""));
            let job = self.inner.job.lock().unwrap_or_else(|p| p.into_inner()).clone();
            if !job.as_ref().is_some_and(|(active_key, run)| {
                active_key == &key && request["jobId"].as_str() == Some(run.as_str())
            }) {
                return Err("The integration setup job changed or finished. Refresh its status before cancelling.".into());
            }
            return self.inner.cancel();
        }
        if self.busy() {
            return Err("Wait for integration setup or cancel it first.".into());
        }
        let id = game["id"]
            .as_str()
            .ok_or("Choose an available telemetry integration.")?;
        if command == "game_integration_settings_save" {
            return self.save_provider_settings(request, game);
        }
        if command == "game_integration_check"
            && game["integrationCompatible"] != false
            && id.starts_with("assetto-corsa")
            && self.inner.native
            && self.inner.settings_error.is_none()
        {
            return self.begin_acr(request, game, platform, false);
        }
        if command == "game_integration_check" {
            let (record, status) = self.inner.inspect_scoped(game);
            let mut checked_preview = None;
            if id == "dcs" {
                if let Some(folder) = request
                    .get("userFolder")
                    .filter(|folder| !folder.is_null() && *folder != &status["userFolder"])
                {
                    let mut preview = self.inner.inspect(game, Some(folder));
                    self.inner.decorate_removal(id, &mut preview);
                    // Keep the checked folder and its actual connection intact.
                    // Provider preferences belong to the canonical main status.
                    checked_preview = Some(preview);
                }
            }
            self.inner
                .publish_check_preview(game, &record, checked_preview);
            self.inner.publish_setup(game, &record, status);
            return Ok(());
        }
        if !self.inner.native {
            return Err("Native integration setup is available on Windows.".into());
        }
        if command == "game_integration_recover" {
            if id != "dcs" {
                return Err("DCS setup recovery belongs to the DCS game.".into());
            }
            if platform.find(game)?.is_some() {
                return Err("Close DCS before restoring its integration files.".into());
            }
            let recovery = crate::dcs_setup::recover(&self.inner.work);
            // A failed attempt can leave durable recovery evidence. Refresh
            // the passive view before replying to either command outcome.
            let (record, status) = self.inner.inspect_scoped(game);
            self.inner.publish_setup(game, &record, status);
            let restored = recovery?;
            if restored["restored"] != true {
                return Err(format!(
                    "Setup recovery preserved external changes: {}",
                    restored["conflicts"]
                ));
            }
            return Ok(());
        }
        if let Some(error) = &self.inner.settings_error {
            return Err(error.clone());
        }
        if command == "game_integration_remove" {
            return self.remove(request, game, platform);
        }
        if command == "game_integration_setup"
            && crate::game_discovery::integration_key(game).is_none()
        {
            return Err(
                "Choose a compatible executable before setting up this integration.".into(),
            );
        }
        if command == "game_integration_setup" && id.starts_with("x-plane-") {
            if game["installed"] != true {
                return Err("Choose an X-Plane installation first.".into());
            }
            let executable = game["executable"]
                .as_str()
                .ok_or("Choose the X-Plane executable.")?;
            platform.validate(id, executable)?;
            let version = id.trim_start_matches("x-plane-").parse::<u64>().unwrap();
            let base = settings_snapshot(&self.inner);
            let mut connection = base["games"][id]["connection"]
                .as_object()
                .map(|_| base["games"][id]["connection"].clone())
                .unwrap_or_else(|| crate::xplane::defaults(version));
            if let Some(overrides) = request.get("connection") {
                for (k, v) in overrides
                    .as_object()
                    .ok_or("Invalid X-Plane connection overrides.")?
                {
                    if !["host", "port", "listenPort", "updateHz", "freshnessMs"]
                        .contains(&k.as_str())
                    {
                        return Err("Unsupported X-Plane setup override.".into());
                    }
                    connection[k] = v.clone();
                }
            }
            validate_connection(id, &connection)?;
            let mut draft = base.clone();
            draft["games"][id] = json!({"connection":connection,"executable":executable});
            commit_settings(&self.inner, &base, draft)?;
            self.refresh_admitted(std::slice::from_ref(game));
            return Ok(());
        }
        if command == "game_integration_setup" && id.starts_with("assetto-corsa") {
            return self.begin_acr(request, game, platform, true);
        }
        if command != "game_integration_setup" || id != "dcs" {
            return Err("Automatic setup for this game is not yet available.".into());
        }
        if game["installed"] != true {
            return Err("Find the game installation before setting up integration.".into());
        }
        if self.pending() {
            return Err("Recover the interrupted DCS setup before installing again.".into());
        }
        if platform.find(game)?.is_some() {
            return Err(
                "Close DCS before changing its export scripts, then run setup again.".into(),
            );
        }
        let checked = self.inner.inspect(game, request.get("userFolder"));
        let target = folder(&checked["userFolder"])?;
        reject_game_install_folder(&target, game)?;
        let saved =
            self.inner.saved.lock().unwrap_or_else(|p| p.into_inner())["games"]["dcs"].clone();
        let mut connection = dcs_connection(&target.join("Scripts/DCS-BIOS/doc/json"));
        if let Some(previous) = saved["connection"].as_object() {
            for (k, v) in previous {
                if k != "cataloguePath" {
                    connection[k] = v.clone();
                }
            }
        }
        if let Some(overrides) = request.get("connection") {
            let values = overrides
                .as_object()
                .ok_or("Invalid connection overrides.")?;
            for (k, v) in values {
                if k != "cataloguePath" {
                    connection[k] = v.clone();
                }
            }
        }
        validate_connection(id, &connection)?;
        self.inner.cancel.store(false, Ordering::Release);
        self.inner.busy.store(true, Ordering::Release);
        let run = self.inner.publish_start(game, json!({"state":"setting-up","message":"Preparing DCS-BIOS setup.","userFolder":target,"version":crate::dcs_setup::VERSION,"progress":"Preparing"}));
        let inner = self.inner.clone();
        let game = game.clone();
        let mut worker = self.worker.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(previous) = worker.take() {
            let _ = previous.join();
        }
        *worker = Some(thread::spawn(move || {
            let result = crate::dcs_setup::setup_for_installation(
                game["installPath"].as_str().map(Path::new),
                &target,
                &inner.work,
                || inner.cancel.load(Ordering::Acquire),
                |message, percent| {
                    inner.publish_progress(&game, message, Some(percent));
                },
                || {
                    if platform.find(&game)?.is_some() {
                        Err("DCS started during integration setup. Close DCS, then run setup again before its export scripts are changed.".into())
                    } else {
                        Ok(())
                    }
                },
            )
            .and_then(|_| {
                let base = settings_snapshot(&inner);
                let mut draft = base.clone();
                let record=&mut draft["games"]["dcs"];
                if !record.is_object(){*record=json!({});}
                record["userFolder"]=json!(target);record["connection"]=connection;
                commit_settings(&inner, &base, draft)?;
                inner.publish_progress(&game, "Finalizing DCS integration setup", None);
                Ok(())
            });
            let (record, mut status) = inner.inspect_scoped(&game);
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
}
