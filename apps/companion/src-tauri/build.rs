fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").expect("target OS");
    assert!(matches!(target.as_str(), "android" | "ios"),
        "The native Companion supports Android and iOS only; use the browser companion on desktop.");
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new()
                .commands(&["companion_lifecycle", "companion_client_request"]),
        ),
    )
    .expect("failed to build Companion command permissions");
}
