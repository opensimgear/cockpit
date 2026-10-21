//! Managed read-only ownship extension, using the parent byte-checked transaction.
use super::*;
pub(super) const RELATIVE: &str = "OpenSimGear/DCSPhysics.lua";
const HEADER: &str = "-- OpenSimGear managed ownship physics v1\n-- sha256: ";
const BLOCK: &str = "-- OpenSimGear ownship physics begin\npackage.path = lfs.writedir() .. '?.lua;' .. package.path\npcall(dofile, lfs.writedir() .. [[Scripts\\OpenSimGear\\DCSPhysics.lua]])\n-- OpenSimGear ownship physics end\n";
pub(super) struct Plan {
    pub before: Option<Vec<u8>>,
    pub after: Vec<u8>,
    pub export: Vec<u8>,
}
fn regular_chain(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        reject_link(ancestor)?;
    }
    Ok(())
}
fn owned(bytes: &[u8]) -> bool {
    let Some(rest) = bytes.strip_prefix(HEADER.as_bytes()) else {
        return false;
    };
    let Some(newline) = rest.iter().position(|b| *b == b'\n') else {
        return false;
    };
    rest[..newline] == *hash(&rest[newline + 1..]).as_bytes()
}
fn export_bytes(bytes: &[u8]) -> Result<Vec<u8>, String> {
    loader_count(bytes)?;
    let positions = bytes
        .windows(BLOCK.len())
        .enumerate()
        .filter_map(|(index, value)| (value == BLOCK.as_bytes()).then_some(index))
        .collect::<Vec<_>>();
    if positions.len() > 1 {
        return Err("Duplicate managed DCS ownship loaders need review; existing export scripts were preserved.".into());
    }
    let mut body = bytes.to_vec();
    if let Some(index) = positions.first() {
        body.drain(*index..*index + BLOCK.len());
    }
    let text = String::from_utf8_lossy(&body);
    if text.contains("-- OpenSimGear ownship physics")
        || tokens(&text)?
            .iter()
            .any(|token| matches!(token,Token::Text(value) if value.contains("dcsphysics")))
    {
        return Err("An existing or edited DCS ownship export loader needs review; existing scripts were preserved.".into());
    }
    if !body.is_empty() && !body.ends_with(b"\n") {
        body.push(b'\n');
    }
    body.extend_from_slice(BLOCK.as_bytes());
    Ok(body)
}
pub(super) fn plan(target: &Path, export: &[u8]) -> Result<Plan, String> {
    let target = checked_target(target)?;
    let path = target.join("Scripts").join(RELATIVE);
    regular_chain(&path)?;
    let before = if path.exists() {
        Some(read(&path, MAX_FILE)?)
    } else {
        None
    };
    if before.as_ref().is_some_and(|bytes| !owned(bytes)) {
        return Err("DCSPhysics.lua is foreign or was edited. Its contents were preserved; review it before setup.".into());
    }
    let body = crate::dcs_physics::export_source();
    let after = format!("{HEADER}{}\n{body}", hash(body.as_bytes())).into_bytes();
    Ok(Plan {
        before,
        after,
        export: export_bytes(export)?,
    })
}
pub(super) fn apply(target: &Path, plan: &Plan) -> Result<(), String> {
    let target = checked_target(target)?;
    let path = target.join("Scripts").join(RELATIVE);
    regular_chain(&path)?;
    let current = if path.exists() {
        Some(read(&path, MAX_FILE)?)
    } else {
        None
    };
    if current != plan.before {
        return Err("DCS ownship export changed during setup; it was preserved.".into());
    }
    if current.as_ref() != Some(&plan.after) {
        fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        replace(&path, &plan.after)?;
    }
    Ok(())
}
pub(super) fn restore(
    target: &Path,
    backup: &Path,
    journal: &Value,
    conflicts: &mut Vec<String>,
) -> Result<(), String> {
    if journal.get("physicsBefore").is_none() {
        return Ok(());
    }
    let target = checked_target(target)?;
    let path = target.join("Scripts").join(RELATIVE);
    regular_chain(&path)?;
    let current = if path.exists() {
        Some(hash(&read(&path, MAX_FILE)?))
    } else {
        None
    };
    let before = journal["physicsBefore"].as_str();
    if current.as_deref() == before {
        return Ok(());
    }
    if current.as_deref() != journal["physicsAfter"].as_str() {
        conflicts.push("DCS ownship export changed after setup; it was preserved.".into());
        return Ok(());
    }
    if let Some(expected) = before {
        let bytes = read(&backup.join("DCSPhysics.lua.before"), MAX_FILE)?;
        if hash(&bytes) != expected {
            return Err("DCS ownship export backup failed its integrity check.".into());
        }
        replace(&path, &bytes)?;
    } else {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    if journal["createdPhysicsDir"] == true {
        let _ = fs::remove_dir(path.parent().unwrap());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loader_appends_after_other_hooks_preserves_bytes_and_is_idempotent() {
        let original = b"\xef\xbb\xbf-- Tacview\r\nLuaExportStart = customStart\r\n";
        let once = export_bytes(original).unwrap();
        assert!(once.starts_with(original));
        assert!(once.ends_with(BLOCK.as_bytes()));
        assert_eq!(export_bytes(&once).unwrap(), once);
        let mut later = once.clone();
        later.extend_from_slice(b"-- another exporter\nLuaExportStop = customStop\n");
        let repaired = export_bytes(&later).unwrap();
        assert!(repaired.ends_with(BLOCK.as_bytes()));
        assert!(String::from_utf8(repaired)
            .unwrap()
            .contains("LuaExportStop = customStop\n-- OpenSimGear ownship physics begin"));
    }
    #[test]
    fn edited_and_duplicate_loaders_are_preserved() {
        for value in [
            format!("{BLOCK}{BLOCK}"),
            BLOCK.replace("pcall(dofile", "dofile"),
            "dofile('Scripts/OpenSimGear/DCSPhysics.lua')".into(),
        ] {
            assert!(export_bytes(value.as_bytes()).is_err());
        }
    }
}
