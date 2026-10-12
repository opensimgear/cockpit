fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "pico_gateway",
            "corehid_gateway",
            "open_pico_flashing_guide",
            "export_pico_firmware",
        ]),
    ))
    .expect("could not build Cockpit desktop resources");
}
