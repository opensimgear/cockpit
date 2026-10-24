use super::*;
pub(super) fn settings_repository(
    path: &Path,
) -> Result<osg_persistence::Repository<Value>, String> {
    let db = osg_persistence::Database::for_namespace(path)?;
    osg_persistence::Repository::new(db, "games", "setup", MAX_SETTINGS, validate_settings)
}
pub(super) fn write_settings(inner: &Inner, value: &Value) -> Result<(), String> {
    inner.store.as_ref().map_err(Clone::clone)?.save(value)
}
pub(crate) fn read_settings(path: &Path) -> Result<Value, String> {
    settings_repository(path)?
        .load()
        .map(|value| value.unwrap_or(json!({"schemaVersion":1,"games":{}})))
}
pub(crate) fn validate_settings(value: &Value) -> Result<(), String> {
    if value.as_object().is_none_or(|object| {
        object
            .keys()
            .any(|key| !["schemaVersion", "games"].contains(&key.as_str()))
    }) || value["schemaVersion"] != 1
        || !value["games"].is_object()
        || value["games"].as_object().unwrap().keys().any(|k| {
            !matches!(
                k.as_str(),
                "dcs"
                    | "assetto-corsa-rally"
                    | "x-plane-11"
                    | "x-plane-12"
                    | "assetto-corsa"
                    | "assetto-corsa-competizione"
                    | "assetto-corsa-evo"
            )
        })
    {
        return Err(
            "Invalid integration setup settings. Preserve the file and repair it before setup."
                .into(),
        );
    }
    for (game, record) in value["games"].as_object().unwrap() {
        if !record.is_object() || !record["connection"].is_object() {
            return Err("Invalid saved integration setup record.".into());
        }
        validate_connection(game, &record["connection"])?;
        provider_settings::validate_saved_record(game, record)?;
        if game == "dcs" {
            absolute_folder(&record["userFolder"])?;
        }
    }
    Ok(())
}
pub(super) fn absolute_folder(value: &Value) -> Result<PathBuf, String> {
    let path = value
        .as_str()
        .filter(|p| !p.is_empty() && p.len() <= 4096 && !p.chars().any(char::is_control))
        .ok_or("Choose an absolute DCS Saved Games user folder.")?;
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err("Choose an absolute DCS Saved Games user folder.".into());
    }
    Ok(path)
}
pub(super) fn folder(value: &Value) -> Result<PathBuf, String> {
    let path = absolute_folder(value)?;
    if !path.is_dir() {
        return Err(
            "Choose an existing DCS Saved Games user folder. Start DCS once if it is missing."
                .into(),
        );
    }
    Ok(path)
}
pub(super) fn reject_game_install_folder(target: &Path, game: &Value) -> Result<(), String> {
    let Some(installation) = game["installPath"].as_str() else {
        return Ok(());
    };
    let target = fs::canonicalize(target).map_err(|e| e.to_string())?;
    let installation = fs::canonicalize(installation)
        .map_err(|e| format!("Cannot verify the game installation boundary: {e}"))?;
    let target_parts = target.components().collect::<Vec<_>>();
    let install_parts = installation.components().collect::<Vec<_>>();
    let inside = target_parts.len() >= install_parts.len()
        && target_parts.iter().zip(install_parts.iter()).all(|(a, b)| {
            if cfg!(windows) {
                a.as_os_str().to_string_lossy().to_lowercase()
                    == b.as_os_str().to_string_lossy().to_lowercase()
            } else {
                a == b
            }
        });
    if inside {
        return Err("Choose a DCS Saved Games user folder outside the game installation. Setup never writes inside the installed game's folders.".into());
    }
    Ok(())
}
pub(crate) fn dcs_connection(path: &Path) -> Value {
    // Match upstream's default multicast route instead of restricting membership
    // to the loopback interface. Existing explicit interface overrides are kept.
    // https://learn.microsoft.com/en-us/windows/win32/api/ws2ipdef/ns-ws2ipdef-ip_mreq
    json!({"cataloguePath":path,"freshnessMs":1500,"commandHost":"127.0.0.1","commandPort":7778,"listenAddress":"0.0.0.0","listenPort":5010,"multicastGroup":"239.255.50.10","multicastInterface":"0.0.0.0"})
}
pub(super) fn validate_connection(game: &str, value: &Value) -> Result<(), String> {
    if game.starts_with("x-plane-") {
        crate::xplane::validate_connection(value)?;
        if value["version"].as_u64() != game.trim_start_matches("x-plane-").parse::<u64>().ok() {
            return Err("X-Plane connection version must match the game.".into());
        }
        return Ok(());
    }
    if !value.is_object() || value.to_string().len() > 8192 {
        return Err("Invalid local game connection settings.".into());
    }
    let keys = if game == "dcs" {
        vec![
            "cataloguePath",
            "freshnessMs",
            "commandHost",
            "commandPort",
            "listenAddress",
            "listenPort",
            "multicastGroup",
            "multicastInterface",
        ]
    } else {
        vec!["freshnessMs", "source"]
    };
    if value
        .as_object()
        .unwrap()
        .keys()
        .any(|k| !keys.contains(&k.as_str()))
    {
        return Err("Unsupported local game connection setting.".into());
    }
    if value["freshnessMs"]
        .as_u64()
        .is_none_or(|v| !(100..=10000).contains(&v))
    {
        return Err("Use a freshness timeout from 100 to 10000 ms.".into());
    }
    if game != "dcs" {
        return if value["source"] == "shared-memory" {
            Ok(())
        } else {
            Err("Assetto integration requires the native shared-memory source.".into())
        };
    }
    if !value["cataloguePath"].as_str().is_some_and(|s| {
        s.len() <= 4096 && Path::new(s).is_absolute() && !s.chars().any(char::is_control)
    }) {
        return Err("Choose an absolute DCS catalogue directory.".into());
    }
    crate::dcs_bios::Network::parse(value)?;
    Ok(())
}
pub(super) fn compatible_dcs_connection(value: &Value) -> Result<(), String> {
    validate_connection("dcs", value)?;
    let network = crate::dcs_bios::Network::parse(value)?;
    if !network.command_host.is_loopback()
        || network.command_port != 7778
        || network.listen_port != 5010
        || network.multicast_group.map(|g| g.to_string()).as_deref() != Some("239.255.50.10")
    {
        return Err("Saved UDP overrides were preserved, but they do not match the installed DCS-BIOS defaults (local command port 7778; multicast 239.255.50.10:5010). Review the connection settings before integration.".into());
    }
    Ok(())
}
pub(super) fn blocking_acr_status(proof: &Value) -> bool {
    matches!(
        proof["status"].as_str(),
        Some(
            "unsupported-layout"
                | "access-denied"
                | "conflicting-producer"
                | "executable-changed"
                | "unsupported-platform"
        )
    )
}
