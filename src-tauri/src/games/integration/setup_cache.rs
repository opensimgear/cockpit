//! Small immutable setup views. Only source inspection and commands publish;
//! decoration never enters admission, inspects files, or starts work.
use super::*;

// 512 authored titles and the seven built-in discovery entries may coexist.
const MAX_SCOPES: usize = 519;
const MAX_VIEW_BYTES: usize = 256 * 1024;
const MAX_CACHE_BYTES: usize = MAX_SCOPES * MAX_VIEW_BYTES;

#[derive(Default)]
pub(super) struct SetupViews {
    entries: BTreeMap<String, (Arc<Value>, Arc<Value>, usize, u64)>,
    source: BTreeMap<String, (Arc<Value>, Value)>,
    previews: BTreeMap<String, (Arc<Value>, Arc<Value>)>,
    bytes: usize,
    sequence: u64,
}

#[cfg(test)]
#[path = "setup_stream_tests.rs"]
mod stream_tests;

fn source_scope(game: &Value, id: &str) -> String {
    provider_settings::digest(&json!([
        id, game["provider"], game["integrationCompatible"],
        game["executable"], game["installPath"], game["selectedInstallationId"],
        game["installed"], game["processExecutables"]
    ]))
}

impl SetupViews {
    fn insert(&mut self, key: String, record: Arc<Value>, status: Arc<Value>, bytes: usize) {
        if let Some((_, _, old, _)) = self.entries.remove(&key) {
            self.bytes -= old;
        }
        while self.entries.len() >= MAX_SCOPES || self.bytes + bytes > MAX_CACHE_BYTES {
            let Some(oldest) = self.entries.iter().min_by_key(|(_, entry)| entry.3)
                .map(|(key, _)| key.clone()) else { break };
            if let Some((_, _, old, _)) = self.entries.remove(&oldest) {
                self.bytes -= old;
            }
        }
        // This clock is private eviction order, never an identity/revision.
        // Exhaustion discards display entries rather than reusing an order.
        self.sequence = if let Some(next) = self.sequence.checked_add(1) {
            next
        } else {
            self.entries.clear();
            self.bytes = 0;
            1
        };
        self.bytes += bytes;
        self.entries.insert(key, (record, status, bytes, self.sequence));
    }
}

impl Inner {
    pub(super) fn changed(&self) {
        if let Some(changes) = self.changes.get() { changes.mark(crate::game_read_models::signals::Domain::Setup); }
    }
    pub(super) fn job_progress(&self, phase: &str, message: &str, percent: Option<u8>) {
        let job = self.job.lock().unwrap_or_else(|p| p.into_inner()).clone();
        if let (Some(changes), Some((key, run))) = (self.changes.get(), job) {
            changes.job(crate::game_read_models::signals::Domain::Setup, &key, &run, phase, message, percent, false);
        }
    }
    pub(super) fn finish_job(&self, run: &str, status: &Value) {
        let mut current = self.job.lock().unwrap_or_else(|p| p.into_inner());
        let Some((key, active)) = current.as_ref().filter(|(_, active)| active == run) else { return; };
        let phase = if self.cancel.load(Ordering::Acquire) { "cancelled" }
            else if status["state"] == "needs-attention" { "failed" } else { "complete" };
        if let Some(changes) = self.changes.get() {
            changes.job(crate::game_read_models::signals::Domain::Setup, key, active, phase,
                status["message"].as_str().unwrap_or("Integration setup finished"), None, true);
        }
        *current = None;
    }
    pub(super) fn cached_setup_summary(&self, game: &Value, id: &str) -> Value {
        let key = source_scope(game, id);
        let (record, revision) = self.provider_snapshot(id);
        let status = {
            let views = self.views.lock().unwrap_or_else(|p| p.into_inner());
            views.entries.get(&key).filter(|(known, _, _, _)| known.as_ref() == &record).map(|(_, status, _, _)| Arc::clone(status))
        };
        let Some(status) = status else { return self.cached_setup(game, id); };
        let mut summary = serde_json::Map::new();
        for field in ["state", "message", "installed", "removable", "verified", "recoveryNeeded", "repairable", "settingsError", "connection", "version", "aircraftModule", "userFolder", "progress", "percent", "removalMessage"] {
            if let Some(value) = status.get(field) { summary.insert(field.into(), value.clone()); }
        }
        let mut status = Value::Object(summary);
        status["live"] = json!("waiting");
        status["liveMessage"] = json!("Setup is configuration metadata. Inspect the applied profile for actual source delivery.");
        self.decorate_provider_snapshot(id, &record, &revision, &mut status);
        status
    }
    pub(super) fn retain_setup_scopes(&self, catalogue: &[Value]) {
        let scopes = catalogue.iter().filter_map(|game| {
            crate::game_discovery::integration_binding(game).map(|id| source_scope(game, id))
        }).collect::<std::collections::BTreeSet<_>>();
        let mut views = self.views.lock().unwrap_or_else(|p| p.into_inner());
        views.entries.retain(|key, _| scopes.contains(key));
        views.source.retain(|key, _| scopes.contains(key));
        views.previews.retain(|key, _| scopes.contains(key));
        views.bytes = views.entries.values().map(|entry| entry.2).sum();
    }
    pub(super) fn cached_source_setup(&self, game: &Value, id: &str) -> Value {
        let key = source_scope(game, id);
        let (record, revision) = self.provider_snapshot(id);
        let captured = {
            let views = self.views.lock().unwrap_or_else(|p| p.into_inner());
            views.source.get(&key).filter(|(known, _)| known.as_ref() == &record)
                .map(|(_, status)| status.clone())
        };
        // Source readiness is conservatively withheld throughout any worker.
        if self.busy.load(Ordering::Acquire) {
            return json!({"state":"setting-up","verified":false,"providerSetupRevision":revision});
        }
        let mut status = captured.unwrap_or_else(|| json!({"state":"checking","verified":false}));
        status["providerSetupRevision"] = json!(revision);
        status
    }
    pub(super) fn cached_setup(&self, game: &Value, id: &str) -> Value {
        let key = source_scope(game, id);
        let (record, revision) = self.provider_snapshot(id);
        let captured = {
            let views = self.views.lock().unwrap_or_else(|p| p.into_inner());
            views.entries.get(&key)
                .filter(|(known, _, _, _)| known.as_ref() == &record)
                .map(|(_, status, _, _)| Arc::clone(status))
        };
        let mut status = captured.map(|status| status.as_ref().clone()).unwrap_or_else(|| {
            if let Some(error) = &self.settings_error {
                json!({"state":"needs-attention","message":error,"settingsError":true,
                    "recoveryNeeded":self.recovery_needed.load(Ordering::Acquire)})
            } else if !self.native {
                json!({"state":"unsupported-platform","message":"Native game integration setup requires Windows. Profile editing and installation metadata remain available."})
            } else if game["integrationCompatible"] == false {
                json!({"state":"setup-required","message":"Choose a compatible executable from this installation before preparing telemetry.","verified":false})
            } else if game["installed"] != true {
                json!({"state":"not-installed","message":"Find the game installation first."})
            } else {
                json!({"state":"checking","verified":false,"message":"Checking integration setup metadata for this installation."})
            }
        });
        status["live"] = json!("waiting");
        status["liveMessage"] = json!("Setup is configuration metadata. Inspect the applied profile for actual source delivery.");
        self.decorate_provider_snapshot(id, &record, &revision, &mut status);
        status
    }

    pub(super) fn inspect_scoped(&self, game: &Value) -> (Value, Value) {
        let id = game["id"].as_str().unwrap_or("");
        let record = self.provider_snapshot(id).0;
        #[cfg(test)]
        if let Some((entered, release)) = self.inspection_pause.lock()
            .unwrap_or_else(|p| p.into_inner()).take()
        {
            entered.send(()).unwrap();
            release.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        }
        // Source publication always inspects the configured/default folder.
        // An explicit folder check is a separate preview, never this proof.
        let mut status = self.inspect(game, None);
        self.prepare_setup(id, &mut status);
        (record, status)
    }

    fn prepare_setup(&self, id: &str, status: &mut Value) {
        if let Some(needed) = status["recoveryNeeded"].as_bool() {
            self.recovery_needed.store(needed, Ordering::Release);
        }
        if id == "dcs" {
            status["version"] = json!(crate::dcs_setup::VERSION);
        }
        self.decorate_removal(id, status);
        self.decorate_provider_settings(id, status);
    }

    pub(super) fn publish_check_preview(&self, game: &Value, record: &Value, preview: Option<Value>) {
        let key = source_scope(game, game["id"].as_str().unwrap_or(""));
        let preview = preview.map(|preview| {
            let mut selected = serde_json::Map::new();
            for field in ["state", "message", "userFolder", "connection", "installed",
                "removable", "removalMessage", "repairable", "verified"] {
                if let Some(value) = preview.get(field) {
                    selected.insert(field.into(), value.clone());
                }
            }
            let selected = Value::Object(selected);
            if serde_json::to_vec(&selected).expect("JSON preview").len() > MAX_VIEW_BYTES {
                Arc::new(json!({"state":"unavailable","message":"Folder check metadata exceeds the display limit."}))
            } else {
                Arc::new(selected)
            }
        });
        let id = game["id"].as_str().unwrap_or("");
        let record_copy = Arc::new(record.clone());
        let saved = self.saved.lock().unwrap_or_else(|p| p.into_inner());
        if &saved["games"][id] != record { return; }
        let mut views = self.views.lock().unwrap_or_else(|p| p.into_inner());
        views.previews.remove(&key);
        if let Some(preview) = preview {
            if views.previews.len() >= MAX_SCOPES {
                // Full refresh retains current source scopes. Bound previews
                // from standalone commands as well, independently of proofs.
                if let Some(oldest) = views.previews.keys().next().cloned() {
                    views.previews.remove(&oldest);
                }
            }
            views.previews.insert(key, (record_copy, preview));
        }
    }

    pub(super) fn publish_setup(&self, game: &Value, record: &Value, mut status: Value) {
        let id = game["id"].as_str().unwrap_or("");
        let key = source_scope(game, id);
        let preview = {
            let views = self.views.lock().unwrap_or_else(|p| p.into_inner());
            views.previews.get(&key).filter(|(known, _)| known.as_ref() == record)
                .map(|(_, preview)| Arc::clone(preview))
        };
        if let Some(object) = status.as_object_mut() {
            object.remove("checkPreview");
        }
        if let Some(preview) = preview {
            status["checkPreview"] = preview.as_ref().clone();
        }
        // Repeated passive inspections keep the same immutable view and avoid
        // encoding a large identical catalogue every source polling interval.
        {
            let saved = self.saved.lock().unwrap_or_else(|p| p.into_inner());
            if &saved["games"][id] != record { return; }
            let views = self.views.lock().unwrap_or_else(|p| p.into_inner());
            let states = self.states.lock().unwrap_or_else(|p| p.into_inner());
            if views.entries.get(&key).is_some_and(|(known, _, _, _)| known.as_ref() == record)
                && states.get(id).is_some_and(|old| old.as_ref() == &status) { return; }
        }
        let raw_status = Arc::new(status.clone());
        let source_state = match status["state"].as_str() {
            Some(state @ ("ready" | "setting-up" | "needs-attention" | "setup-required"
                | "not-installed" | "unsupported-platform")) => state,
            _ => "checking",
        };
        let source_status = json!({"state":source_state,
            "verified":status["verified"] == true});
        if let Some(object) = status.as_object_mut() {
            object.remove("channels");
            object.remove("optionalMaxRpm");
        }
        {
            let saved = self.saved.lock().unwrap_or_else(|p| p.into_inner());
            if &saved["games"][id] != record { return; }
            let views = self.views.lock().unwrap_or_else(|p| p.into_inner());
            if views.entries.get(&key).is_some_and(|(known, previous, _, _)| known.as_ref() == record && previous.as_ref() == &status)
                && views.source.get(&key).is_some_and(|(known, previous)| known.as_ref() == record && previous == &source_status) {
                self.states.lock().unwrap_or_else(|p| p.into_inner()).insert(id.into(), raw_status);
                return;
            }
        }
        // Normal configured records are bounded by MAX_SETTINGS (32 KiB),
        // plus small fixed provider diagnostics/exports. Candidate discovery
        // can be larger; that affects display availability only.
        let mut bytes = serde_json::to_vec(&status).expect("JSON view").len();
        if bytes > MAX_VIEW_BYTES {
            status = json!({"state":"unavailable","verified":false,
                "message":"Integration setup metadata is unavailable for display because it exceeds the display limit."});
            bytes = serde_json::to_vec(&status).expect("JSON view").len();
        }
        let captured = Arc::new(status);
        let record_copy = Arc::new(record.clone());
        let saved = self.saved.lock().unwrap_or_else(|p| p.into_inner());
        // An inspection that overlapped a settings/proof write is never tagged
        // with the new record, even if it finished after that write.
        if &saved["games"][id] != record {
            return;
        }
        let mut views = self.views.lock().unwrap_or_else(|p| p.into_inner());
        if views.previews.get(&key).is_some_and(|(known, _)| known.as_ref() != record) {
            views.previews.remove(&key);
        }
        // Source proofs have no display budget or LRU. Full source refresh
        // retains current scopes; overflow conservatively leaves a cold proof.
        if views.source.contains_key(&key) || views.source.len() < MAX_SCOPES {
            views.source.insert(key.clone(), (Arc::clone(&record_copy), source_status));
        }
        views.insert(key, record_copy, captured, bytes);
        self.states.lock().unwrap_or_else(|p| p.into_inner()).insert(id.into(), raw_status);
        drop(views); drop(saved);
        self.changed();
    }

    pub(super) fn publish_start(&self, game: &Value, mut status: Value) -> String {
        let id = game["id"].as_str().unwrap_or("");
        let run = uuid::Uuid::new_v4().to_string();
        let key = format!("setup:{id}");
        *self.job.lock().unwrap_or_else(|p| p.into_inner()) = Some((key.clone(), run.clone()));
        if let Some(changes) = self.changes.get() { changes.start_job(crate::game_read_models::signals::Domain::Setup,
            &key, &run, "setting-up", status["progress"].as_str().or_else(|| status["message"].as_str()).unwrap_or("Preparing integration setup"), None); }
        let record = self.provider_snapshot(id).0;
        self.prepare_setup(id, &mut status);
        self.publish_setup(game, &record, status);
        run
    }

    pub(super) fn publish_progress(&self, game: &Value, message: &str, percent: Option<u8>) {
        self.job_progress("setting-up", message, percent);
        let id = game["id"].as_str().unwrap_or("");
        let record = self.provider_snapshot(id).0;
        let status = self.states.lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned();
        if let Some(status) = status {
            let mut status = status.as_ref().clone();
            status["progress"] = json!(message);
            if let Some(percent) = percent {
                status["percent"] = json!(percent);
            }
            self.decorate_provider_settings(id, &mut status);
            self.publish_setup(game, &record, status);
        }
    }
}
