//! Optional aircraft extensions, scoped to the chosen installation/user folder.
use super::*;

pub(super) const RELATIVE: &str = "OpenSimGear/DCSAircraft.lua";
const HEADER: &str = "-- OpenSimGear managed aircraft exports v1\n-- sha256: ";
const BLOCK: &str = "-- OpenSimGear aircraft exports begin\npackage.path = lfs.writedir() .. '?.lua;' .. package.path\ndofile(lfs.writedir() .. [[Scripts\\OpenSimGear\\DCSAircraft.lua]])\n-- OpenSimGear aircraft exports end\n";
const AH6: &str = include_str!("../../../examples/dcs-bios/aircraft/AH-6J.json");
const OH6: &str = include_str!("../../../examples/dcs-bios/aircraft/OH-6A.json");
const HOOK: &str = include_str!("../../../examples/dcs-bios/AircraftExports.lua");

pub(super) struct Plan {
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
    pub export: Vec<u8>,
    pub status: Vec<Value>,
}

fn regular_chain(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        reject_link(ancestor)?;
    }
    Ok(())
}

fn advertises(bytes: &[u8], name: &str) -> Result<bool, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "Mod entry.lua is not UTF-8.")?;
    let parsed = tokens(text)?;
    Ok(parsed.windows(3).any(|row| {
        row == [
            Token::Word("make_flyable".into()),
            Token::Word("(".into()),
            Token::Text(name.to_ascii_lowercase()),
        ]
    }))
}

fn source_matches(root: &Path, catalogue: &Value) -> Result<bool, String> {
    let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    for (name, expected) in catalogue["sourceHashes"].as_object().unwrap() {
        let path = root.join("Cockpit/Scripts").join(safe_relative(name)?);
        regular_chain(&path)?;
        if !path.exists() || hash(&read(&path, MAX_FILE)?) != expected.as_str().unwrap() {
            return Ok(false);
        }
    }
    Ok(true)
}

fn find_mod(
    target: &Path,
    installation: Option<&Path>,
    folder: &str,
) -> Result<Option<PathBuf>, String> {
    // Saved Games mods take precedence. Never search other installations/variants.
    for base in std::iter::once(target).chain(installation) {
        if !base.is_absolute() {
            return Err("DCS installation must be absolute.".into());
        }
        let base = if base.exists() {
            fs::canonicalize(base).map_err(|e| e.to_string())?
        } else {
            base.to_path_buf()
        };
        let root = base.join("Mods/aircraft").join(folder);
        regular_chain(&root)?;
        if root.join("entry.lua").exists() {
            return fs::canonicalize(&root)
                .map(Some)
                .map_err(|e| format!("Cannot resolve mod folder: {e}"));
        }
    }
    Ok(None)
}

fn lua_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    // DCS's Lua 5.1 C stdio needs a regular path, not Win32's extended namespace.
    let path = if let Some(unc) = path.strip_prefix("\\\\?\\UNC\\") {
        format!("//{}", unc.replace('\\', "/"))
    } else {
        path.strip_prefix("\\\\?\\")
            .unwrap_or(&path)
            .replace('\\', "/")
    };
    path
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

fn export_bytes(bytes: &[u8], enabled: bool) -> Result<Vec<u8>, String> {
    loader_count(bytes)?; // Refuse unsupported encodings before touching byte offsets.
    let bom = if bytes.starts_with(b"\xef\xbb\xbf") {
        3
    } else {
        0
    };
    let body = &bytes[bom..];
    let body = body.strip_prefix(BLOCK.as_bytes()).unwrap_or(body);
    let text = String::from_utf8_lossy(body);
    let parsed = tokens(&text)?;
    if text.contains("-- OpenSimGear aircraft exports") || parsed.iter().any(|token|
        matches!(token, Token::Text(value) if value.contains("dcsaircraft") || value.contains("mh6commondata"))) {
        return Err("An existing or edited aircraft export loader needs review. Restore the temporary MH-6J hook or edited managed block before setup; existing scripts were preserved.".into());
    }
    let mut result = bytes[..bom].to_vec();
    if enabled {
        result.extend_from_slice(BLOCK.as_bytes());
    }
    result.extend_from_slice(body);
    with_loader(&result)
}

pub(super) fn plan(target: &Path, installation: Option<&Path>) -> Result<Plan, String> {
    let target = checked_target(target)?;
    let mut specs = Vec::new();
    let mut status = Vec::new();
    for (folder, name, catalogue) in [
        ("AH-6J", "MH-6J", None),
        ("AH-6J", "AH-6J", Some(AH6)),
        ("OH-6A", "OH-6A", Some(OH6)),
    ] {
        let root = find_mod(&target, installation, folder)?;
        let found = match &root {
            Some(root) => advertises(&read(&root.join("entry.lua"), MAX_FILE)?, name)?,
            None => false,
        };
        let mut spec = catalogue
            .map(|source| serde_json::from_str::<Value>(source).unwrap())
            .unwrap_or_else(|| json!({"name":name,"base":0xc000}));
        let compatible = if found {
            catalogue.is_none() || source_matches(root.as_ref().unwrap(), &spec)?
        } else {
            false
        };
        status.push(json!({"aircraft":name,"detected":found,"compatible":compatible,
            "path":root,"capability":if catalogue.is_none(){"common-data-only"}else{"controls-and-gauges"},
            "message":if !found {"Mod not installed for this DCS; hook omitted."}
                else if !compatible {"Mod control definitions changed; hook omitted pending mapping review."}
                else {"Matching mod detected. Hook included by integration setup; launch DCS to generate its catalogue."}}));
        if compatible {
            spec["path"] = json!(lua_path(&root.unwrap()));
            spec.as_object_mut().unwrap().remove("sourceHashes");
            specs.push(spec);
        }
    }
    let path = target.join("Scripts").join(RELATIVE);
    regular_chain(&path)?;
    let before = if path.exists() {
        Some(read(&path, MAX_FILE)?)
    } else {
        None
    };
    if before.as_ref().is_some_and(|bytes| !owned(bytes)) {
        return Err("DCSAircraft.lua is foreign or was edited. Its contents were preserved; review it before setup.".into());
    }
    let after = if specs.is_empty() {
        None
    } else {
        let json = serde_json::to_string(&specs).map_err(|e| e.to_string())?;
        let mut delimiter = "=".to_owned();
        while json.contains(&format!("]{delimiter}]")) {
            delimiter.push('=');
        }
        let body = format!("local configured = require('Scripts.DCS-BIOS.lib.ext.JSON'):decode([{delimiter}[{json}]{delimiter}])\n{HOOK}");
        Some(format!("{HEADER}{}\n{body}", hash(body.as_bytes())).into_bytes())
    };
    let export = target.join("Scripts/Export.lua");
    let bytes = if export.exists() {
        read(&export, MAX_EXPORT)?
    } else {
        Vec::new()
    };
    let export = export_bytes(&bytes, after.is_some())?;
    Ok(Plan {
        before,
        after,
        export,
        status,
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
        return Err("Aircraft export changed during setup; it was preserved.".into());
    }
    if current != plan.after {
        if let Some(bytes) = &plan.after {
            fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
            replace(&path, bytes)?;
        } else {
            fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub(super) fn restore(
    target: &Path,
    backup: &Path,
    journal: &Value,
    conflicts: &mut Vec<String>,
) -> Result<(), String> {
    if journal.get("aircraftBefore").is_none() {
        return Ok(());
    } // schema 1 legacy journals
    let target = checked_target(target)?;
    let path = target.join("Scripts").join(RELATIVE);
    regular_chain(&path)?;
    let current = if path.exists() {
        Some(hash(&read(&path, MAX_FILE)?))
    } else {
        None
    };
    let before = journal["aircraftBefore"].as_str();
    if current.as_deref() == before {
        return Ok(());
    }
    if current.as_deref() != journal["aircraftAfter"].as_str() {
        conflicts.push("Aircraft export changed after setup; it was preserved.".into());
        return Ok(());
    }
    if let Some(expected) = before {
        let bytes = read(&backup.join("DCSAircraft.lua.before"), MAX_FILE)?;
        if hash(&bytes) != expected {
            return Err("Aircraft export backup failed its integrity check.".into());
        }
        replace(&path, &bytes)?;
    } else {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    if journal["createdAircraftDir"] == true {
        let _ = fs::remove_dir(path.parent().unwrap());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tree() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }
    fn mod_entry(base: &Path, folder: &str, entry: &str) -> PathBuf {
        let root = base.join("Mods/aircraft").join(folder);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("entry.lua"), entry).unwrap();
        root
    }
    #[test]
    fn detection_is_scoped_to_selected_install_and_user_folder() {
        let scratch = tree();
        let user = scratch.path().join("DCS");
        let install = scratch.path().join("selected");
        let other = scratch.path().join("other");
        fs::create_dir_all(&user).unwrap();
        mod_entry(&other, "AH-6J", "make_flyable('MH-6J')");
        let absent = plan(&user, Some(&install)).unwrap();
        assert!(absent.after.is_none());
        assert!(absent.status.iter().all(|s| s["detected"] == false));
        mod_entry(
            &install,
            "AH-6J",
            "-- make_flyable('AH-6J')\nlocal x=\"make_flyable('OH-6A')\"\nmake_flyable('MH-6J')",
        );
        let selected = plan(&user, Some(&install)).unwrap();
        assert_eq!(selected.status[0]["compatible"], true);
        assert_eq!(selected.status[1]["detected"], false);
        let hook = String::from_utf8(selected.after.unwrap()).unwrap();
        assert!(hook.contains("\"name\":\"MH-6J\""));
        assert!(!hook.contains("\"name\":\"AH-6J\""));
        mod_entry(&user, "AH-6J", "make_flyable('AH-6J')");
        let precedence = plan(&user, Some(&install)).unwrap();
        assert_eq!(precedence.status[0]["detected"], false);
        assert_eq!(precedence.status[1]["detected"], true);
        assert_eq!(precedence.status[1]["compatible"], false);
        assert!(precedence.after.is_none());
    }
    #[test]
    fn canonical_and_regular_paths_generate_identical_hooks() {
        let scratch = tree();
        let user = scratch.path().join("DCS");
        mod_entry(&user, "AH-6J", "make_flyable('MH-6J')");
        assert_eq!(
            plan(&user, None).unwrap().after,
            plan(&fs::canonicalize(&user).unwrap(), None).unwrap().after
        );
        #[cfg(windows)]
        assert_eq!(
            lua_path(Path::new(r"\\?\C:\Saved Games\DCS")),
            "C:/Saved Games/DCS"
        );
    }
    #[test]
    fn fingerprints_check_every_definition_and_refuse_changed_sources() {
        let scratch = tree();
        let root = scratch.path();
        fs::create_dir_all(root.join("Cockpit/Scripts")).unwrap();
        fs::write(root.join("Cockpit/Scripts/devices.lua"), b"device ids").unwrap();
        let catalogue = json!({"sourceHashes":{"devices.lua":hash(b"device ids")}});
        assert!(source_matches(root, &catalogue).unwrap());
        fs::write(root.join("Cockpit/Scripts/devices.lua"), b"new device ids").unwrap();
        assert!(!source_matches(root, &catalogue).unwrap());
        fs::remove_file(root.join("Cockpit/Scripts/devices.lua")).unwrap();
        assert!(!source_matches(root, &catalogue).unwrap());
        for source in [AH6, OH6] {
            let catalogue: Value = serde_json::from_str(source).unwrap();
            assert_eq!(catalogue["sourceHashes"].as_object().unwrap().len(), 5);
            assert!(catalogue["inputs"].as_array().unwrap().len() > 60);
            assert!(catalogue["outputs"].as_array().unwrap().len() > 80);
        }
    }
    #[test]
    fn loader_order_idempotence_and_removal_preserve_original_bytes() {
        let original = b"\xef\xbb\xbf-- user ANSI \xff\r\ndofile('Helios.lua')\r\ndofile(lfs.writedir() .. [[Scripts\\DCS-BIOS\\BIOS.lua]])\r\n";
        let installed = export_bytes(original, true).unwrap();
        assert_eq!(&installed[..3], &original[..3]);
        assert!(installed[3..].starts_with(BLOCK.as_bytes()));
        assert!(installed.ends_with(&original[3..]));
        assert_eq!(export_bytes(&installed, true).unwrap(), installed);
        assert_eq!(export_bytes(&installed, false).unwrap(), original);
        assert!(export_bytes(b"require('Scripts.OpenSimGear.MH6CommonData')", true).is_err());
        assert!(export_bytes(b"-- OpenSimGear aircraft exports edited", true).is_err());
        assert!(export_bytes(b"\xff\xfea\0", true).is_err());
    }
    #[test]
    fn owned_hook_update_removal_and_foreign_change_protection() {
        let scratch = tree();
        let root = scratch.path();
        let mod_root = mod_entry(root, "AH-6J", "make_flyable('MH-6J')");
        let first = plan(root, None).unwrap();
        assert!(first.before.is_none());
        apply(root, &first).unwrap();
        fs::write(root.join("Scripts/Export.lua"), first.export).unwrap();
        let same = plan(root, None).unwrap();
        assert_eq!(same.before, same.after);
        let path = root.join("Scripts").join(RELATIVE);
        fs::write(mod_root.join("entry.lua"), "-- mod removed").unwrap();
        let remove = plan(root, None).unwrap();
        assert!(remove.after.is_none());
        fs::write(&path, b"user change").unwrap();
        assert!(apply(root, &remove).unwrap_err().contains("preserved"));
        assert!(plan(root, None).err().unwrap().contains("edited"));
        fs::write(&path, remove.before.as_ref().unwrap()).unwrap();
        apply(root, &remove).unwrap();
        assert!(!path.exists());
        assert!(!String::from_utf8(remove.export)
            .unwrap()
            .contains("DCSAircraft"));
    }
}
