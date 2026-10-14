//! Exact representation of HidHide's one global application policy.
//! Never broaden a per-device selection or overwrite externally managed devices.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
pub const LIMIT: usize = 65_536;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct State {
    pub active: bool,
    pub inverse: bool,
    pub applications: Vec<String>,
    pub hidden: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Policy {
    Visible,
    HideAll,
    HideFromProcess { executable: String },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    pub instance: String,
    pub policy: Policy,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Plan {
    pub before: State,
    pub after: State,
    pub capture: String,
    pub selections: Vec<Selection>,
}
pub fn key(value: &str) -> String {
    value.to_lowercase()
}
pub fn contains(values: &[String], value: &str) -> bool {
    values.iter().any(|v| key(v) == key(value))
}
fn valid_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
}
pub fn executable(value: &str) -> Result<(), String> {
    if !valid_text(value)
        || !key(value).starts_with("\\device\\")
        || !key(value).ends_with(".exe")
        || value.split('\\').any(|v| v == "." || v == "..")
    {
        return Err("Resolve an existing executable to its exact Windows image path.".into());
    }
    Ok(())
}
pub fn instance(value: &str) -> Result<(), String> {
    if !valid_text(value)
        || value.len() > 1024
        || !key(value).starts_with("hid\\")
        || value.contains('/')
        || value.contains('*')
        || value.contains('?')
    {
        return Err("An exact physical HID instance is required; wildcard or virtual selections are unsupported.".into());
    }
    Ok(())
}
impl State {
    pub fn validate(&self) -> Result<(), String> {
        for values in [&self.applications, &self.hidden] {
            if values.len() > 256
                || values.iter().any(|v| !valid_text(v))
                || values.iter().map(|v| key(v)).collect::<BTreeSet<_>>().len() != values.len()
                || values
                    .iter()
                    .map(|v| (v.encode_utf16().count() + 1) * 2)
                    .sum::<usize>()
                    + 2
                    > LIMIT
            {
                return Err("HidHide policy is oversized, duplicated or malformed; preserve it for inspection.".into());
            }
        }
        Ok(())
    }
    pub fn capture_allowed(&self, capture: &str) -> bool {
        !self.active
            || if self.inverse {
                !contains(&self.applications, capture)
            } else {
                contains(&self.applications, capture)
            }
    }
}
impl Plan {
    pub fn build(
        before: State,
        selections: Vec<Selection>,
        capture: String,
    ) -> Result<Self, String> {
        before.validate()?;
        executable(&capture)?;
        if selections.is_empty() || selections.len() > 16 {
            return Err("Select 1 to 16 controllers.".into());
        }
        let mut seen = BTreeSet::new();
        let mut desired = None;
        let mut additions = vec![];
        for selection in &selections {
            instance(&selection.instance)?;
            if !seen.insert(key(&selection.instance)) {
                return Err("Duplicate controller selection.".into());
            }
            let policy = match &selection.policy {
                Policy::Visible => continue, // External hiding is reported, never removed.
                Policy::HideAll => (false, None),
                Policy::HideFromProcess { executable: target } => {
                    executable(target)?;
                    if key(target) == key(&capture) {
                        return Err("The capture executable cannot be blocked.".into());
                    }
                    (true, Some(key(target)))
                }
            };
            if desired.as_ref().is_some_and(|v| v != &policy) {
                return Err("These devices require incompatible global HidHide policies.".into());
            }
            desired = Some(policy);
            if !contains(&before.hidden, &selection.instance) {
                additions.push(selection.instance.clone());
            }
        }
        let mut after = before.clone();
        if let Some((inverse, target)) = desired {
            after.active = true;
            after.inverse = inverse;
            if let Some(target) = target {
                after.applications = vec![target];
            } else {
                if before.inverse {
                    after.applications.clear();
                }
                if !contains(&after.applications, &capture) {
                    after.applications.push(capture.clone());
                }
            }
            // Any global change affects every externally hidden device, even if
            // the user selected that device too. An inactive policy is not consent.
            let same_apps = |a: &[String], b: &[String]| {
                a.iter().map(|s| key(s)).collect::<BTreeSet<_>>()
                    == b.iter().map(|s| key(s)).collect()
            };
            if !before.hidden.is_empty()
                && (!before.active
                    || before.inverse != after.inverse
                    || !same_apps(&before.applications, &after.applications))
            {
                return Err("This would change another application's existing hidden-device policy. Resolve that policy explicitly first.".into());
            }
            if same_apps(&before.applications, &after.applications) {
                after.applications = before.applications.clone();
            }
            after.hidden.extend(additions);
        }
        after.validate()?;
        if !after.capture_allowed(&capture) && after != before {
            return Err("Capture access would be lost.".into());
        }
        Ok(Self {
            before,
            after,
            capture,
            selections,
        })
    }
    pub fn validate(&self) -> Result<(), String> {
        let expected = Self::build(
            self.before.clone(),
            self.selections.clone(),
            self.capture.clone(),
        )?;
        if expected != *self {
            return Err("Visibility journal does not match its captured policy intent.".into());
        }
        Ok(())
    }
    /// Each step changes just one property. Hide devices last, and unhide first
    /// when reversing the steps, preserving access during setup and cleanup.
    pub fn steps(&self) -> Result<Vec<State>, String> {
        self.validate()?;
        let mut steps = vec![self.before.clone()];
        let mut current = self.before.clone();
        for property in 0..4 {
            match property {
                0 => current.applications = self.after.applications.clone(),
                1 => current.inverse = self.after.inverse,
                2 => current.active = self.after.active,
                _ => current.hidden = self.after.hidden.clone(),
            }
            if steps.last() != Some(&current) {
                steps.push(current.clone());
            }
        }
        Ok(steps)
    }
}
#[cfg(test)]
#[path = "policy_tests.rs"]
pub(super) mod tests;
