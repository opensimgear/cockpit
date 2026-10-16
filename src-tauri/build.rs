fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "gateway_request",
            "optional_dependencies",
            "optional_dependency_install",
            "pick_game_executable",
            "desktop_preferences",
            "set_tray_preferences",
            "set_autostart",
        ]),
    ))
    .expect("could not build Cockpit desktop resources");
}
