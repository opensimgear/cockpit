//! Captured game visibility never replaces the standalone user's saved policies.
use super::super::{
    capabilities::Platform,
    policy::{Plan, Policy, Selection},
    settings::Preferences,
};
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IntentCapture {
    mode: String,
    source_ids: Vec<String>,
    #[serde(default)]
    policies: BTreeMap<String, Intent>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Prepared {
    intent: IntentCapture,
    policy: Option<Plan>,
}
pub(super) struct Owner {
    owner: String,
    job: String,
}
impl IntentCapture {
    fn parse(value: &Value) -> Result<Self, String> {
        if value.to_string().len() > 16384 {
            return Err("Captured visibility is oversized.".into());
        }
        let intent: Self = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        if intent.source_ids.is_empty()
            || intent.source_ids.len() > 16
            || intent.source_ids.iter().collect::<BTreeSet<_>>().len() != intent.source_ids.len()
            || intent
                .source_ids
                .iter()
                .any(|id| uuid::Uuid::parse_str(id).is_err())
        {
            return Err("Choose distinct physical controller identities for visibility.".into());
        }
        Ok(intent)
    }
    fn preferences(&self, platform: Platform) -> Result<Preferences, String> {
        let expected = match platform {
            Platform::Windows => "hidhide-session",
            Platform::Linux => "linux-grab",
            Platform::Macos => "macos-seize",
            Platform::Unsupported => "unsupported",
        };
        if self.mode != expected {
            return Err("This visibility scope is unavailable in this game coordinator. Linux isolated launch uses its separate owned launcher; no broader scope is substituted.".into());
        }
        if self.policies.len() != self.source_ids.len()
            || self
                .source_ids
                .iter()
                .any(|id| !self.policies.contains_key(id))
        {
            return Err("Recapture explicit policies for every selected controller.".into());
        }
        for policy in self.policies.values() {
            policy.validate()?;
            match (platform, policy) {
                (Platform::Windows, Intent::HideAll | Intent::HideFromProcess { .. }) => {}
                (Platform::Linux | Platform::Macos, Intent::BlockOriginalInput) => {}
                _ => {
                    return Err(
                        "Captured policy does not match the requested visibility scope.".into(),
                    )
                }
            }
        }
        let result = Preferences {
            schema: 1,
            enabled: true,
            devices: self.policies.clone(),
        };
        result.validate()?;
        Ok(result)
    }
}
impl Service {
    pub(super) fn pending_games(&self) -> Result<Vec<String>, String> {
        let Some(parent) = self.path.as_ref().and_then(|p| p.parent()) else {
            return Ok(vec![]);
        };
        let entries = match std::fs::read_dir(parent) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e.to_string()),
        };
        let mut owners = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name();
            let Some(owner) = name
                .to_str()
                .and_then(|s| s.strip_prefix("game-visibility-"))
                .and_then(|s| s.strip_suffix(".json"))
            else {
                continue;
            };
            let path = self.game_path(owner)?;
            if journal::inspect(&path)?.is_some_and(|r| !r.clean) {
                owners.push(owner.to_owned());
            }
            if owners.len() > 16 {
                return Err(
                    "Too many pending game visibility journals; inspect recovery storage.".into(),
                );
            }
        }
        Ok(owners)
    }
    fn game_path(&self, owner: &str) -> Result<PathBuf, String> {
        if uuid::Uuid::parse_str(owner)
            .ok()
            .map(|id| id.to_string())
            .as_deref()
            != Some(owner)
        {
            return Err("Invalid game visibility owner.".into());
        }
        Ok(self
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .ok_or("Durable visibility recovery storage is required.")?
            .join(format!("game-visibility-{owner}.json")))
    }
    fn game_enabled(&self) -> Result<(), String> {
        if !self.qualified {
            return Err("Captured device visibility is not qualified on this platform; no hiding policy was changed.".into());
        }
        let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        if settings.error.is_some() || !settings.value.enabled {
            return Err("Enable optional visibility explicitly and repair any settings error before game activation.".into());
        }
        Ok(())
    }
    fn game_policy(&self, intent: &IntentCapture) -> Result<Option<Plan>, String> {
        let preferences = intent.preferences(self.platform)?;
        if self.platform != Platform::Windows {
            let backend = if self.platform == Platform::Linux {
                "linux-evdev"
            } else {
                "macos-iohid"
            };
            self.registry
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .physical_sources(&intent.source_ids, backend)?;
            return Ok(None);
        }
        let sources = self
            .registry
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .visibility_sources(&intent.source_ids)?;
        let mut adapter = (self.factory)()?;
        let image = adapter.executable(
            std::env::current_exe()
                .map_err(|e| e.to_string())?
                .to_str()
                .ok_or("Invalid capture executable path.")?,
        )?;
        let mut selections = Vec::new();
        for source in sources {
            let policy = match &preferences.devices[&source.id] {
                Intent::HideAll => Policy::HideAll,
                Intent::HideFromProcess { executable } => Policy::HideFromProcess {
                    executable: adapter.executable(executable)?,
                },
                _ => return Err("Unsupported captured policy.".into()),
            };
            selections.push(Selection {
                instance: adapter.instance(&source.interface)?,
                policy,
            });
        }
        Ok(Some(Plan::build(adapter.read()?, selections, image)?))
    }
    fn game_status(&self) -> Value {
        if self.platform == Platform::Windows {
            self.worker
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .snapshot()
                .map(|s| json!(s))
                .unwrap_or(json!({"active":false,"phase":"restored"}))
        } else {
            self.suppression.snapshot()
        }
    }
    pub(crate) fn game_request(&self, operation: &str, value: &Value) -> Result<Value, String> {
        let _admission = self.admission.lock().unwrap_or_else(|p| p.into_inner());
        if operation == "capture" {
            let mut intent = IntentCapture::parse(value)?;
            let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
            // Capture current explicit policies once; later editor changes do not alter the run.
            intent.policies=intent.source_ids.iter().map(|id| {
                settings.value.devices.get(id).cloned().map(|p|(id.clone(),p)).ok_or("Save an explicit policy for every selected controller before capturing a game profile.")
            }).collect::<Result<_,_>>()?;
            intent.preferences(self.platform)?;
            return Ok(json!(intent));
        }
        if operation == "preflight" {
            self.game_enabled()?;
            if self.active() {
                return Err(
                    "Restore the existing visibility session before game preflight.".into(),
                );
            }
            if let Some(path) = &self.path {
                if journal::inspect(path)?.is_some_and(|r| !r.clean) {
                    return Err("Recover standalone visibility before game activation.".into());
                }
            }
            let intent = IntentCapture::parse(value)?;
            let policy = self.game_policy(&intent)?;
            return Ok(json!(Prepared { intent, policy }));
        }
        let owner = value["owner"]
            .as_str()
            .ok_or("Missing game visibility owner.")?;
        let path = self.game_path(owner)?;
        match operation {
            "start" => {
                self.game_enabled()?;
                if self.active() {
                    return Err("Another session owns visibility.".into());
                }
                let prepared: Prepared =
                    serde_json::from_value(value["prepared"].clone()).map_err(|e| e.to_string())?;
                let intent = IntentCapture::parse(&json!(prepared.intent))?;
                let policy = self.game_policy(&intent)?;
                if policy != prepared.policy {
                    return Err("Visibility policy or controller instance changed after preflight; preserve the external state.".into());
                }
                let job = if self.platform == Platform::Windows {
                    let expected = self.observer()(&intent.source_ids)?;
                    self.worker
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .start(
                            Task::Apply {
                                expected_plan: policy,
                                preferences: intent.preferences(self.platform)?,
                                ids: intent.source_ids,
                                capture: std::env::current_exe()
                                    .map_err(|e| e.to_string())?
                                    .to_str()
                                    .ok_or("Invalid capture path.")?
                                    .into(),
                                expected,
                            },
                            path,
                            self.factory.clone(),
                            self.observer(),
                        )?
                        .job_id
                } else {
                    let backend = if self.platform == Platform::Linux {
                        "linux-evdev"
                    } else {
                        "macos-iohid"
                    };
                    let sources = self
                        .registry
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .physical_sources(&intent.source_ids, backend)?;
                    self.suppression.apply(sources)?
                };
                *self.game_owner.lock().unwrap_or_else(|p| p.into_inner()) = Some(Owner {
                    owner: owner.into(),
                    job,
                });
                Ok(self.game_status())
            }
            "poll" => {
                if !self
                    .game_owner
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .as_ref()
                    .is_some_and(|s| s.owner == owner)
                {
                    return Err("Game visibility ownership changed.".into());
                }
                Ok(self.game_status())
            }
            "stop" => {
                let mut owned = self.game_owner.lock().unwrap_or_else(|p| p.into_inner());
                if owned.as_ref().is_some_and(|s| s.owner != owner) {
                    return Err("Another session owns visibility; preserve it.".into());
                }
                if let Some(owned) = owned.as_ref() {
                    if self.platform == Platform::Windows {
                        self.worker
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .stop(Some(&owned.job))?;
                    } else {
                        self.suppression.stop(Some(&owned.job))?;
                    }
                } else if self.platform == Platform::Windows {
                    let mut worker = self.worker.lock().unwrap_or_else(|p| p.into_inner());
                    if worker.active() {
                        return Err("Standalone visibility is active; preserve it.".into());
                    }
                    if let Some(record) = journal::inspect(&path)?.filter(|r| !r.clean) {
                        if !self.qualified {
                            return Err("Owned visibility recovery remains unqualified; preserve its journal for inspection.".into());
                        }
                        let status = worker.start(
                            Task::Recover { id: record.id },
                            path.clone(),
                            self.factory.clone(),
                            self.observer(),
                        )?;
                        *owned = Some(Owner {
                            owner: owner.into(),
                            job: status.job_id,
                        });
                    }
                } else if self.suppression.active() {
                    return Err("A different suppression session is active; preserve it.".into());
                }
                let status = self.game_status();
                if status["active"] != true {
                    if self.platform == Platform::Windows
                        && journal::inspect(&path)?.is_some_and(|r| !r.clean)
                    {
                        return Err(format!(
                            "Owned visibility recovery needs attention: {}",
                            status["message"].as_str().unwrap_or("policy conflict")
                        ));
                    }
                    if self.platform != Platform::Windows
                        && matches!(status["phase"].as_str(), Some("attention" | "fault"))
                    {
                        return Err(format!(
                            "Suppression restoration needs attention: {}",
                            status["message"]
                        ));
                    }
                    *owned = None;
                    return Ok(json!({"active":false,"phase":"restored"}));
                }
                Ok(status)
            }
            "abandon" => {
                let mut owned = self.game_owner.lock().unwrap_or_else(|p| p.into_inner());
                if owned.as_ref().is_some_and(|s| s.owner == owner) {
                    *owned = None;
                }
                Ok(json!({}))
            }
            _ => Err("Unknown internal game visibility operation.".into()),
        }
    }
}
