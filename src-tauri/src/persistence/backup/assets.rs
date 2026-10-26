use super::*;
use std::io::{Read, Write};
pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(super) fn read(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    osg_persistence::check_path(path, false)?;
    let mut data = Vec::new();
    fs::File::open(path)
        .map_err(|_| "A backup asset is unavailable.")?
        .take(limit + 1)
        .read_to_end(&mut data)
        .map_err(|_| "Cannot read a backup asset.")?;
    if data.len() as u64 > limit {
        return Err("A backup asset exceeds its byte budget.".into());
    }
    Ok(data)
}
pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    osg_persistence::check_path(path, true)?;
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| "Cannot create a backup asset; its path may already exist.")?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "Cannot commit backup asset bytes.".into())
}
fn artwork(value: &Value, paths: &mut BTreeMap<String, String>) -> Result<(), String> {
    match value {
        Value::String(text) if text.starts_with("artwork:") => {
            let name = text.strip_prefix("artwork:").unwrap();
            let hash = name
                .strip_suffix(".jpg")
                .filter(|v| model::hash(v))
                .ok_or("Invalid managed artwork reference in backup.")?;
            paths.insert(format!("game-artwork/{name}"), hash.into());
        }
        Value::Array(values) => {
            for value in values {
                artwork(value, paths)?;
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                artwork(value, paths)?;
            }
        }
        _ => {}
    }
    Ok(())
}
pub(super) fn collect(
    database: &Database,
    root: &Path,
    directory: &Path,
) -> Result<Vec<Asset>, String> {
    let mut paths = BTreeMap::new();
    if let Some(record) = database.read("games", "library")? {
        let settings: crate::game_installations::Settings =
            serde_json::from_slice(&record.payload).map_err(|_| "Invalid saved game registry.")?;
        settings.validate()?;
        artwork(
            &serde_json::to_value(settings).map_err(|_| "Cannot inspect game artwork.")?,
            &mut paths,
        )?;
    }
    if let Some(record) = database.read(
        "recovery-dcs",
        "games.integration-setup.setup-work/dcs-setup-journal.json",
    )? {
        let document: Value =
            serde_json::from_slice(&record.payload).map_err(|_| "Invalid DCS backup evidence.")?;
        let records = document["pending"]
            .as_object()
            .map(|_| &document["pending"])
            .into_iter()
            .chain(
                document["receipts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|receipt| &receipt["record"]),
            );
        for record in records {
            let operation = record["id"]
                .as_str()
                .filter(|v| model::id(v))
                .ok_or("Invalid DCS backup identity.")?;
            for (key, name) in [
                ("exportBefore", "Export.lua.before"),
                ("aircraftBefore", "DCSAircraft.lua.before"),
                ("physicsBefore", "DCSPhysics.lua.before"),
            ] {
                if let Some(hash) = record[key].as_str() {
                    if !model::hash(hash) {
                        return Err("Invalid DCS backup fingerprint.".into());
                    }
                    paths.insert(
                        format!("games.integration-setup.setup-work/{operation}/{name}"),
                        hash.into(),
                    );
                }
            }
        }
    }
    if paths.len() > MAX_ASSETS {
        return Err("Backup asset count exceeds its bound.".into());
    }
    let mut assets = vec![];
    let mut total = 0;
    for (path, expected) in paths {
        let bytes = read(&root.join(&path), MAX_ASSET_BYTES)?;
        if digest(&bytes) != expected {
            return Err("A referenced backup asset changed; no backup was published.".into());
        }
        total += bytes.len() as u64;
        if total > MAX_TOTAL_ASSETS {
            return Err("Backup assets exceed their byte budget.".into());
        }
        let target = directory.join("assets").join(&expected);
        if !target
            .try_exists()
            .map_err(|_| "Cannot inspect backup asset storage.")?
        {
            write_new(&target, &bytes)?;
        }
        assets.push(Asset {
            path,
            sha256: expected,
            size: bytes.len() as u64,
        });
    }
    Ok(assets)
}
pub(super) fn verify(directory: &Path, manifest: &Manifest) -> Result<(), String> {
    for asset in &manifest.assets {
        let bytes = read(
            &directory.join("assets").join(&asset.sha256),
            MAX_ASSET_BYTES,
        )?;
        if bytes.len() as u64 != asset.size || digest(&bytes) != asset.sha256 {
            return Err(
                "A backup asset failed its hash check. Existing data was preserved.".into(),
            );
        }
    }
    Ok(())
}
pub(super) fn restore(root: &Path, directory: &Path, manifest: &Manifest) -> Result<(), String> {
    for asset in &manifest.assets {
        if !asset.path.starts_with("game-artwork/") {
            continue;
        }
        let bytes = read(
            &directory.join("assets").join(&asset.sha256),
            MAX_ASSET_BYTES,
        )?;
        let target = root.join(&asset.path);
        osg_persistence::check_path(&target, false)?;
        if target
            .try_exists()
            .map_err(|_| "Cannot inspect managed artwork.")?
        {
            if digest(&read(&target, MAX_ASSET_BYTES)?) != asset.sha256 {
                return Err("Current managed artwork conflicts with the backup. Existing bytes were preserved.".into());
            }
        } else {
            write_new(&target, &bytes)?;
        }
    }
    Ok(())
}
