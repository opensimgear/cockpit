//! Machine-local vJoy preferences never enter a portable profile document.
use super::{check_revision, expected_revision, required_text, Service};
use crate::{store::LocalAssignment, ProfileDocument};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn validate(document: &ProfileDocument, slots: &BTreeMap<String, u8>) -> Result<(), String> {
    for (node, slot) in slots {
        if !(1..=16).contains(slot) {
            return Err("A local vJoy slot must be an integer from 1 through 16.".into());
        }
        if !document
            .virtual_outputs
            .iter()
            .any(|output| &output.id == node)
        {
            return Err(format!(
                "Local vJoy slot {slot} references missing virtual target {node}."
            ));
        }
    }
    Ok(())
}

impl Service {
    /// Capture pins under the same library lock as the saved portable graph.
    pub fn compile_virtual_source(
        &self,
        id: &str,
    ) -> Result<
        (
            ProfileDocument,
            Vec<LocalAssignment>,
            bool,
            BTreeMap<String, u8>,
        ),
        String,
    > {
        let (document, assignments, read_only) = self.compile_source(id)?;
        Ok((
            document,
            assignments,
            read_only,
            self.library
                .local_virtual_slots
                .get(id)
                .cloned()
                .unwrap_or_default(),
        ))
    }

    pub(super) fn virtual_slots_response(&self, id: &str) -> Result<Value, String> {
        let document = self.document(id)?;
        let pins = self.library.local_virtual_slots.get(id);
        let slots = document
            .virtual_outputs
            .iter()
            .map(|output| {
                json!({
                    "nodeId":output.id,"slot":pins.and_then(|pins|pins.get(&output.id))
                })
            })
            .collect::<Vec<_>>();
        Ok(
            json!({"profileId":id,"profileRevision":document.revision,"readOnly":self.read_only,"virtualSlots":slots,"virtualOutputIds":document.virtual_outputs.iter().map(|output| &output.id).collect::<Vec<_>>()}),
        )
    }

    pub(super) fn set_virtual_slot(&mut self, request: &Value) -> Result<Value, String> {
        if self.read_only {
            return Err("Repair profile storage before changing local vJoy slots.".into());
        }
        let id = required_text(request, "id")?;
        let node = required_text(request, "nodeId")?;
        let document = self.document(id)?;
        check_revision(document, expected_revision(request)?)?;
        if !document
            .virtual_outputs
            .iter()
            .any(|output| output.id == node)
        {
            return Err(format!(
                "Save virtual target {node} before choosing its local vJoy slot."
            ));
        }
        let slot = match request.get("slot") {
            Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_u64()
                    .filter(|slot| (1..=16).contains(slot))
                    .ok_or("A local vJoy slot must be null or an integer from 1 through 16.")?
                    as u8,
            ),
            None => {
                return Err("An explicit slot or null for automatic selection is required.".into())
            }
        };
        let mut library = self.library.clone();
        let pins = library
            .local_virtual_slots
            .entry(id.to_owned())
            .or_default();
        if let Some(slot) = slot {
            pins.insert(node.to_owned(), slot);
        } else {
            pins.remove(node);
        }
        library.prune_virtual_slots();
        self.persist(&library)?;
        self.library = library;
        self.virtual_slots_response(id)
    }
}

#[cfg(test)]
#[path = "virtual_slots_tests.rs"]
mod tests;
