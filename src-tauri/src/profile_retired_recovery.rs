//! Cleanup-only access to retained pre-profile output ownership journals.
//! No obsolete mapping/feedback configuration, input capture or runtime is opened.
use osg_mapping::engine::RunToken;
use osg_virtual_controller::output_journal::{self, Phase, Record, Store};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};
const MAX_COMPANION_JOURNALS: usize = 32;
const QUALIFICATION: &str = osg_mapping_host::service::QUALIFICATION;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum JournalId {
    Manual,
    Companion { file: String },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct JournalRef {
    pub id: JournalId,
    pub owner: RunToken,
    pub record_hash: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProvisionRef {
    pub operation: String,
    pub record_hash: String,
    pub target: u8,
    pub refresh_uncertain: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OutputStatus {
    pub reference: JournalRef,
    pub phase: Phase,
    pub slots: Vec<u8>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProvisionJob {
    pub job_id: String,
    pub operation: String,
    pub active: bool,
    pub state: String,
    pub message: String,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    pub required: bool,
    pub outputs: Vec<OutputStatus>,
    pub provisioning: Vec<ProvisionRef>,
    pub job: Option<ProvisionJob>,
    pub diagnostics: Vec<String>,
    pub output_qualified: bool,
    pub provisioning_qualified: bool,
}
trait Operations: Send + Sync {
    fn output_qualified(&self) -> bool;
    fn provision_qualified(&self) -> bool;
    fn recover_output(&self, path: PathBuf, expected: &Record) -> Result<(), String>;
    fn provisioning(&self) -> Result<Vec<ProvisionRef>, String>;
    fn job(&self) -> Option<ProvisionJob>;
    fn recover_provision(&self, operation: &str) -> Result<ProvisionJob, String>;
    fn cancel_provision(&self, job: &str) -> Result<ProvisionJob, String>;
}
struct Native {
    enabled: bool,
    #[cfg(windows)]
    provisioning: Mutex<osg_virtual_controller::windows_jobs::Manager>,
}
impl Operations for Native {
    fn output_qualified(&self) -> bool {
        self.enabled && osg_mapping_host::service::LIVE_OUTPUT_QUALIFIED
    }
    fn provision_qualified(&self) -> bool {
        #[cfg(windows)]
        {
            self.output_qualified() && osg_virtual_controller::windows_launcher::QUALIFIED
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
    fn recover_output(&self, path: PathBuf, expected: &Record) -> Result<(), String> {
        if !self.output_qualified() {
            return Err(QUALIFICATION.into());
        }
        osg_virtual_controller::session::Session::recover_stored_exact(
            osg_virtual_controller::entry::factory(),
            path,
            expected,
        )
    }
    fn provisioning(&self) -> Result<Vec<ProvisionRef>, String> {
        if !self.enabled {
            return Ok(vec![]);
        }
        #[cfg(windows)]
        {
            use osg_virtual_controller::windows_provision::NativeBackend;
            NativeBackend::native("00000000-0000-0000-0000-000000000000")?
                .pending()?
                .into_iter()
                .map(|pending| {
                    let record = NativeBackend::native(&pending.operation)?.read_journal()?;
                    Ok(ProvisionRef {
                        operation: pending.operation,
                        record_hash: digest(&record)?,
                        target: pending.target,
                        refresh_uncertain: pending.refresh_uncertain,
                    })
                })
                .collect()
        }
        #[cfg(not(windows))]
        {
            Ok(vec![])
        }
    }
    fn job(&self) -> Option<ProvisionJob> {
        #[cfg(windows)]
        {
            self.provisioning
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .status()
                .map(provision_job)
        }
        #[cfg(not(windows))]
        {
            None
        }
    }
    fn recover_provision(&self, operation: &str) -> Result<ProvisionJob, String> {
        if !self.provision_qualified() {
            return Err(QUALIFICATION.into());
        }
        #[cfg(windows)]
        {
            self.provisioning
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .start(
                    osg_virtual_controller::windows_launcher::Action::Recover,
                    Some(operation.into()),
                )
                .map(provision_job)
        }
        #[cfg(not(windows))]
        {
            Err(QUALIFICATION.into())
        }
    }
    fn cancel_provision(&self, job: &str) -> Result<ProvisionJob, String> {
        #[cfg(windows)]
        {
            self.provisioning
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .cancel(job)
                .map(provision_job)
        }
        #[cfg(not(windows))]
        {
            Err("No native provisioning cleanup is active.".into())
        }
    }
}
#[cfg(windows)]
fn provision_job(job: osg_virtual_controller::windows_jobs::Status) -> ProvisionJob {
    ProvisionJob {
        job_id: job.job_id,
        operation: job.operation,
        active: job.active,
        state: job.state,
        message: job.message,
    }
}
fn digest(value: &impl Serialize) -> Result<String, String> {
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|e| e.to_string())?)
    ))
}
fn canonical(value: &str) -> bool {
    uuid::Uuid::parse_str(value)
        .ok()
        .map(|id| id.to_string())
        .as_deref()
        == Some(value)
}
fn hash(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|value| {
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn companion_file(value: &str) -> bool {
    value.strip_prefix("instance-").is_some_and(|hash| {
        hash.len() == 32
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn redirected(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        return metadata.file_attributes() & 0x400 != 0;
    }
    #[cfg(not(windows))]
    false
}
/// Every existing ancestor is checked, including a missing leaf's parents.
/// These retained paths are native-only; a junction must not redirect cleanup.
fn ordinary(path: &Path, directory: bool) -> Result<bool, String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(
            "Retained ownership requires an absolute native path without parent traversal.".into(),
        );
    }
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part.as_os_str());
        // A Windows disk prefix alone (including \\?\C:) is not a rooted
        // filesystem location. Inspect it only after the root component arrives.
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if redirected(&metadata) => return Err("Retained ownership cannot follow a symlink or reparse point; preserve it for inspection.".into()),
            Ok(metadata) if current != path && !metadata.is_dir() => return Err("A retained ownership parent must be an ordinary directory.".into()),
            Ok(metadata) if current == path && (if directory { !metadata.is_dir() } else { !metadata.is_file() }) => return Err("Retained ownership requires ordinary directories and regular journal/lock files.".into()),
            Ok(_) => {},
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(true)
}
fn directory(path: &Path) -> Result<bool, String> {
    ordinary(path, true)
}
pub(crate) struct Recovery {
    root: Option<PathBuf>,
    ops: Arc<dyn Operations>,
    operation: Mutex<()>,
}
impl Recovery {
    pub(crate) fn new(root: Option<PathBuf>, native: bool) -> Self {
        Self {
            root,
            ops: Arc::new(Native {
                enabled: native,
                #[cfg(windows)]
                provisioning: Mutex::new(Default::default()),
            }),
            operation: Mutex::new(()),
        }
    }
    fn root(&self) -> Result<Option<&Path>, String> {
        let Some(root) = self.root.as_deref() else {
            return Ok(None);
        };
        if !root.is_absolute() {
            return Err("Retained recovery requires its native absolute storage location.".into());
        }
        Ok(directory(root)?.then_some(root))
    }
    fn path(&self, id: &JournalId) -> Result<PathBuf, String> {
        let root = self
            .root()?
            .ok_or("The retained ownership journal no longer exists.")?;
        let path = match id {
            JournalId::Manual => root.join("output-recovery.json"),
            JournalId::Companion { file } if companion_file(file) => {
                let parent = root.join("companion-recovery");
                if !directory(&parent)? {
                    return Err("The retained companion ownership journal no longer exists.".into());
                }
                parent.join(format!("{file}.json"))
            }
            _ => {
                return Err(
                    "Choose an exact native retained journal identity; paths are never accepted."
                        .into(),
                )
            }
        };
        ordinary(&path, false)?;
        ordinary(&path.with_extension("lock"), false)?;
        Ok(path)
    }
    fn inspect(&self, id: JournalId) -> Result<Option<OutputStatus>, String> {
        let path = self.path(&id)?;
        output_journal::inspect(&path)?
            .filter(Record::pending)
            .map(|record| {
                Ok(OutputStatus {
                    reference: JournalRef {
                        id,
                        owner: record.token.clone(),
                        record_hash: digest(&record)?,
                    },
                    phase: record.phase,
                    slots: record.bindings.iter().map(|binding| binding.slot).collect(),
                })
            })
            .transpose()
    }
    pub(crate) fn status(&self) -> Status {
        let mut status = Status {
            output_qualified: self.ops.output_qualified(),
            provisioning_qualified: self.ops.provision_qualified(),
            job: self.ops.job(),
            ..Default::default()
        };
        let scanned = (|| -> Result<(), String> {
            let Some(root) = self.root()? else {
                return Ok(());
            };
            if let Some(output) = self.inspect(JournalId::Manual)? {
                status.outputs.push(output);
            }
            let parent = root.join("companion-recovery");
            if !directory(&parent)? {
                return Ok(());
            }
            let mut count = 0;
            for entry in std::fs::read_dir(parent).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let name = entry.file_name();
                let Some(name) = name.to_str() else {
                    return Err("Retained recovery has an unreadable filename.".into());
                };
                if !name.ends_with(".json") {
                    continue;
                }
                count += 1;
                if count > MAX_COMPANION_JOURNALS {
                    return Err("More than 32 retained companion ownership journals require native inspection.".into());
                }
                let file = name
                    .strip_suffix(".json")
                    .filter(|file| companion_file(file))
                    .ok_or(
                        "An unknown companion recovery journal must be preserved for inspection.",
                    )?;
                if let Some(output) = self.inspect(JournalId::Companion { file: file.into() })? {
                    status.outputs.push(output);
                }
            }
            Ok(())
        })();
        if let Err(error) = scanned {
            status.diagnostics.push(error.chars().take(512).collect());
        }
        match self.ops.provisioning() {
            Ok(records) if records.len() <= 16
                && records.iter().all(|record| canonical(&record.operation)
                    && hash(&record.record_hash) && (1..=16).contains(&record.target))
                && records.iter().map(|record| &record.operation).collect::<std::collections::BTreeSet<_>>().len() == records.len()
                => status.provisioning = records,
            Ok(_)=>status.diagnostics.push("Invalid retained provisioning ownership inventory; preserve it for native inspection.".into()),
            Err(error)=>status.diagnostics.push(error.chars().take(512).collect()),
        }
        status
            .outputs
            .sort_by_key(|output| serde_json::to_string(&output.reference.id).unwrap_or_default());
        status.required = !status.outputs.is_empty()
            || !status.provisioning.is_empty()
            || !status.diagnostics.is_empty()
            || status.job.as_ref().is_some_and(|job| job.active);
        status
    }
    pub(crate) fn guard_clear(&self) -> Result<(), String> {
        if self.status().required {
            Err("Finish retained output/provision ownership cleanup before acquiring new profile output or learning input. Saved obsolete configuration is not needed for that cleanup.".into())
        } else {
            Ok(())
        }
    }
    pub(crate) fn request(&self, request: &serde_json::Value) -> Result<serde_json::Value, String> {
        let command = request["command"]
            .as_str()
            .ok_or("Choose an explicit retained cleanup operation.")?;
        let fields: &[&str] = match command {
            "profile_retired_recovery_status" => &[],
            "profile_retired_recovery_recover"
            | "profile_retired_recovery_acknowledge"
            | "profile_retired_recovery_provision_recover" => &["reference"],
            "profile_retired_recovery_provision_cancel" => &["jobId"],
            _ => return Err(
                "Unknown retained cleanup operation; retired authoring and starts are unavailable."
                    .into(),
            ),
        };
        if request
            .as_object()
            .ok_or("Retained cleanup requires an object.")?
            .keys()
            .any(|key| key != "command" && !fields.contains(&key.as_str()))
        {
            return Err("Unknown field in retained cleanup request.".into());
        }
        match command {
            "profile_retired_recovery_status" => {}
            "profile_retired_recovery_recover" | "profile_retired_recovery_acknowledge" => {
                let reference: JournalRef = serde_json::from_value(request["reference"].clone())
                    .map_err(|_| "Choose the exact inspected native retained output reference.")?;
                if command == "profile_retired_recovery_recover" {
                    self.recover(&reference)?;
                } else {
                    self.acknowledge(&reference)?;
                }
            }
            "profile_retired_recovery_provision_recover" => {
                let reference: ProvisionRef = serde_json::from_value(request["reference"].clone())
                    .map_err(|_| "Choose the exact inspected native provisioning reference.")?;
                self.recover_provision(&reference)?;
            }
            _ => {
                self.cancel_provision(
                    request["jobId"]
                        .as_str()
                        .ok_or("Choose the exact active native cleanup job.")?,
                )?;
            }
        }
        serde_json::to_value(self.status()).map_err(|e| e.to_string())
    }
    pub(crate) fn shutdown(&self) {
        if let Some(job) = self.ops.job().filter(|job| job.active) {
            let _ = self.ops.cancel_provision(&job.job_id);
        }
    }
    fn matches(reference: &JournalRef, record: &Record) -> Result<(), String> {
        if !canonical(&reference.owner.session)
            || reference.owner.generation.0 == 0
            || record.token != reference.owner
            || digest(record)? != reference.record_hash
        {
            return Err("The retained output owner or complete journal changed; inspect its exact current reference before cleanup.".into());
        }
        Ok(())
    }
    pub(crate) fn recover(&self, reference: &JournalRef) -> Result<(), String> {
        let _operation = self.operation.lock().unwrap_or_else(|p| p.into_inner());
        if !self.ops.output_qualified() {
            return Err(QUALIFICATION.into());
        }
        let path = self.path(&reference.id)?;
        let record =
            output_journal::inspect(&path)?.ok_or("The retained ownership journal disappeared.")?;
        Self::matches(reference, &record)?;
        if record.phase != Phase::Confirmed {
            return Err("Only confirmed retained ownership can perform cleanup; ambiguous gaps require explicit exact-owner acknowledgement without output.".into());
        }
        self.ops.recover_output(path.clone(), &record)?;
        let after =
            output_journal::inspect(&path)?.ok_or("The retained cleanup record disappeared.")?;
        if after.pending() || after.token != reference.owner {
            return Err("Retained output cleanup did not complete for this exact owner; preserve the journal.".into());
        }
        Ok(())
    }
    /// Read-only native capture for a retained game journal's exact live owner.
    /// The old baseline token is deliberately not accepted as cleanup authority.
    pub(crate) fn capture_mapping_owner(&self, owner: &str) -> Result<Option<JournalRef>, String> {
        if !canonical(owner) {
            return Err("Retained game cleanup requires its exact canonical output owner.".into());
        }
        if self.root()?.is_none() {
            return Ok(None);
        }
        let Some(output) = self.inspect(JournalId::Manual)? else {
            return Ok(None);
        };
        if output.reference.owner.session != owner {
            return Err(
                "Another output owner changed retained game recovery state; preserving it.".into(),
            );
        }
        Ok(Some(output.reference))
    }
    pub(crate) fn recover_mapping_owner(
        &self,
        owner: &str,
        expected: &Option<JournalRef>,
    ) -> Result<(), String> {
        if !canonical(owner) {
            return Err("Retained game cleanup requires its exact canonical output owner.".into());
        }
        match expected {
            Some(reference) if reference.id == JournalId::Manual && reference.owner.session == owner => self.recover(reference),
            Some(_) => Err("The retained game cleanup reference belongs to another owner or journal; preserving it.".into()),
            None if self.capture_mapping_owner(owner)?.is_none() => Ok(()),
            None => Err("Output ownership appeared after the retained game cleanup capture; inspect its exact current reference.".into()),
        }
    }
    pub(crate) fn acknowledge(&self, reference: &JournalRef) -> Result<(), String> {
        let _operation = self.operation.lock().unwrap_or_else(|p| p.into_inner());
        if !self.ops.output_qualified() {
            return Err(QUALIFICATION.into());
        }
        let mut store = Store::open(self.path(&reference.id)?)?;
        let record = store
            .record()
            .ok_or("The retained ownership journal disappeared.")?;
        Self::matches(reference, record)?;
        store.acknowledge(&reference.owner)
    }
    pub(crate) fn recover_provision(
        &self,
        reference: &ProvisionRef,
    ) -> Result<ProvisionJob, String> {
        let _operation = self.operation.lock().unwrap_or_else(|p| p.into_inner());
        if !self.ops.provision_qualified() {
            return Err(QUALIFICATION.into());
        }
        if !canonical(&reference.operation) {
            return Err("Choose an exact recorded native provisioning operation.".into());
        }
        let current = self
            .ops
            .provisioning()?
            .into_iter()
            .find(|current| current.operation == reference.operation)
            .ok_or("The recorded provisioning operation no longer requires cleanup.")?;
        if serde_json::to_value(&current).map_err(|e| e.to_string())?
            != serde_json::to_value(reference).map_err(|e| e.to_string())?
        {
            return Err(
                "The recorded provisioning cleanup changed; inspect its exact current reference."
                    .into(),
            );
        }
        self.ops.recover_provision(&reference.operation)
    }
    pub(crate) fn cancel_provision(&self, job_id: &str) -> Result<ProvisionJob, String> {
        if !canonical(job_id) {
            return Err("Choose the exact active native cleanup job.".into());
        }
        self.ops.cancel_provision(job_id)
    }
}
#[cfg(test)]
#[path = "profile_retired_recovery_tests.rs"]
mod tests;
