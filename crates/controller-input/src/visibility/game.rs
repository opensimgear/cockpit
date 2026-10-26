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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    handoff: Option<super::super::handoff::Proof>,
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
        let entries=osg_persistence::Evidence::siblings(&parent.join("inventory.json"),"recovery-visibility",1024)?;
        let mut owners=Vec::new();
        for name in entries {
            let Some(owner)=name.strip_prefix("game-visibility-").and_then(|s|s.strip_suffix(".json")) else{continue};
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
    fn handoff_metadata_snapshot(
        &self,
        owner: &str,
    ) -> Result<(journal::Record, super::super::runtime::Status), String> {
        if self.platform != Platform::Windows {
            return Err("Persistent visibility handoff is unavailable on this platform.".into());
        }
        if self
            .path
            .as_ref()
            .map(|path| journal::inspect(path))
            .transpose()?
            .flatten()
            .is_some_and(|record| !record.clean)
            || self.suppression.active()
            || self
                .launch
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .active()
        {
            return Err(
                "A foreign visibility scope must be restored before profile handoff.".into(),
            );
        }
        let job = self
            .game_owner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .filter(|active| active.owner == owner)
            .map(|active| active.job.clone())
            .ok_or("The outgoing profile does not own the active native hide session.")?;
        let pending = self.pending_games()?;
        if pending.len() != 1 || pending[0] != owner {
            return Err("Other pending visibility journals block handoff.".into());
        }
        let path = self.game_path(owner)?;
        let record =
            journal::inspect(&path)?.ok_or("The outgoing visibility journal is missing.")?;
        let status = self
            .worker
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .handoff_status()
            .ok()
            .filter(|status| status.job_id == job)
            .ok_or("The outgoing native visibility worker changed.")?;
        Ok((record, status))
    }
    fn handoff_prepared(&self, owner: &str, intent: IntentCapture) -> Result<Prepared, String> {
        let (record, status) = self.handoff_metadata_snapshot(owner)?;
        let preferences = intent.preferences(self.platform)?;
        let sources = self
            .registry
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .visibility_sources(&intent.source_ids)?;
        let capture = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .to_str()
            .ok_or("Invalid capture executable path.")?
            .to_owned();
        let ticket = self
            .worker
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .prepare_handoff(super::super::runtime::HandoffInput {
                job: status.job_id.clone(),
                record: record.clone(),
                preferences,
                sources,
                capture,
            })?;
        // No Service worker, mutation-admission, registry or status lock is held
        // while the retained adapter performs its bounded read-only work.
        let read = ticket.wait()?;
        let proof = super::super::handoff::Proof::capture(owner, &record, &status, &read.actual)?;
        self.handoff_validate_metadata(&proof)?;
        self.game_enabled()?;
        read.validate_live()?;
        Ok(Prepared {
            intent,
            policy: Some(read.policy),
            handoff: Some(proof),
        })
    }
    fn handoff_validate_metadata(
        &self,
        proof: &super::super::handoff::Proof,
    ) -> Result<(), String> {
        let (record, status) = self.handoff_metadata_snapshot(&proof.owner)?;
        proof.validate_metadata_active(&record, &status)?;
        if !self
            .game_owner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_some_and(|active| active.owner == proof.owner && active.job == proof.job_id)
        {
            return Err(
                "The outgoing native visibility owner changed during proof capture.".into(),
            );
        }
        Ok(())
    }
    fn handoff_restored(&self, proof: &super::super::handoff::Proof) -> Result<(), String> {
        if self.platform != Platform::Windows {
            return Err("Persistent visibility handoff is unavailable on this platform.".into());
        }
        let record = journal::inspect(&self.game_path(&proof.owner)?)?
            .ok_or("The outgoing visibility journal disappeared before incoming hiding.")?;
        let status = self
            .worker
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .snapshot()
            .ok_or("The outgoing native visibility restore proof is missing.")?;
        proof.validate_restored(&record, &status, &(self.factory)()?.read()?)
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
        // Read-only native proof I/O must not hold the mutation admission
        // mutex: the outgoing owner still needs responsive health/Stop.
        if operation == "handoff_preflight" {
            self.game_enabled()?;
            let owner = value["owner"]
                .as_str()
                .ok_or("Missing outgoing visibility owner.")?;
            let intent = IntentCapture::parse(&value["intent"])?;
            return Ok(json!(self.handoff_prepared(owner, intent)?));
        }
        if operation == "handoff_validate" {
            self.game_enabled()?;
            let proof: super::super::handoff::Proof =
                serde_json::from_value(value["proof"].clone()).map_err(|e| e.to_string())?;
            proof.validate()?;
            self.handoff_validate_metadata(&proof)?;
            return Ok(json!({"valid":true}));
        }
        let _admission = self.admission.lock().unwrap_or_else(|p| p.into_inner());
        if operation == "capture" {
            let mut intent = IntentCapture::parse(value)?;
            let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
            // Capture current explicit policies once; later editor changes do not alter the run.
            intent.policies=intent.source_ids.iter().map(|id|settings.value.devices.get(id).cloned().map(|policy|(id.clone(),policy)).ok_or("Save an explicit policy for every selected controller before capturing a game profile.")).collect::<Result<_,_>>()?;
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
                if journal::inspect(path)?.is_some_and(|record| !record.clean) {
                    return Err("Recover standalone visibility before game activation.".into());
                }
            }
            let intent = IntentCapture::parse(value)?;
            let policy = self.game_policy(&intent)?;
            return Ok(json!(Prepared {
                intent,
                policy,
                handoff: None
            }));
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
                if let Some(proof) = &prepared.handoff {
                    if proof.owner == owner {
                        return Err(
                            "An incoming hide session requires a distinct native owner.".into()
                        );
                    }
                    self.handoff_restored(proof)?;
                }
                let intent = IntentCapture::parse(&json!(prepared.intent))?;
                let policy = if self.platform == Platform::Windows {
                    // Native reads belong to the existing worker so its caller
                    // can keep neutral output leased and acknowledge Stop.
                    // Task::Apply reconstructs this exact plan from fresh
                    // identities/policy and compares it before any mutation.
                    let policy = prepared.policy.clone()
                        .ok_or("Windows visibility requires its captured native policy.")?;
                    policy.validate()?;
                    Some(policy)
                } else {
                    let policy = self.game_policy(&intent)?;
                    if policy != prepared.policy {
                        return Err("Visibility policy or controller instance changed after preflight; preserve the external state.".into());
                    }
                    policy
                };
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
