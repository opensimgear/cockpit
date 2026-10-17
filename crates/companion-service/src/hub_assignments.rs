//! Atomic assignment authority for captured game sessions. No output is started here.
use super::*;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Row {
    instance_id: String,
    companion_id: Option<String>,
    publication_hash: Option<String>,
    capability_fingerprint: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Snapshot {
    schema_version: u8,
    assignments: Vec<Row>,
}
fn parse(value: &Value) -> Result<BTreeMap<String, Row>, String> {
    exact(value, &["schemaVersion", "assignments"], &[])?;
    let array = value["assignments"]
        .as_array()
        .ok_or("Invalid assignment snapshot.")?;
    for row in array {
        exact(
            row,
            &[
                "instanceId",
                "companionId",
                "publicationHash",
                "capabilityFingerprint",
            ],
            &[],
        )?;
    }
    let snapshot: Snapshot = serde_json::from_value(value.clone())
        .map_err(|_| "Invalid captured assignment snapshot.")?;
    ensure(
        snapshot.schema_version == 1
            && !snapshot.assignments.is_empty()
            && snapshot.assignments.len() <= 8,
        "Captured assignment budget/version mismatch.",
    )?;
    let mut rows = BTreeMap::new();
    for row in snapshot.assignments {
        ensure(id(&row.instance_id), "Invalid captured instance ID.")?;
        match (
            &row.companion_id,
            &row.publication_hash,
            &row.capability_fingerprint,
        ) {
            (None, None, None) => {}
            (Some(companion), Some(publication), Some(capability)) => ensure(
                id(companion) && digest_id(publication) && digest_id(capability),
                "Invalid captured assignment identity/revision.",
            )?,
            _ => {
                return Err(
                    "Missing assignment must use an explicit null identity/revision trio.".into(),
                )
            }
        }
        ensure(
            rows.insert(row.instance_id.clone(), row).is_none(),
            "Duplicate captured controller instance.",
        )?;
    }
    Ok(rows)
}
fn snapshot(
    inner: &Inner,
    ids: impl Iterator<Item = String>,
) -> Result<BTreeMap<String, Row>, String> {
    let mut rows = BTreeMap::new();
    for instance in ids {
        ensure(id(&instance), "Invalid captured controller instance.")?;
        let row = if let Some(a) = inner
            .record
            .instances
            .iter()
            .find(|a| a.instance_id == instance)
        {
            let publication = inner.publications.get(&a.publication_hash).ok_or(
                "Republish the assigned immutable revision before capturing/restoring it.",
            )?;
            Row {
                instance_id: instance.clone(),
                companion_id: Some(a.companion_id.clone()),
                publication_hash: Some(a.publication_hash.clone()),
                capability_fingerprint: Some(publication.capability_fingerprint.clone()),
            }
        } else {
            Row {
                instance_id: instance.clone(),
                companion_id: None,
                publication_hash: None,
                capability_fingerprint: None,
            }
        };
        ensure(
            rows.insert(instance, row).is_none(),
            "Duplicate captured controller instance.",
        )?;
    }
    ensure(
        !rows.is_empty() && rows.len() <= 8,
        "Capture one to eight controller instances.",
    )?;
    Ok(rows)
}
fn json_snapshot(rows: &BTreeMap<String, Row>) -> Value {
    json!({"schemaVersion":1,"assignments":rows.values().collect::<Vec<_>>()})
}
fn plan(inner: &Inner, desired: &BTreeMap<String, Row>) -> Result<Record, String> {
    let mut record = inner.record.clone();
    record
        .instances
        .retain(|a| !desired.contains_key(&a.instance_id));
    for row in desired.values() {
        if let (Some(companion), Some(hash), Some(capability)) = (
            &row.companion_id,
            &row.publication_hash,
            &row.capability_fingerprint,
        ) {
            ensure(
                record
                    .credentials
                    .iter()
                    .any(|c| &c.companion_id == companion),
                "Captured companion credential was revoked.",
            )?;
            let p = inner.publications.get(hash).ok_or(
                "Captured immutable publication is unavailable; republish it before activation.",
            )?;
            ensure(
                &p.capability_fingerprint == capability,
                "Captured companion capability fingerprint differs.",
            )?;
            record.instances.push(Assignment {
                instance_id: row.instance_id.clone(),
                companion_id: companion.clone(),
                publication_hash: hash.clone(),
                definition: p.definition(&row.instance_id),
                ownership: None,
            });
        }
    }
    ensure(
        record.instances.len() <= MAX_INSTANCES,
        "Companion instance budget exhausted.",
    )?;
    Ok(record)
}
impl Hub {
    pub fn preflight_assignments(&self, desired: &Value) -> Result<(), String> {
        ensure(
            serde_json::to_vec(desired)
                .map_err(|e| e.to_string())?
                .len()
                <= 8192,
            "Captured assignment snapshot exceeds its budget.",
        )?;
        let desired = parse(desired)?;
        let inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        plan(&inner, &desired).map(|_| ())
    }
    pub fn assignment_snapshot(&self, instance_ids: &[String]) -> Result<Value, String> {
        ensure(
            !instance_ids.is_empty() && instance_ids.len() <= 8,
            "Capture one to eight controller instances.",
        )?;
        let inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        snapshot(&inner, instance_ids.iter().cloned()).map(|rows| json_snapshot(&rows))
    }
    pub fn compare_assignments(&self, expected: &Value, desired: &Value) -> Result<Value, String> {
        ensure(
            serde_json::to_vec(expected)
                .map_err(|e| e.to_string())?
                .len()
                <= 8192
                && serde_json::to_vec(desired)
                    .map_err(|e| e.to_string())?
                    .len()
                    <= 8192,
            "Captured assignment snapshot exceeds its budget.",
        )?;
        let expected = parse(expected)?;
        let desired = parse(desired)?;
        ensure(
            expected.keys().eq(desired.keys()),
            "Expected/desired assignment scopes must match, including missing instances.",
        )?;
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        ensure(!inner.record.instances.iter().any(|assignment|
            expected.contains_key(&assignment.instance_id) && assignment.ownership.is_some()),
            "A unified profile owns this companion assignment; stop or edit that profile before legacy reassignment.")?;
        ensure(
            snapshot(&inner, expected.keys().cloned())? == expected,
            "Companion assignments changed externally; preserve the current authority.",
        )?;
        let changed = expected
            .keys()
            .filter(|id| expected[*id] != desired[*id])
            .cloned()
            .collect::<Vec<_>>();
        if changed.is_empty() {
            return Ok(json_snapshot(&desired));
        }
        let record = plan(&inner, &desired)?;
        // Admission above is complete before any owned source is released.
        let sessions = inner
            .sessions
            .iter()
            .filter(|(_, s)| changed.contains(&s.token.source_id))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for session in sessions {
            self.disarm(&mut inner, &session, "captured-assignment-changed", true);
        }
        let register = (|| {
            for instance in &changed {
                if let Some(a) = record.instances.iter().find(|a| &a.instance_id == instance) {
                    self.sink.register(a.definition.clone())?;
                }
            }
            self.persist(&record)
        })();
        if let Err(error) = register {
            // Retained definitions are not authority and cannot open a session.
            // Restore old descriptors where possible; failed owners stay disarmed.
            let mut failures = Vec::new();
            for instance in &changed {
                if let Some(a) = inner
                    .record
                    .instances
                    .iter()
                    .find(|a| &a.instance_id == instance)
                {
                    if let Err(e) = self.sink.register(a.definition.clone()) {
                        failures.push(e);
                    }
                }
            }
            if !failures.is_empty() {
                inner.diagnostic=Some(format!("Assignment authority was preserved; descriptor restoration needs attention: {}",failures.join("; ")));
            }
            return Err(format!(
                "Captured assignments were not committed; changed sources remain disarmed: {error}"
            ));
        }
        inner.record = record;
        for instance in changed {
            inner.feedback.remove(&instance);
        }
        Ok(json_snapshot(&desired))
    }
}
