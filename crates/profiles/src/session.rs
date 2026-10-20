//! Selection and immutable apply candidates. This module never starts an output.
use crate::{default_document, semantic_hash, ProfileDocument};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SelectionMode {
    Manual,
    Automatic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionState {
    Stopped,
    Faulted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotSummary {
    pub profile_id: String,
    pub revision: u64,
    pub semantic_hash: String,
}

/// A private deep copy, so saving or editing a draft cannot change this candidate.
/// Capturing a candidate does not execute it. P4's coordinator must separately
/// validate resources and acquire outputs before recording any running state.
#[derive(Clone, Debug)]
pub struct AppliedSnapshot {
    document: ProfileDocument,
    summary: SnapshotSummary,
}

impl AppliedSnapshot {
    pub fn capture(document: &ProfileDocument) -> Result<Self, String> {
        Ok(Self {
            document: document.clone(),
            summary: SnapshotSummary {
                profile_id: document.id.clone(),
                revision: document.revision,
                semantic_hash: semantic_hash(document)?,
            },
        })
    }

    pub fn document(&self) -> &ProfileDocument {
        &self.document
    }

    pub fn summary(&self) -> &SnapshotSummary {
        &self.summary
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatus {
    pub selected_profile_id: String,
    pub selection_mode: SelectionMode,
    pub state: SessionState,
    pub applied_snapshot: Option<SnapshotSummary>,
    pub fault: Option<String>,
}

pub struct Session {
    status: SessionStatus,
    snapshot: Option<AppliedSnapshot>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            status: SessionStatus {
                selected_profile_id: default_document().id,
                selection_mode: SelectionMode::Manual,
                state: SessionState::Stopped,
                applied_snapshot: None,
                fault: None,
            },
            snapshot: None,
        }
    }
}

impl Session {
    pub fn status(&self) -> &SessionStatus {
        &self.status
    }

    pub fn select(&mut self, id: &str, source: SelectionMode) -> bool {
        if source == SelectionMode::Automatic
            && self.status.selection_mode != SelectionMode::Automatic
        {
            return false;
        }
        if source == SelectionMode::Manual {
            self.status.selection_mode = SelectionMode::Manual;
        }
        self.status.selected_profile_id = id.to_owned();
        // Selection is metadata. In particular it cannot clear a fault.
        true
    }

    pub fn set_mode(&mut self, mode: SelectionMode) {
        self.status.selection_mode = mode;
    }

    pub fn fault(&mut self, message: String) {
        self.status.state = SessionState::Faulted;
        self.status.fault = Some(message);
    }

    /// Capture only; there is deliberately no hardware activation in P0.
    pub fn capture_selected(&mut self, document: &ProfileDocument) -> Result<(), String> {
        if self.status.state != SessionState::Stopped {
            return Err("Faulted sessions cannot capture a new apply candidate.".into());
        }
        if document.id != self.status.selected_profile_id {
            return Err("Only the selected profile may be captured.".into());
        }
        let snapshot = AppliedSnapshot::capture(document)?;
        self.status.applied_snapshot = Some(snapshot.summary().clone());
        self.snapshot = Some(snapshot);
        Ok(())
    }

    pub fn snapshot(&self) -> Option<&AppliedSnapshot> {
        self.snapshot.as_ref()
    }

    pub fn fallback_if_removed(&mut self, id: &str) {
        if self.status.selected_profile_id == id {
            self.status.selected_profile_id = default_document().id;
        }
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
