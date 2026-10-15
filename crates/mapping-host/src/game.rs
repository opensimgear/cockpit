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
        if value.to_string().len() > 65536 {
            return Err("Captured mapping exceeds 64 KiB.".into());
        }
        let captured: Self = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        captured.profile.compile()?;
        validate_assignments(&captured.profile, &captured.assignments)?;
        if captured.profile.requirements.virtual_devices || !captured.targets.is_empty() {
            crate::settings::validate_bindings(&captured.profile, &captured.targets)?;
        }
        if captured.profile.requirements.device_hiding != captured.visibility.is_some() {
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
        let capture = Capture::parse(desired)?;
        if self.capture.visibility_active() || self.provisioning_active() {
            return Err(
                "Stop standalone visibility or provisioning before game activation.".into(),
            );
        }
        if capture.profile.requirements.virtual_devices {
            return Err(format!(
                "Captured virtual output is not qualified: {QUALIFICATION}"
            ));
        }
        if capture.profile.requirements.device_hiding {
            return Err("Captured device visibility is not qualified on this platform; no hiding policy was changed.".into());
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
            osg_logic::worker::Worker::spawn(
                &std::env::current_exe().map_err(|e| e.to_string())?,
                &script.source,
            )?;
        }
        Ok(())
    }
    pub fn game_snapshot(&self, desired: &Value) -> Result<Value, String> {
        self.game_preflight(desired)?;
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
            json!({"owner":uuid::Uuid::new_v4().to_string(),"desired":desired,"baseline":self.active_capture(&router)?,"baselineToken":router.runtime.token().ok(),"settings":self.settings.lock().unwrap_or_else(|p|p.into_inner()).value,"taken":false,"armed":false,"stopped":false,"resumed":false}),
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
    fn start_capture(&self, capture: Capture, epoch: u64) -> Result<RunToken, String> {
        let _compile = self
            .compilation
            .try_lock()
            .map_err(|_| "A script compilation is already running.")?;
        let mut workers = std::collections::BTreeMap::new();
        for script in &capture.profile.scripts {
            workers.insert(
                script.id.clone(),
                osg_logic::worker::Worker::spawn(
                    &std::env::current_exe().map_err(|e| e.to_string())?,
                    &script.source,
                )?,
            );
        }
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
        let mut router = self.router.lock().unwrap_or_else(|p| p.into_inner());
        if self.lifecycle.load(Ordering::Acquire) != epoch || self.stop.load(Ordering::Acquire) {
            return Err("Game mapping start cancelled by Stop.".into());
        }
        router.start_with_panels(
            capture.profile,
            capture.assignments,
            snapshot,
            workers,
            panels,
        )
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
        self.game_preflight(&journal["desired"])?;
        let token = self.start_capture(capture, epoch)?;
        let mut lease = self.game_lease.lock().unwrap_or_else(|p| p.into_inner());
        let lease = lease
            .as_mut()
            .filter(|l| journal["owner"] == l.owner)
            .ok_or("Game mapping ownership changed.")?;
        lease.expected = Some(token);
        if lease.emergency || self.lifecycle.load(Ordering::Acquire) != epoch {
            return Err("Mapping was stopped during game activation.".into());
        }
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
            let prior = Capture::parse(&journal["baseline"])?;
            self.game_preflight(&journal["baseline"])?;
            self.start_capture(prior, epoch)?;
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
        let (s, _root, c) = setup();
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
