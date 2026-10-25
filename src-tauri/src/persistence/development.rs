//! Fresh development examples. No import, upgrade, protected identity or activation.
use osg_persistence::{Change, Database};
use osg_profiles::{store::Library, ProfileDocument};
use serde_json::{json, Value};

pub(crate) const SIGNALS_ID: &str = "00000000-0000-4000-8000-000000000100";
pub(crate) const WORKSPACE_ID: &str = "00000000-0000-4000-8000-000000000101";
pub(crate) const DCS_ID: &str = "f592a7b4-6f39-4267-a8f9-b46fbe4ff91a";

fn example(id: &str, name: &str) -> ProfileDocument {
    let mut document = osg_profiles::default_document();
    document.id = id.into();
    document.name = name.into();
    document
}
fn profiles() -> Result<Library, String> {
    let mut library = Library::default();
    let mut signals = example(SIGNALS_ID, "Dev · Signals");
    signals.resources=serde_json::from_value(json!([
        {"id":"demo-controls","name":"Demo controls","kind":"device","required":false,"ports":[{"id":"button","name":"Button state","direction":"output","contract":{"delivery":"state","schema":{"type":"boolean"},"freshnessMs":1000,"fallback":false}}]},
        {"id":"demo-panel","name":"Demo panel","kind":"device","required":false,"ports":[{"id":"pressed","name":"Press event","direction":"input","contract":{"delivery":"event","schema":{"type":"boolean"}}}]}
    ])).map_err(|_|"Invalid development logical resources.")?;
    signals.processing_nodes=serde_json::from_value(json!([
        {"id":"edge","name":"Rising edge","operation":{"kind":"event-edge","edge":"rising"},"inputs":[{"id":"in","name":"Button state","direction":"input","contract":{"delivery":"state","schema":{"type":"boolean"},"freshnessMs":1000,"fallback":false}}],"outputs":[{"id":"out","name":"Press event","direction":"output","contract":{"delivery":"event","schema":{"type":"boolean"}}}]}
    ])).map_err(|_|"Invalid development signal example.")?;
    signals.bindings=serde_json::from_value(json!([
        {"id":"button-edge","from":{"nodeId":"demo-controls","portId":"button"},"to":{"nodeId":"edge","portId":"in"}},
        {"id":"edge-panel","from":{"nodeId":"edge","portId":"out"},"to":{"nodeId":"demo-panel","portId":"pressed"}}
    ])).map_err(|_|"Invalid development signal wiring.")?;
    signals.editor.positions = vec![
        osg_profiles::NodePosition {
            node_id: "demo-controls".into(),
            x: 100.0,
            y: 140.0,
        },
        osg_profiles::NodePosition {
            node_id: "demo-panel".into(),
            x: 480.0,
            y: 140.0,
        },
    ];
    library
        .profiles
        .extend([signals, example(WORKSPACE_ID, "Dev · Workspace")]);
    let dcs: ProfileDocument = serde_json::from_str(include_str!(
        "../../../examples/dcs-bios/feature-lab.profile.json"
    ))
    .map_err(|_| "Invalid development DCS profile.")?;
    debug_assert_eq!(dcs.id, DCS_ID);
    library.profiles.push(dcs);
    library.validate()?;
    Ok(library)
}
fn workspace() -> Result<Value, String> {
    let workspace: Value = serde_json::from_str(include_str!(
        "../../../examples/dcs-bios/win-companion/workspace.json"
    ))
    .map_err(|_| "Invalid development designer workspace.")?;
    crate::widget_library::validate_workspace(&workspace)?;
    Ok(workspace)
}
pub(crate) fn seed(database: &Database) -> Result<(), String> {
    let rows = database.read_many(&[
        ("development", "initialized"),
        ("profiles", "library"),
        ("widgets", "library"),
        ("companion", "designer-workspace"),
    ])?;
    if let Some(marker) = &rows[0].1 {
        if marker.payload != b"true" {
            return Err(
                "Invalid development initialization marker. Reset the dev database.".into(),
            );
        }
        return Ok(());
    }
    if rows.iter().any(|(revision, _)| *revision != 0) {
        return Err("Development examples require a fresh SQLite database. Reset the dev data directory; existing data is not migrated.".into());
    }
    let library = profiles()?.encode()?;
    let workspace = workspace()?;
    let bundle = serde_json::to_vec(&json!({"definitions":workspace["sources"]}))
        .map_err(|_| "Cannot encode development widget sources.")?;
    let workspace = serde_json::to_vec(&workspace)
        .map_err(|_| "Cannot encode development designer workspace.")?;
    database.commit(&[
        Change {
            domain: "profiles",
            key: "library",
            expected_revision: 0,
            payload: Some(&library),
        },
        Change {
            domain: "widgets",
            key: "library",
            expected_revision: 0,
            payload: Some(&bundle),
        },
        Change {
            domain: "companion",
            key: "designer-workspace",
            expected_revision: 0,
            payload: Some(&workspace),
        },
        Change {
            domain: "development",
            key: "initialized",
            expected_revision: 0,
            payload: Some(b"true"),
        },
    ])?;
    Ok(())
}

#[cfg(test)]
mod tests;
