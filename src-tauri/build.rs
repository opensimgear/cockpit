#[path="src/local_firmware.rs"]
mod local_firmware;
fn main() {
    let root = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let files = ["bootloader.bin", "partition-table.bin", "application.bin"];
    let mut generated=String::new();
    let mut versions=Vec::new();
    for (profile,symbol,boot) in [("waveshare-lcd7-s3","WAVESHARE_FIRMWARE",0),("sunton-3248s035r","SUNTON_FIRMWARE",0x1000)] {
        let board=root.join("resources/firmware").join(profile);
        println!("cargo:rerun-if-changed={}",board.display());
        let bytes=files.iter().map(|file|std::fs::read(board.join(file))).collect::<Result<Vec<_>,_>>();
        let verified=bytes.ok().and_then(|bytes|{
            let manifest=std::fs::read(board.join("manifest.json")).ok()?;
            let segments=bytes.iter().zip([boot,0x8000,0x10000]).map(|(bytes,offset)|(offset,bytes.as_slice())).collect::<Vec<_>>();
            match local_firmware::bundle_version(&manifest,profile,&segments){Ok(version)=>Some(version),Err(error)=>{println!("cargo:warning={profile}: {error}");None}}
        });
        let entries=if verified.is_some() {
            files.iter().zip([boot,0x8000,0x10000]).map(|(file,offset)|format!("osg_companion_serial::FirmwareSegment{{offset:{offset},bytes:include_bytes!({:?})}}",board.join(file))).collect::<Vec<_>>().join(",")
        }else{String::new()};
        generated.push_str(&format!("#[cfg(debug_assertions)]\npub const {symbol}_VERSION: Option<&str> = {:?};\n#[cfg(not(debug_assertions))]\npub const {symbol}_VERSION: Option<&str> = None;\n",verified.as_deref()));
        generated.push_str(&format!("#[cfg(debug_assertions)]\npub static {symbol}: &[osg_companion_serial::FirmwareSegment<'static>] = &[{entries}];\n#[cfg(not(debug_assertions))]\npub static {symbol}: &[osg_companion_serial::FirmwareSegment<'static>] = &[];\n"));
        versions.push(verified);
    }
    let common=versions.first().and_then(|version|version.as_deref()).filter(|version|versions.iter().all(|other|other.as_deref()==Some(*version)));
    generated.push_str(&format!("#[cfg(debug_assertions)]\npub const LOCAL_FIRMWARE_VERSION: Option<&str> = {common:?};\n#[cfg(not(debug_assertions))]\npub const LOCAL_FIRMWARE_VERSION: Option<&str> = None;\n"));
    std::fs::write(std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("companion_firmware.rs"),generated).unwrap();

    let browser = root.join("../apps/browser-companion/dist");
    println!("cargo:rerun-if-changed={}", browser.display());
    let mut entries = Vec::new();
    fn collect(directory: &std::path::Path, root: &std::path::Path, entries: &mut Vec<String>) {
        if !directory.is_dir() { return; }
        let mut files = std::fs::read_dir(directory).unwrap().map(|v|v.unwrap().path()).collect::<Vec<_>>();
        files.sort();
        for path in files {
            if path.is_dir() { collect(&path, root, entries); continue; }
            let relative = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            let route = if relative=="index.html" { "/".to_string() } else { format!("/{relative}") };
            let mime = match path.extension().and_then(|e|e.to_str()).unwrap_or("") {
                "html"=>"text/html; charset=utf-8", "js"=>"text/javascript; charset=utf-8",
                "css"=>"text/css; charset=utf-8", "webmanifest"=>"application/manifest+json",
                "png"=>"image/png", _=>"application/octet-stream",
            };
            entries.push(format!("osg_companion_service::browser::BrowserAsset{{path:{route:?},mime:{mime:?},bytes:include_bytes!({path:?})}}"));
        }
    }
    collect(&browser, &browser, &mut entries);
    std::fs::write(std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("companion_browser.rs"),
        format!("static BROWSER_ASSETS: &[osg_companion_service::browser::BrowserAsset] = &[{}];", entries.join(","))).unwrap();

    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "persistence_status",
            "persistence_workspace",
            "gateway_request",
            "mcp_settings_get",
            "mcp_settings_update",
            "mcp_credential_create",
            "mcp_credential_revoke",
            "profile_activation_window_open",
            "profile_activation_window_update",
            "profile_activation_window_state",
            "profile_activation_window_cancel",
            "profile_activation_window_close",
            "profile_progress_settings_get",
            "profile_progress_settings_save",
            "open_companion_browser",
            "export_companion_certificate",
            "optional_dependencies",
            "open_hidhide",
            "open_vjoy",
            "optional_dependency_install",
            "pick_game_executable",
            "pick_game_image",
            "pick_game_integration_folder",
            "desktop_preferences",
            "set_tray_preferences",
            "set_autostart",
        ]),
    ))
    .expect("could not build Cockpit desktop resources");
}
