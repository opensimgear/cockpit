//! Exact filesystem inputs read by DCS setup for one selected user folder.
//!
//! Missing files are returned too: the watcher can then observe their creation
//! from the nearest existing parent without recursively watching a game tree.
use super::RUNTIME_HASHES;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const AH6_INVENTORY: &str = include_str!("../../../examples/dcs-bios/aircraft/AH-6J.json");
const OH6_INVENTORY: &str = include_str!("../../../examples/dcs-bios/aircraft/OH-6A.json");

/// Return exact files whose contents or presence can change DCS setup inspection.
///
/// Parent paths inside the selected roots keep folder replacement observable.
/// Callers watch `saved_games_root()` separately for profile discovery.
pub(crate) fn setup_watch_paths(target: &Path, install: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = BTreeSet::new();
    let mut roots = vec![target];
    if let Some(install) = install {
        if install != target {
            roots.push(install);
        }
    }

    for (relative, _) in RUNTIME_HASHES {
        paths.insert(target.join("Scripts/DCS-BIOS").join(relative));
    }
    let module = target.join("Scripts/DCS-BIOS");
    paths.insert(module.join("BIOSConfig.lua"));
    let catalogue = module.join("doc/json");
    for name in [
        "MetadataStart.json",
        "MetaDataStart.json",
        "MetadataEnd.json",
        "MetaDataEnd.json",
        "AircraftAliases.json",
    ] {
        paths.insert(catalogue.join(name));
    }
    paths.insert(target.join("Scripts/Export.lua"));
    paths.insert(target.join("Scripts/OpenSimGear/DCSAircraft.lua"));
    paths.insert(target.join("Scripts/OpenSimGear/DCSPhysics.lua"));

    for root in roots {
        for (folder, inventory) in [("AH-6J", AH6_INVENTORY), ("OH-6A", OH6_INVENTORY)] {
            let mod_root = root.join("Mods/aircraft").join(folder);
            paths.insert(mod_root.join("entry.lua"));
            let source_hashes = serde_json::from_str::<Value>(inventory)
                .expect("checked-in aircraft inventory must be valid JSON");
            if let Some(sources) = source_hashes.get("sourceHashes").and_then(Value::as_object) {
                for source in sources.keys() {
                    // Inventory paths are checked-in relative paths consumed by
                    // aircraft::source_matches; preserve that exact resolution.
                    paths.insert(mod_root.join("Cockpit/Scripts").join(source));
                }
            }
        }
    }
    let parents = paths.iter().flat_map(|path| {
        let root = if path.starts_with(target) { target } else { install.unwrap_or(target) };
        path.ancestors().skip(1).take_while(move |parent| parent.starts_with(root)).map(Path::to_path_buf)
    }).collect::<Vec<_>>();
    paths.extend(parents);
    paths.into_iter().collect()
}

/// Exact installation-side input used by automatic Saved Games variant discovery.
pub(crate) fn install_discovery_watch_paths(install: Option<&Path>) -> Vec<PathBuf> {
    install
        .filter(|path| path.is_absolute())
        .map(|path| vec![path.join("dcs_variant.txt")])
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn includes_all_runtime_and_setup_files_even_when_missing() {
        let target = Path::new("C:/Saved Games/DCS");
        let paths = setup_watch_paths(target, None);
        for (relative, _) in RUNTIME_HASHES {
            assert!(paths.contains(&target.join("Scripts/DCS-BIOS").join(relative)));
        }
        for relative in [
            "Scripts/DCS-BIOS/BIOSConfig.lua",
            "Scripts/DCS-BIOS/doc/json/MetadataStart.json",
            "Scripts/DCS-BIOS/doc/json/MetaDataStart.json",
            "Scripts/DCS-BIOS/doc/json/MetadataEnd.json",
            "Scripts/DCS-BIOS/doc/json/MetaDataEnd.json",
            "Scripts/DCS-BIOS/doc/json/AircraftAliases.json",
            "Scripts/Export.lua",
            "Scripts/OpenSimGear/DCSAircraft.lua",
            "Scripts/OpenSimGear/DCSPhysics.lua",
        ] {
            assert!(paths.contains(&target.join(relative)), "missing {relative}");
        }
    }

    #[test]
    fn includes_selected_install_mod_inputs_but_no_sibling_install() {
        let target = Path::new("C:/Saved Games/DCS");
        let install = Path::new("D:/Games/DCS");
        let paths = setup_watch_paths(target, Some(install));
        for base in [target, install] {
            assert!(paths.contains(&base.join("Mods/aircraft/AH-6J/entry.lua")));
            assert!(paths.contains(&base.join("Mods/aircraft/OH-6A/entry.lua")));
        }
        assert!(!paths.iter().any(|p| p.starts_with("E:/Games/DCS")));
        assert_eq!(paths.len(), paths.iter().collect::<BTreeSet<_>>().len());
    }

    #[test]
    fn discovery_tracks_only_variant_file_and_leaves_saved_games_root_explicit() {
        let install = Path::new("D:/Games/DCS");
        assert_eq!(
            install_discovery_watch_paths(Some(install)),
            vec![install.join("dcs_variant.txt")]
        );
        assert!(install_discovery_watch_paths(None).is_empty());
        assert!(install_discovery_watch_paths(Some(Path::new("relative"))).is_empty());
    }

    #[test]
    fn includes_checked_in_mod_source_inventory_paths_for_creation_wakes() {
        let target = Path::new("C:/Saved Games/DCS");
        let paths = setup_watch_paths(target, None);
        for (folder, inventory) in [("AH-6J", AH6_INVENTORY), ("OH-6A", OH6_INVENTORY)] {
            let inventory: Value = serde_json::from_str(inventory).unwrap();
            let sources = inventory["sourceHashes"].as_object().unwrap();
            assert!(!sources.is_empty());
            for source in sources.keys() {
                assert!(paths.contains(
                    &target
                        .join("Mods/aircraft")
                        .join(folder)
                        .join("Cockpit/Scripts")
                        .join(source)
                ));
            }
        }
    }
}
