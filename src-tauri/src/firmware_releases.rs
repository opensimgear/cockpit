//! Operator-requested firmware acquisition. Ordinary status reads never use the network.
#[cfg(test)]
#[path="local_firmware.rs"]
mod local_firmware;
use osg_companion_serial::{FirmwareSegment, FirmwareTarget, OwnedFirmwareSegment};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read, Write},
    path::Path,
    sync::Mutex,
    time::{Duration, Instant},
};

include!(concat!(env!("OUT_DIR"), "/companion_firmware.rs"));

const API: &str = "https://api.github.com/repos/opensimgear/cockpit/releases";
const RELEASE_PREFIX: &str = "esp32-companion-v";
const MAX_ARCHIVE: usize = 10 * 1024 * 1024;
const MAX_CATALOG: usize = 2 * 1024 * 1024;
const REQUEST_BUDGET: Duration = Duration::from_secs(30);
const BOARDS: [(&str, &str); 2] = [
    ("waveshare-lcd7-s3", "Waveshare ESP32-S3 Touch LCD 7 · Rev 1.2 · 800×480"),
    ("sunton-3248s035r", "Sunton/GUITION ESP32-3248S035R · resistive · 480×320"),
];

#[derive(Clone)]
struct Asset { name: String, url: String, size: usize }
#[derive(Clone)]
struct Release { version: String, assets: BTreeMap<String, Asset> }
#[derive(Default)]
struct State { checked: bool, release: Option<Release>, error: Option<String>, observed: BTreeMap<String, (Value, String, String)> }
#[derive(Default)]
pub(crate) struct Resolver { state: Mutex<State>, operation: Mutex<()> }

fn source(development: bool) -> &'static str {
    if development { "development" } else { "github-release" }
}
fn target(board: &str) -> Result<FirmwareTarget, String> {
    match board {
        "waveshare-lcd7-s3" => Ok(FirmwareTarget::Esp32s3),
        "sunton-3248s035r" => Ok(FirmwareTarget::Esp32),
        _ => Err("Choose a supported board profile.".into()),
    }
}
fn local_version(board:&str)->Option<&'static str>{match board{"waveshare-lcd7-s3"=>WAVESHARE_FIRMWARE_VERSION,"sunton-3248s035r"=>SUNTON_FIRMWARE_VERSION,_=>None}}
fn embedded(board: &str) -> &'static [FirmwareSegment<'static>] {
    match board { "waveshare-lcd7-s3" => WAVESHARE_FIRMWARE, "sunton-3248s035r" => SUNTON_FIRMWARE, _ => &[] }
}
fn stable_version(version: &str) -> Option<(u64, u64, u64)> {
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|s| s.is_empty() || (s.len() > 1 && s.starts_with('0')) || !s.bytes().all(|c| c.is_ascii_digit())) { return None; }
    Some((parts[0].parse().ok()?, parts[1].parse().ok()?, parts[2].parse().ok()?))
}
fn reject_downgrade(installed: &[String], available: &str) -> Result<(), String> {
    let Some(available_version) = stable_version(available) else { return Ok(()); };
    for installed in installed {
        // Four-part development date versions predate component SemVer releases.
        // Their numbers are deliberately not ordered against the new scheme.
        if stable_version(installed).is_some_and(|version| version > available_version) {
            return Err(format!("Installed firmware {installed} is newer than published firmware {available}. Refusing to downgrade this companion."));
        }
    }
    Ok(())
}
fn asset_name(version: &str, board: &str) -> String { format!("esp32-companion-{version}-{board}.zip") }
fn select_release(rows: &[Value]) -> Result<Release, String> {
    let release = rows.iter().filter(|row| row["draft"] == false && row["prerelease"] == false && row["published_at"].is_string())
        .filter_map(|row| Some((stable_version(row["tag_name"].as_str()?.strip_prefix(RELEASE_PREFIX)?)?, row)))
        .max_by_key(|(version, _)| *version).map(|(_, row)| row)
        .ok_or("No published stable ESP32 companion firmware release is available.")?;
    let version = release["tag_name"].as_str().unwrap().strip_prefix(RELEASE_PREFIX).unwrap();
    let rows = release["assets"].as_array().ok_or("The firmware release has no asset list.")?;
    let mut assets = BTreeMap::new();
    for (board, _) in BOARDS {
        let name = asset_name(version, board);
        let matching: Vec<_> = rows.iter().filter(|row| row["name"] == name).collect();
        if matching.len() > 1 { return Err("The firmware release has duplicate board assets.".into()); }
        if let Some(row) = matching.first() {
            let size = row["size"].as_u64().filter(|size| *size > 0 && *size <= MAX_ARCHIVE as u64).ok_or("The firmware release asset exceeds its size limit.")? as usize;
            let expected = format!("https://github.com/opensimgear/cockpit/releases/download/{RELEASE_PREFIX}{version}/{name}");
            if row["browser_download_url"].as_str() != Some(expected.as_str()) { return Err("The firmware release asset URL is invalid.".into()); }
            assets.insert(board.to_owned(), Asset { name, url: expected, size });
        }
    }
    Ok(Release { version: version.into(), assets })
}
fn bounded_read(reader: impl Read, maximum: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader.take(maximum as u64 + 1).read_to_end(&mut bytes).map_err(|e| format!("Could not read firmware data: {e}"))?;
    if bytes.len() > maximum { return Err("Firmware data exceeds its download limit.".into()); }
    Ok(bytes)
}
fn request(url: &str, maximum: usize, deadline: Instant) -> Result<Vec<u8>, String> {
    let timeout = deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()).ok_or("Firmware download timed out.")?;
    let response = ureq::AgentBuilder::new().timeout(timeout).redirects(3).build().get(url)
        .set("User-Agent", "OpenSimGear-Cockpit-firmware")
        .set("Accept", if url.starts_with(API) { "application/vnd.github+json" } else { "application/octet-stream" })
        .call().map_err(|e| format!("Could not download ESP32 companion firmware: {e}"))?;
    if response.header("Content-Length").and_then(|s| s.parse::<u64>().ok()).is_some_and(|size| size > maximum as u64) {
        return Err("Firmware data exceeds its download limit.".into());
    }
    bounded_read(response.into_reader(), maximum)
}
fn release_catalog(deadline: Instant) -> Result<Release, String> {
    let mut rows = Vec::new();
    // A component can be older than the repository's latest desktop/mobile release.
    // Inspect every returned page; refuse an incomplete result at the bounded cap.
    for page in 1..=10 {
        let bytes = request(&format!("{API}?per_page=100&page={page}"), MAX_CATALOG, deadline)?;
        let mut batch: Vec<Value> = serde_json::from_slice(&bytes).map_err(|_| "GitHub returned an invalid firmware release catalog.")?;
        let finished = batch.len() < 100;
        rows.append(&mut batch);
        if finished { return select_release(&rows); }
    }
    Err("The firmware release catalog exceeds the lookup limit. Try again after old development releases are archived.".into())
}

fn validate_archive(bytes: &[u8], board: &str, version: &str) -> Result<Vec<OwnedFirmwareSegment>, String> {
    let target = target(board)?;
    if bytes.len() > MAX_ARCHIVE { return Err("Firmware archive exceeds its size limit.".into()); }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "Invalid firmware ZIP archive.")?;
    let expected: BTreeSet<_> = ["manifest.json", "bootloader.bin", "partition-table.bin", "application.bin", "README.md"].into_iter().collect();
    if archive.len() != expected.len() { return Err("Firmware archive must contain exactly the documented root files.".into()); }
    let mut files = BTreeMap::new();
    let mut total = 0usize;
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(|_| "Cannot read firmware ZIP entry.")?;
        let name = file.name().to_owned();
        if !expected.contains(name.as_str()) || file.is_dir() || file.unix_mode().is_some_and(|mode| mode & 0o170000 == 0o120000) || files.contains_key(&name) {
            return Err("Firmware archive contains an unexpected, duplicate or unsafe path.".into());
        }
        let limit = match name.as_str() { "manifest.json" | "README.md" => 65536, _ => MAX_ARCHIVE };
        if file.size() > limit as u64 { return Err("Firmware ZIP entry exceeds its size limit.".into()); }
        let data = bounded_read(file, limit)?;
        total += data.len();
        if total > MAX_ARCHIVE { return Err("Expanded firmware archive exceeds its size limit.".into()); }
        files.insert(name, data);
    }
    let manifest: Value = serde_json::from_slice(&files["manifest.json"]).map_err(|_| "Invalid firmware manifest JSON.")?;
    let (chip, boot, chip_id) = if board == "sunton-3248s035r" { ("esp32", "0x1000", 0u16) } else { ("esp32s3", "0x0", 9u16) };
    if manifest["schemaVersion"] != 1 || manifest["component"] != "esp32-companion" || manifest["board"] != board || manifest["chip"] != chip || manifest["version"] != version
        || manifest["sourceCommit"].as_str().is_none_or(|sha| sha.len() != 40 || !sha.bytes().all(|c| c.is_ascii_hexdigit())) {
        return Err("Firmware manifest does not match the selected board, chip, version or source commit.".into());
    }
    let records = manifest["segments"].as_array().filter(|records| records.len() == 3).ok_or("Invalid firmware manifest segment list.")?;
    let layout = [(boot, "bootloader.bin"), ("0x8000", "partition-table.bin"), ("0x10000", "application.bin")];
    let mut segments = Vec::new();
    for (record, (offset, name)) in records.iter().zip(layout) {
        let bytes = files.remove(name).ok_or("Firmware segment is missing.")?;
        if record["offset"] != offset || record["file"] != name || record["sha256"].as_str() != Some(format!("{:x}", Sha256::digest(&bytes)).as_str()) {
            return Err("Firmware segment layout or SHA-256 checksum is invalid.".into());
        }
        if name != "partition-table.bin" && (bytes.len() < 24 || u16::from_le_bytes([bytes[12], bytes[13]]) != chip_id) {
            return Err("Firmware image targets the wrong ESP32 chip.".into());
        }
        segments.push(OwnedFirmwareSegment { offset: u32::from_str_radix(&offset[2..], 16).unwrap(), bytes });
    }
    let borrowed: Vec<_> = segments.iter().map(|s| FirmwareSegment { offset: s.offset, bytes: &s.bytes }).collect();
    osg_companion_serial::validate_firmware_for(target, &borrowed)?;
    Ok(segments)
}

fn cached_archive(directory: &Path, board: &str, version: &str) -> Result<Option<Vec<OwnedFirmwareSegment>>, String> {
    let path = directory.join(format!("{board}.zip"));
    let metadata = match std::fs::symlink_metadata(&path) { Ok(metadata) => metadata, Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None), Err(e) => return Err(format!("Cannot inspect firmware cache: {e}")) };
    if !metadata.is_file() || metadata.len() > MAX_ARCHIVE as u64 { return Err("Firmware cache is not a bounded regular file. Remove the invalid cache and retry.".into()); }
    let bytes = bounded_read(std::fs::File::open(&path).map_err(|e| format!("Cannot read firmware cache: {e}"))?, MAX_ARCHIVE)?;
    // A prior component version is replaced on the next install. Corruption of the
    // requested version is reported, so it can never be mistaken for valid data.
    let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).map_err(|_| "Firmware cache is corrupt. Remove the invalid cache and retry.")?;
    let manifest: Value = serde_json::from_slice(&bounded_read(zip.by_name("manifest.json").map_err(|_| "Firmware cache has no manifest.")?, 65536)?).map_err(|_| "Firmware cache manifest is invalid.")?;
    if manifest["version"] != version { return Ok(None); }
    validate_archive(&bytes, board, version).map(Some)
}

impl Resolver {
    /// Retain version observations across the installer releasing the serial port.
    /// A changed USB identity discards the observation before another selection.
    pub(crate) fn observe_ports(&self, ports: &Value, statuses: &[Value]) {
        let Some(ports) = ports.as_array() else { return; };
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.observed.retain(|_, (identity, _, _)| ports.iter().any(|port| ["port", "vid", "pid", "serial"].iter().all(|key| port[key] == identity[key])));
        for port in ports.iter().take(128) {
            let Some(status) = statuses.iter().find(|status| status["connected"] == true && status["port"] == port["port"]) else { continue; };
            if let (Some(path), Some(board), Some(version)) = (port["port"].as_str(), status["device"]["boardProfile"].as_str(), status["device"]["firmwareVersion"].as_str()) {
                if target(board).is_ok() { state.observed.insert(path.into(), (port.clone(), board.into(), version.into())); }
            }
        }
    }
    pub(crate) fn observed_version(&self, port: &str, board: &str) -> Option<String> {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).observed.get(port)
            .filter(|(_, observed_board, _)| observed_board == board).map(|(_, _, version)| version.clone())
    }
    pub(crate) fn metadata(&self) -> Value {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let development = cfg!(debug_assertions);
        let version = if development { LOCAL_FIRMWARE_VERSION } else { state.release.as_ref().map(|release| release.version.as_str()) };
        let profiles: Vec<_> = BOARDS.iter().map(|(id, name)| {
            let profile_version=if development {local_version(id)}else{version};
            let available = if development { profile_version.is_some() && !embedded(id).is_empty() } else { state.error.is_none() && state.release.as_ref().is_some_and(|release| release.assets.contains_key(*id)) };
            let error = if development && !available { Some("Local development firmware has not been built for this board.") }
                else if !development && state.checked && state.error.is_none() && !available { Some("This board's firmware asset is missing from the latest release.") }
                else { state.error.as_deref() };
            json!({"id":id,"name":name,"available":available,"version":profile_version,"source":source(development),"error":error})
        }).collect();
        json!({"firmwareSource":source(development),"firmwareChecked":development || state.checked,"firmwareVersion":version,"firmwareError":state.error,"firmwareProfiles":profiles})
    }
    pub(crate) fn check(&self) {
        if cfg!(debug_assertions) { return; }
        // Another explicit request is already checking or installing; ordinary
        // metadata will observe its result without queuing another long request.
        let Ok(_operation) = self.operation.try_lock() else { return; };
        let _ = self.refresh_catalog(Instant::now() + REQUEST_BUDGET);
    }
    fn refresh_catalog(&self, deadline: Instant) -> Result<(), String> {
        let result = release_catalog(deadline);
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.checked = true;
        match result {
            Ok(release) => { state.release = Some(release); state.error = None; Ok(()) },
            Err(error) => { state.release = None; state.error = Some(error.clone()); Err(error) }
        }
    }
    pub(crate) fn load(&self, directory: &Path, board: &str, installed: &[String]) -> Result<Vec<OwnedFirmwareSegment>, String> {
        target(board)?;
        if cfg!(debug_assertions) {
            let local = embedded(board);
            if local.is_empty() { return Err("Build and stage local development firmware for this board first.".into()); }
            if let Some(version) = local_version(board) { reject_downgrade(installed, version)?; }
            osg_companion_serial::validate_firmware_for(target(board)?, local)?;
            return Ok(local.iter().map(|segment| OwnedFirmwareSegment { offset: segment.offset, bytes: segment.bytes.to_vec() }).collect());
        }
        let deadline = Instant::now() + REQUEST_BUDGET;
        let _operation = self.operation.try_lock().map_err(|_| "Another firmware check or download is running. Wait for it to finish and retry.")?;
        // An explicit install always rechecks published state, with one total
        // network deadline for the catalog and selected firmware asset together.
        let result = self.refresh_catalog(deadline).and_then(|()|self.load_published(directory, board, installed, deadline));
        self.state.lock().unwrap_or_else(|p| p.into_inner()).error = result.as_ref().err().cloned();
        result
    }
    fn load_published(&self, directory: &Path, board: &str, installed: &[String], deadline: Instant) -> Result<Vec<OwnedFirmwareSegment>, String> {
        let release = { let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            state.release.clone().ok_or_else(|| state.error.clone().unwrap_or_else(|| "Check for a published firmware release first.".into()))? };
        let asset = release.assets.get(board).ok_or("The latest firmware release has no asset for this board.")?;
        reject_downgrade(installed, &release.version)?;
        let cache = directory.join("firmware-release-cache");
        std::fs::create_dir_all(&cache).map_err(|e| format!("Cannot create firmware cache: {e}"))?;
        if std::fs::symlink_metadata(&cache).map_err(|e| e.to_string())?.file_type().is_symlink() { return Err("Firmware cache directory must not be a symbolic link.".into()); }
        if let Some(segments) = cached_archive(&cache, board, &release.version)? { return Ok(segments); }
        let bytes = request(&asset.url, asset.size, deadline)?;
        if bytes.len() != asset.size { return Err(format!("Firmware download size differs from the release asset {}.", asset.name)); }
        let segments = validate_archive(&bytes, board, &release.version)?;
        let mut temporary = tempfile::NamedTempFile::new_in(&cache).map_err(|e| format!("Cannot write firmware cache: {e}"))?;
        temporary.write_all(&bytes).map_err(|e| format!("Cannot write firmware cache: {e}"))?;
        temporary.as_file().sync_all().map_err(|e| format!("Cannot save firmware cache: {e}"))?;
        temporary.persist(cache.join(format!("{board}.zip"))).map_err(|e| format!("Cannot save firmware cache: {e}"))?;
        Ok(segments)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(version: &str) -> Value {
        json!({"tag_name":format!("{RELEASE_PREFIX}{version}"),"draft":false,"prerelease":false,
            "published_at":"2026-10-20T12:00:00Z","assets":BOARDS.iter().map(|(board,_)|{
                let name=asset_name(version,board);
                json!({"name":name,"size":1024,"browser_download_url":format!("https://github.com/opensimgear/cockpit/releases/download/{RELEASE_PREFIX}{version}/{name}")})
            }).collect::<Vec<_>>()})
    }
    fn archive(board: &str, change: impl FnOnce(&mut Value, &mut BTreeMap<String, Vec<u8>>)) -> Vec<u8> {
        let mut image=vec![0;24]; image[0]=0xe9; image[12]=if board=="waveshare-lcd7-s3"{9}else{0};
        let mut files=BTreeMap::from([
            ("bootloader.bin".to_owned(),image.clone()),("partition-table.bin".into(),vec![0xaa,0x50]),
            ("application.bin".into(),image),("README.md".into(),b"Flash using Cockpit.".to_vec()),
        ]);
        let layout=[(if board=="waveshare-lcd7-s3"{"0x0"}else{"0x1000"},"bootloader.bin"),("0x8000","partition-table.bin"),("0x10000","application.bin")];
        let mut manifest=json!({"schemaVersion":1,"component":"esp32-companion","version":"1.2.3","board":board,
            "chip":if board=="waveshare-lcd7-s3"{"esp32s3"}else{"esp32"},"sourceCommit":"a".repeat(40),
            "segments":layout.iter().map(|(offset,file)|json!({"offset":offset,"file":file,"sha256":format!("{:x}",Sha256::digest(&files[*file]))})).collect::<Vec<_>>()});
        change(&mut manifest,&mut files);
        files.insert("manifest.json".into(),serde_json::to_vec(&manifest).unwrap());
        let mut writer=zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name,data) in files {
            writer.start_file(name,zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).unwrap();
            writer.write_all(&data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }
    #[test]
    fn selects_component_semver_without_using_the_repository_latest_release() {
        let mut unrelated=release("99.0.0");unrelated["tag_name"]=json!("cockpit-v99.0.0");
        let mut draft=release("11.0.0");draft["draft"]=json!(true);
        let mut preview=release("12.0.0");preview["prerelease"]=json!(true);
        let mut unpublished=release("13.0.0");unpublished["published_at"]=Value::Null;
        let selected=select_release(&[unrelated,draft,preview,unpublished,release("1.9.0"),release("1.10.0"),release("1.10.0-rc.1")]).unwrap();
        assert_eq!(selected.version,"1.10.0");assert_eq!(selected.assets.len(),2);
        assert!(select_release(&[]).is_err());
    }
    #[test]
    fn refuses_untrusted_or_oversized_assets_and_never_substitutes_an_old_release() {
        for (key,value) in [("browser_download_url",json!("https://example.com/firmware.zip")),("size",json!(MAX_ARCHIVE+1))] {
            let mut row=release("1.2.3");row["assets"][0][key]=value;
            assert!(select_release(&[row]).is_err());
        }
        let mut incomplete=release("2.0.0");incomplete["assets"]=json!([]);
        let selected=select_release(&[release("1.0.0"),incomplete]).unwrap();
        assert_eq!(selected.version,"2.0.0");assert!(selected.assets.is_empty());
    }
    #[test]
    fn validates_both_board_images_before_the_serial_worker_can_open_a_port() {
        for board in ["waveshare-lcd7-s3","sunton-3248s035r"] {
            let bytes=archive(board,|_,_|{});
            let images=validate_archive(&bytes,board,"1.2.3").unwrap();
            assert_eq!(images.len(),3);assert_eq!(images[2].offset,0x10000);
            assert!(validate_archive(&bytes,board,"9.9.9").is_err());
            let other=if board=="waveshare-lcd7-s3"{"sunton-3248s035r"}else{"waveshare-lcd7-s3"};
            assert!(validate_archive(&bytes,other,"1.2.3").is_err());
        }
    }
    #[test]
    fn rejects_traversal_corruption_chip_mismatch_and_wrong_offsets() {
        let board="waveshare-lcd7-s3";
        let traversal=archive(board,|_,files|{let readme=files.remove("README.md").unwrap();files.insert("../README.md".into(),readme);});
        assert!(validate_archive(&traversal,board,"1.2.3").unwrap_err().contains("unsafe path"));
        let corrupt=archive(board,|_,files|{files.get_mut("application.bin").unwrap()[1]=1;});
        assert!(validate_archive(&corrupt,board,"1.2.3").unwrap_err().contains("checksum"));
        let chip=archive(board,|manifest,files|{
            files.get_mut("application.bin").unwrap()[12]=0;
            manifest["segments"][2]["sha256"]=json!(format!("{:x}",Sha256::digest(&files["application.bin"])));
        });
        assert!(validate_archive(&chip,board,"1.2.3").unwrap_err().contains("wrong ESP32 chip"));
        for (key,value) in [("offset",json!("0x9000")),("file",json!("../application.bin"))] {
            let invalid=archive(board,|manifest,_|manifest["segments"][2][key]=value);
            assert!(validate_archive(&invalid,board,"1.2.3").is_err());
        }
    }
    #[test]
    fn cache_is_revalidated_and_old_versions_are_never_returned() {
        let directory=tempfile::tempdir().unwrap();let board="waveshare-lcd7-s3";
        assert!(cached_archive(directory.path(),board,"1.2.3").unwrap().is_none());
        let path=directory.path().join(format!("{board}.zip"));
        std::fs::write(&path,archive(board,|_,_|{})).unwrap();
        assert!(cached_archive(directory.path(),board,"1.2.3").unwrap().is_some());
        assert!(cached_archive(directory.path(),board,"1.2.4").unwrap().is_none());
        std::fs::write(&path,archive(board,|_,files|{files.get_mut("application.bin").unwrap()[1]=1;})).unwrap();
        assert!(cached_archive(directory.path(),board,"1.2.3").is_err());
        std::fs::write(&path,b"not a zip").unwrap();
        assert!(cached_archive(directory.path(),board,"1.2.3").is_err());
        std::fs::remove_file(&path).unwrap();std::fs::create_dir(&path).unwrap();
        assert!(cached_archive(directory.path(),board,"1.2.3").is_err());
    }
    #[test]
    fn bounded_downloads_stop_before_archive_parsing() {
        assert_eq!(bounded_read(Cursor::new([1,2,3]),3).unwrap(),[1,2,3]);
        assert!(bounded_read(Cursor::new([1,2,3,4]),3).is_err());
        assert!(request(API,32,Instant::now()-Duration::from_secs(1)).unwrap_err().contains("timed out"));
    }
    #[test]
    fn compiled_source_policy_is_local_only_in_development_and_status_never_fetches() {
        let metadata=Resolver::default().metadata();
        assert_eq!(metadata["firmwareSource"],source(cfg!(debug_assertions)));
        assert!(metadata["firmwareError"].is_null());
        if cfg!(debug_assertions) {
            assert_eq!(metadata["firmwareChecked"],true);
            for profile in metadata["firmwareProfiles"].as_array().unwrap(){assert_eq!(profile["version"],json!(local_version(profile["id"].as_str().unwrap())));assert_eq!(profile["available"],profile["version"].is_string()&&!embedded(profile["id"].as_str().unwrap()).is_empty());}
            assert_eq!(metadata["firmwareVersion"],json!(LOCAL_FIRMWARE_VERSION));
        } else {
            assert!(LOCAL_FIRMWARE_VERSION.is_none());
            assert!(WAVESHARE_FIRMWARE.is_empty() && SUNTON_FIRMWARE.is_empty());
            assert_eq!(metadata["firmwareChecked"],false);
            assert!(metadata["firmwareVersion"].is_null());
        }
    }
    #[test]
    fn refuses_installed_semver_downgrades_before_cache_or_serial_writes_and_allows_legacy_migration() {
        let directory=tempfile::tempdir().unwrap();let board="waveshare-lcd7-s3";
        let resolver=Resolver::default();
        resolver.state.lock().unwrap().release=Some(select_release(&[release("1.2.3")]).unwrap());
        let error=resolver.load_published(directory.path(),board,&["1.3.0".into()],Instant::now()+REQUEST_BUDGET).unwrap_err();
        assert!(error.contains("Refusing to downgrade"));
        assert!(!directory.path().join("firmware-release-cache").exists());
        let cache=directory.path().join("firmware-release-cache");std::fs::create_dir(&cache).unwrap();
        std::fs::write(cache.join(format!("{board}.zip")),archive(board,|_,_|{})).unwrap();
        for installed in ["1.2.2","1.2.3","2026.10.20.1"] {
            assert_eq!(resolver.load_published(directory.path(),board,&[installed.into()],Instant::now()+REQUEST_BUDGET).unwrap().len(),3);
        }
        assert!(reject_downgrade(&["1.0.0".into(),"2.0.0".into()],"1.2.3").is_err());
    }
    #[test]
    fn observed_versions_survive_serial_release_but_never_a_changed_usb_identity() {
        let resolver=Resolver::default();
        let ports=json!([{"port":"COM6","vid":0x1a86,"pid":0x7523,"serial":"screen"}]);
        resolver.observe_ports(&ports,&[json!({"connected":true,"port":"COM6","device":{"boardProfile":"sunton-3248s035r","firmwareVersion":"2.0.0"}})]);
        resolver.observe_ports(&ports,&[]);
        assert_eq!(resolver.observed_version("COM6","sunton-3248s035r").as_deref(),Some("2.0.0"));
        assert!(resolver.observed_version("COM6","waveshare-lcd7-s3").is_none());
        let mut changed=ports;changed[0]["serial"]=json!("other");resolver.observe_ports(&changed,&[]);
        assert!(resolver.observed_version("COM6","sunton-3248s035r").is_none());
    }
    #[test]
    fn ordinary_metadata_remains_available_while_a_firmware_operation_is_pending() {
        use std::sync::{mpsc,Arc};
        let resolver=Arc::new(Resolver::default());
        let (started_tx,started_rx)=mpsc::channel();let (release_tx,release_rx)=mpsc::channel();
        let pending=resolver.clone();
        let operation=std::thread::spawn(move||{
            let _operation=pending.operation.lock().unwrap();
            started_tx.send(()).unwrap();release_rx.recv().unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let (status_tx,status_rx)=mpsc::channel();
        let reader=std::thread::spawn(move||{status_tx.send(resolver.metadata()).unwrap();});
        let status=status_rx.recv_timeout(Duration::from_secs(2));
        release_tx.send(()).unwrap();operation.join().unwrap();reader.join().unwrap();
        assert!(status.is_ok(),"Status waited for a firmware download/check operation");
    }
}
