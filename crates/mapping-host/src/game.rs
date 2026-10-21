//! Captured game intent is immutable. It never replaces the user's saved editor documents.
use super::*;
use osg_mapping::{engine::validate_assignments, profile::Profile};
use osg_virtual_controller::vjoy::Binding;
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Capture {
    pub profile: Profile,
    pub assignments: Vec<Assignment>,
    pub targets: Vec<Binding>,
    pub visibility: Option<Value>,
}
impl Capture {
    pub fn parse(value: &Value) -> Result<Self, String> {
        Self::parse_mode(value, false)
    }
    /// Prior runs accepted by game_snapshot are recording previews. Profile
    /// requirements must never turn a resumed preview into live output/hiding.
    pub fn parse_recording(value: &Value) -> Result<Self, String> {
        Self::parse_mode(value, true)
    }
    fn parse_mode(value: &Value, recording: bool) -> Result<Self, String> {
        if value.to_string().len() > 65536 {
            return Err("Captured mapping exceeds 64 KiB.".into());
        }
        let captured: Self = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        captured.profile.compile()?;
        validate_assignments(&captured.profile, &captured.assignments)?;
        if (!recording && captured.profile.requirements.virtual_devices)
            || !captured.targets.is_empty()
        {
            crate::settings::validate_bindings(&captured.profile, &captured.targets)?;
        }
        if (recording && captured.visibility.is_some())
            || (!recording
                && captured.profile.requirements.device_hiding != captured.visibility.is_some())
        {
            return Err(
                "Capture explicit visibility intent only when this mapping requires hiding.".into(),
            );
        }
        if let Some(intent) = &captured.visibility {
            if !intent.is_object()
                || intent.to_string().len() > 8192
                || intent["sourceIds"].as_array().is_none_or(|ids| {
                    ids.is_empty()
                        || ids.len() > 32
                        || ids
                            .iter()
                            .any(|id| id.as_str().is_none_or(|s| s.is_empty() || s.len() > 4096))
                })
                || !matches!(
                    intent["mode"].as_str(),
                    Some("hidhide-session" | "linux-grab" | "macos-seize" | "controlled-launch")
                )
            {
                return Err("Invalid captured visibility intent.".into());
            }
        }
        Ok(captured)
    }
}
#[derive(Default)]
pub(super) struct Lease {
    owner: String,
    expected: Option<RunToken>,
    pub(super) emergency: bool,
}
impl Service {
    pub fn capture_saved(&self, id: &str, visibility: Option<Value>) -> Result<Value, String> {
        let profile = self.store()?.load(id)?;
        let visibility = visibility
            .map(|value| (self.game_visibility)("capture", &value))
            .transpose()?;
        let captured = Capture {
            assignments: self.store()?.load_assignments(id)?,
            targets: self
                .settings
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .value
                .targets
                .get(id)
                .cloned()
                .unwrap_or_default(),
            profile,
            visibility,
        };
        let value = serde_json::to_value(captured).map_err(|e| e.to_string())?;
        Capture::parse(&value)?;
        Ok(value)
    }
    fn active_capture(&self, router: &Router) -> Result<Value, String> {
        if router.runtime.token().is_err() {
            return Ok(Value::Null);
        }
        let trace = router
            .runtime
            .trace
            .trace
            .as_ref()
            .ok_or("Active mapping snapshot unavailable.")?;
        let captured = Capture {
            profile: trace.profile.clone(),
            assignments: trace.assignments.clone(),
            targets: self
                .settings
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .value
                .targets
                .get(&trace.profile.id)
                .cloned()
                .unwrap_or_default(),
            visibility: None,
        };
        serde_json::to_value(captured).map_err(|e| e.to_string())
    }
    pub fn game_preflight(&self, desired: &Value) -> Result<(), String> {
        self.game_prepare(desired, false).map(|_| ())
    }
    fn game_prepare(&self, desired: &Value, recording: bool) -> Result<Value, String> {
        let capture = if recording {
            Capture::parse_recording(desired)?
        } else {
            Capture::parse(desired)?
        };
        if self.capture.visibility_active() || self.provisioning_active() {
            return Err(
                "Stop standalone visibility or provisioning before game activation.".into(),
            );
        }
        if !recording && capture.profile.requirements.virtual_devices {
            if !self.qualified {
                return Err(format!(
                    "Captured virtual output is not qualified: {QUALIFICATION}"
                ));
            }
            let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
            if settings.error.is_some() || !settings.value.virtual_devices {
                return Err("Enable optional virtual output explicitly and repair any settings error before game activation.".into());
            }
            if !capture.profile.scripts.is_empty() && !self.script_live_qualified {
                return Err("Live script output remains disabled pending qualification.".into());
            }
            drop(settings);
            #[cfg(windows)]
            {
                let inventory = (self.inspector)()?;
                for binding in &capture.targets {
                    let slot = inventory
                        .iter()
                        .find(|s| s.id == binding.slot)
                        .ok_or("A captured virtual target is missing.")?;
                    binding.match_slot(slot)?;
                    if slot.status != osg_virtual_controller::vjoy::Status::Free {
                        return Err("A captured virtual target is owned by another feeder.".into());
                    }
                }
            }
            self.confirm_companion_pins(&capture.assignments)?;
            for assignment in capture
                .assignments
                .iter()
                .filter(|a| a.source.origin == Origin::Companion)
            {
                let provider = self.companion_provider()?;
                let descriptor = provider.definition(&assignment.source.id)?;
                provider.require_ready(
                    &assignment.source.id,
                    &descriptor.publication_hash,
                    true,
                )?;
            }
            {
                let _resources = self.reserve_manual(&capture.profile, &capture.targets, true)?;
            }
            // The helper handshake checks dependency readiness without Start/acquisition.
            let mut probe = (self.output_factory)()?;
            probe.terminate();
        }
        if self.recovery_status()["required"] == true {
            return Err(
                "Recover interrupted virtual output before activating a game mapping.".into(),
            );
        }
        // Compile before hardware changes. Physical baselines are deliberately resolved again after re-enumeration.
        let _compile = self
            .compilation
            .try_lock()
            .map_err(|_| "A script compilation is already running.")?;
        for script in &capture.profile.scripts {
            (self.script_factory)(&script.source)?;
        }
        capture
            .visibility
            .as_ref()
            .map(|intent| (self.game_visibility)("preflight", intent))
            .transpose()
            .map(|value| value.unwrap_or(Value::Null))
    }
    pub fn game_snapshot(&self, desired: &Value) -> Result<Value, String> {
        let visibility = self.game_prepare(desired, false)?;
        let router = self.router.lock().unwrap_or_else(|p| p.into_inner());
        if router.live()
            || router.status()["recoveryRequired"] == true
            || router.learner.as_ref().is_some_and(|l| l.active())
        {
            return Err(
                "Stop live output, recovery or input learning before a game session.".into(),
            );
        }
        Ok(
            json!({"owner":uuid::Uuid::new_v4().to_string(),"desired":desired,"visibilityPrepared":visibility,"baseline":self.active_capture(&router)?,"baselineToken":router.runtime.token().ok(),"settings":self.settings.lock().unwrap_or_else(|p|p.into_inner()).value,"taken":false,"armed":false,"stopped":false,"resumed":false}),
        )
    }
    pub fn game_take(&self, journal: &Value) -> Result<(), String> {
        let mut lease = self.game_lease.lock().unwrap_or_else(|p| p.into_inner());
        if lease.is_some() {
            return Err("Another game session owns mapping.".into());
        }
        let owner = journal["owner"]
            .as_str()
            .ok_or("Missing game mapping owner.")?
            .to_string();
        let mut router = self.router.lock().unwrap_or_else(|p| p.into_inner());
        if serde_json::to_value(router.runtime.token().ok()).unwrap() != journal["baselineToken"]
            || self.active_capture(&router)? != journal["baseline"]
        {
            return Err("Mapping changed after game preflight; preserve the newer run.".into());
        }
        *lease = Some(Lease {
            owner,
            expected: None,
            emergency: false,
        });
        router.stop_result()
    }
    fn capture_sources(
        &self,
        capture: &Capture,
    ) -> Result<
        (
            stream::Snapshot,
            Option<crate::panels::Feed>,
            Option<crate::companions::Prepared>,
        ),
        String,
    > {
        let ids = capture
            .assignments
            .iter()
            .filter(|a| a.source.origin == Origin::HostController)
            .map(|a| a.source.id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let page = self.capture.stream_page("", 0);
        let mut snapshot = if ids.is_empty() {
            stream::Snapshot {
                session: page.session,
                cursor: page.cursor,
                captured_us: page.captured_us,
                sources: vec![],
            }
        } else {
            self.capture.stream_snapshot(&ids)?
        };
        let panels = if capture
            .assignments
            .iter()
            .any(|a| a.source.origin == Origin::OsgPanelEvents)
        {
            let prepared = self
                .panel_provider()?
                .prepare(&capture.profile, &capture.assignments)?;
            snapshot.sources.extend(prepared.snapshots);
            snapshot.captured_us = self.capture.stream_page("", 0).captured_us;
            Some(prepared.feed)
        } else {
            None
        };
        let companions = self.prepare_companions(&capture.profile, &capture.assignments)?;
        Ok((snapshot, panels, companions))
    }
    fn start_capture(
        &self,
        capture: Capture,
        epoch: u64,
        owner: Option<&str>,
        visibility: &Value,
        virtual_output: bool,
    ) -> Result<RunToken, String> {
        let _compile = self
            .compilation
            .try_lock()
            .map_err(|_| "A script compilation is already running.")?;
        let cancelled = || {
            if self.lifecycle.load(Ordering::Acquire) != epoch || self.stop.load(Ordering::Acquire)
            {
                Err("Game mapping start cancelled by Stop.".to_owned())
            } else {
                Ok(())
            }
        };
        if virtual_output {
            self.confirm_companion_pins(&capture.assignments)?;
        }
        let _resources = self.reserve_manual(&capture.profile, &capture.targets, virtual_output)?;
        let pinned_sources = if virtual_output {
            capture
                .assignments
                .iter()
                .filter(|a| a.source.origin == Origin::Companion)
                .map(|a| {
                    let provider = self.companion_provider()?;
                    let definition = provider.definition(&a.source.id)?;
                    let generation =
                        provider.require_ready(&a.source.id, &definition.publication_hash, true)?;
                    Ok((a.source.id.clone(), definition.publication_hash, generation))
                })
                .collect::<Result<Vec<_>, String>>()?
        } else {
            vec![]
        };
        let mut workers = std::collections::BTreeMap::new();
        for script in &capture.profile.scripts {
            cancelled()?;
            workers.insert(script.id.clone(), (self.script_factory)(&script.source)?);
        }
        let (mut snapshot, mut panels, mut companions) = self.capture_sources(&capture)?;
        let mut candidate = Router::default();
        if let Some(owner) = owner {
            candidate.runtime = osg_mapping::runtime::Runtime::owned_session(owner)?;
        }
        let cursor = self
            .router
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .runtime
            .recording
            .cursor();
        cancelled()?;
        let mut output = if virtual_output {
            Some(
                osg_virtual_controller::session::Session::start_with_journal(
                    self.output_factory.clone(),
                    candidate.runtime.next_token()?,
                    capture.targets.clone(),
                    cursor,
                    Some(self.recovery_path()?),
                )?,
            )
        } else {
            None
        };
        if !visibility.is_null() {
            let owner = owner.ok_or("Visibility requires a journaled game owner.")?;
            (self.game_visibility)("start", &json!({"owner":owner,"prepared":visibility}))?;
            candidate.visibility = Some(crate::router::GameVisibility {
                owner: owner.into(),
                call: self.game_visibility.clone(),
            });
            let deadline = std::time::Instant::now() + Duration::from_secs(4);
            loop {
                cancelled()?;
                if let Some(output) = &mut output {
                    output.keep_alive()?;
                }
                let status = (self.game_visibility)("poll", &json!({"owner":owner}))?;
                if status["phase"] == "active" {
                    break;
                }
                if status["active"] != true || std::time::Instant::now() >= deadline {
                    return Err(format!(
                        "Visibility did not establish fresh capture: {}",
                        status["message"].as_str().unwrap_or("timed out")
                    ));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            drop(panels.take());
            drop(companions.take());
            (snapshot, panels, companions) = self.capture_sources(&capture)?;
        }
        cancelled()?;
        candidate.start_with_sources(
            capture.profile,
            capture.assignments,
            snapshot,
            workers,
            panels,
            companions,
        )?;
        let mut lease = self.game_lease.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(owner) = owner {
            if !lease
                .as_ref()
                .is_some_and(|l| l.owner == owner && !l.emergency && l.expected.is_none())
            {
                return Err("Game mapping ownership changed before arming.".into());
            }
        }
        let mut router = self.router.lock().unwrap_or_else(|p| p.into_inner());
        cancelled()?;
        if virtual_output {
            for (id, hash, generation) in &pinned_sources {
                if self.companion_provider()?.require_ready(id, hash, false)? != *generation {
                    return Err("Companion ownership changed during game acquisition; Start again explicitly.".into());
                }
            }
        }
        let token = router.adopt_checked(candidate, output, cancelled)?;
        if owner.is_some() {
            lease.as_mut().unwrap().expected = Some(token.clone());
        }
        Ok(token)
    }
    pub fn game_arm(&self, journal: &Value) -> Result<(), String> {
        let epoch = self.lifecycle.load(Ordering::Acquire);
        {
            let lease = self.game_lease.lock().unwrap_or_else(|p| p.into_inner());
            let lease = lease
                .as_ref()
                .filter(|l| journal["owner"] == l.owner)
                .ok_or("Game mapping ownership changed.")?;
            if lease.emergency {
                return Err("Mapping was stopped explicitly during game activation.".into());
            }
        }
        let capture = Capture::parse(&journal["desired"])?;
        let visibility = self.game_prepare(&journal["desired"], false)?;
        if visibility != journal["visibilityPrepared"] {
            return Err(
                "Captured visibility changed after preflight; preserve it and recapture.".into(),
            );
        }
        let live = capture.profile.requirements.virtual_devices;
        self.start_capture(capture, epoch, journal["owner"].as_str(), &visibility, live)?;
        Ok(())
    }
    pub fn game_stop(&self, journal: &Value) -> Result<(), String> {
        let lease = self.game_lease.lock().unwrap_or_else(|p| p.into_inner());
        let mut router = self.router.lock().unwrap_or_else(|p| p.into_inner());
        let current = router.runtime.token().ok();
        match lease.as_ref() {
            Some(lease)
                if journal["owner"] == lease.owner
                    && (current.is_none() || current == lease.expected) =>
            {
                router.stop_result()?
            }
            None if current.is_none() => {} // Restart recovery: never recreate a run from a journal.
            _ => {
                return Err(
                    "Mapping changed outside the game session. Recovery preserved it.".into(),
                )
            }
        }
        drop(router);
        drop(lease);
        let owner = journal["owner"]
            .as_str()
            .ok_or("Missing game mapping owner.")?;
        if journal["desired"]["profile"]["requirements"]["virtualDevices"] == true {
            let path = self.recovery_path()?;
            if let Some(record) =
                osg_virtual_controller::output_journal::inspect(&path)?.filter(|r| r.pending())
            {
                if record.token.session != owner {
                    return Err("Another output owner changed recovery state; preserve it.".into());
                }
                self.router
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .recover_durable(self.output_factory.clone(), path)?;
            }
        }
        if !journal["desired"]["visibility"].is_null() {
            let request = json!({"owner":owner});
            let mut status = (self.game_visibility)("stop", &request)?;
            let deadline = std::time::Instant::now() + Duration::from_secs(4);
            while status["active"] == true {
                if std::time::Instant::now() >= deadline {
                    return Err("Visibility restoration is still pending; retry explicit game recovery before restoring hardware.".into());
                }
                std::thread::sleep(Duration::from_millis(5));
                status = (self.game_visibility)("poll", &request)?;
            }
            (self.game_visibility)("stop", &request)?;
        }
        if serde_json::to_value(
            &self
                .settings
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .value,
        )
        .unwrap()
            != journal["settings"]
        {
            return Err(
                "Local mapping settings changed outside the game session. Recovery preserved them."
                    .into(),
            );
        }
        Ok(())
    }
    pub fn game_abandon(&self, journal: &Value) {
        let mut lease = self.game_lease.lock().unwrap_or_else(|p| p.into_inner());
        if lease.as_ref().is_some_and(|l| journal["owner"] == l.owner) {
            *lease = None;
        }
        if !journal["desired"]["visibility"].is_null() {
            let _ = (self.game_visibility)("abandon", &json!({"owner":journal["owner"]}));
        }
    }
    pub fn game_resume(&self, journal: &Value, recovery: bool) -> Result<(), String> {
        let epoch = self.lifecycle.load(Ordering::Acquire);
        let (owned, resume) = {
            let lease = self.game_lease.lock().unwrap_or_else(|p| p.into_inner());
            let owned = lease.as_ref().is_some_and(|l| journal["owner"] == l.owner);
            (
                owned,
                owned
                    && !recovery
                    && !lease.as_ref().unwrap().emergency
                    && !journal["baseline"].is_null(),
            )
        };
        if resume {
            let prior = Capture::parse_recording(&journal["baseline"])?;
            self.game_prepare(&journal["baseline"], true)?;
            self.start_capture(prior, epoch, None, &Value::Null, false)?;
        }
        if owned {
            *self.game_lease.lock().unwrap_or_else(|p| p.into_inner()) = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (Service, tempfile::TempDir, Value) {
        let (s, root, p, a, _) = super::super::tests::setup();
        s.request(&json!({"command":"mapping_assignments_save","profileId":p.id,"assignments":a}))
            .unwrap();
        let c = s.capture_saved(&p.id, None).unwrap();
        (s, root, c)
    }
    #[test]
    fn capture_is_immutable_and_resume_uses_a_fresh_run() {
        let (s, _root, c) = setup();
        let id = c["profile"]["id"].as_str().unwrap();
        s.request(&json!({"command":"mapping_preview_start","profileId":id}))
            .unwrap();
        let prior = s.request(&json!({"command":"mapping_status"})).unwrap()["token"].clone();
        let j = s.game_snapshot(&c).unwrap();
        s.game_take(&j).unwrap();
        assert!(s.game_take(&j).is_err());
        let mut edited = Capture::parse(&c).unwrap().profile;
        edited.name = "Later edit".into();
        s.store().unwrap().save(&edited).unwrap();
        s.game_arm(&j).unwrap();
        assert_eq!(
            s.router
                .lock()
                .unwrap()
                .runtime
                .engine()
                .unwrap()
                .profile()
                .name,
            c["profile"]["name"]
        );
        s.game_stop(&j).unwrap();
        s.game_resume(&j, false).unwrap();
        let now = s.request(&json!({"command":"mapping_status"})).unwrap();
        assert_eq!(now["running"], true);
        assert_ne!(now["token"], prior);
        assert_eq!(
            s.router
                .lock()
                .unwrap()
                .runtime
                .engine()
                .unwrap()
                .profile()
                .name,
            c["profile"]["name"]
        );
    }
    #[test]
    fn recovery_and_emergency_stop_never_rearm_a_prior_mapping() {
        for recovery in [true, false] {
            let (s, _root, c) = setup();
            s.request(&json!({"command":"mapping_preview_start","profileId":c["profile"]["id"]}))
                .unwrap();
            let j = s.game_snapshot(&c).unwrap();
            s.game_take(&j).unwrap();
            s.game_arm(&j).unwrap();
            if !recovery {
                s.request(&json!({"command":"mapping_stop"})).unwrap();
                assert!(s.game_arm(&j).is_err());
            }
            s.game_stop(&j).unwrap();
            s.game_resume(&j, recovery).unwrap();
            assert_eq!(
                s.request(&json!({"command":"mapping_status"})).unwrap()["running"],
                false
            );
        }
        let (s, _root, c) = setup();
        let j = s.game_snapshot(&c).unwrap();
        s.game_stop(&j).unwrap();
        s.game_resume(&j, true).unwrap();
        assert!(s.game_arm(&j).is_err());
    }
    #[test]
    fn external_runs_and_settings_are_preserved_with_conflicts() {
        let (s, _root, c) = setup();
        let j = s.game_snapshot(&c).unwrap();
        s.request(&json!({"command":"mapping_preview_start","profileId":c["profile"]["id"]}))
            .unwrap();
        assert!(s.game_take(&j).is_err());
        assert!(s.game_stop(&j).is_err());
        s.request(&json!({"command":"mapping_stop"})).unwrap();
        let j = s.game_snapshot(&c).unwrap();
        s.game_take(&j).unwrap();
        s.game_arm(&j).unwrap();
        s.settings.lock().unwrap().enable_virtual(true).unwrap();
        assert!(s.game_stop(&j).unwrap_err().contains("settings changed"));
        assert!(s.router.lock().unwrap().runtime.token().is_err());
        assert!(s.settings.lock().unwrap().value.virtual_devices);
    }
    #[test]
    fn independent_optional_requirements_fail_before_taking_ownership() {
        let (mut s, _root, c) = setup();
        // Exercise the unqualified fixture regardless of the host's supported
        // backend. Production Windows qualification remains unchanged.
        s.qualified = false;
        s.game_visibility = Arc::new(|operation, _| {
            assert_eq!(operation, "preflight");
            Err("Captured device visibility is not qualified in this fixture.".into())
        });
        let mut virtual_only = c.clone();
        virtual_only["profile"]["requirements"]["virtualDevices"] = json!(true);
        let bindings=c["profile"]["targets"].as_array().unwrap().iter().enumerate().map(|(i,t)|json!({"target":t["id"],"slot":i+1,"axes":if i==0{vec![48]}else{vec![]},"layout":t["layout"]})).collect::<Vec<_>>();
        virtual_only["targets"] = json!(bindings);
        assert!(s
            .game_preflight(&virtual_only)
            .unwrap_err()
            .contains("not qualified"));
        let mut hiding = c.clone();
        hiding["profile"]["requirements"]["deviceHiding"] = json!(true);
        assert!(Capture::parse(&hiding).is_err());
        hiding["visibility"] = json!({"mode":"linux-grab","sourceIds":["physical"]});
        assert!(s
            .game_preflight(&hiding)
            .unwrap_err()
            .contains("visibility is not qualified"));
        hiding["visibility"]["mode"] = json!("global");
        assert!(Capture::parse(&hiding).is_err());
        assert!(s.game_lease.lock().unwrap().is_none());
        assert!(s.game_preflight(&c).is_ok());
        assert!(Capture::parse(&json!({"unknown":true})).is_err());
        let mut excessive = c;
        excessive["extra"] = json!("x".repeat(65536));
        assert!(Capture::parse(&excessive).is_err());
    }
}
