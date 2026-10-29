#[path="games/storage/installations.rs"]
mod game_installations;
#[path="games/artwork/mod.rs"]
mod game_artwork;
#[path="games/library/mod.rs"]
mod game_library;
#[path="games/metadata/mod.rs"]
mod game_metadata;
mod retained_game_companions;
#[path="persistence/paths.rs"]
mod config_paths;
mod persistence;
#[path="mcp/server/mod.rs"]
mod mcp;
#[path="mcp/storage/credentials.rs"]
mod mcp_credentials;
mod mcp_tools;
#[path="companion/storage/widgets.rs"]
mod widget_library;
#[path="companion/storage/host_settings.rs"]
mod companion_settings;
mod acr_setup;
mod ac_car_catalogue;
#[path="games/storage/car_catalogue.rs"]
mod ac_car_catalogue_cache;
#[path="games/telemetry/windows_source/mod.rs"]
mod acr_windows;
#[path="companion/host/mod.rs"]
mod companion_host;
mod firmware_releases;
#[path="companion/storage/panels.rs"]
mod companion_panels;
mod actions;
pub mod dcs_bios;
mod dcs_profile;
mod dcs_physics;
mod dcs_setup;
mod desktop;
#[path="desktop/geometry.rs"]
mod desktop_geometry;
mod window_persistence;
mod game_discovery;
mod game_activation;
mod game_context;
#[path="games/integration/mod.rs"]
mod game_integration_setup;
mod game_telemetry;
mod game_channels;
mod game_telemetry_inspection;
#[path="games/vehicles/mod.rs"]
mod game_vehicles;
mod games;
mod activity;
pub mod gateways;

/// Initialize shared native persistence before constructing source workers.
/// The browser development service uses the same namespace as desktop/MCP.
pub fn initialize_persistence(additional_roots:&[std::path::PathBuf])->Result<osg_persistence::Database,String> {
    persistence::backup::initialize()?;
    let database=config_paths::initialize(additional_roots)?;
    #[cfg(debug_assertions)]
    persistence::development::seed(&database)?;
    Ok(database)
}
mod hid_names;
mod system_inventory;
mod local_gateway;
mod optional_dependencies;
mod module_console;
mod module_usb;
mod module_usb_fleet;
mod module_configuration_routes;
mod profile_resources;
#[path="desktop/progress/mod.rs"]
mod profile_activation_window;
mod profile_system_notification;
mod profile_compilation;
mod profile_debug_controller;
mod profile_desktop_preview;
mod profile_input_learning;
mod profile_retired_recovery;
mod profile_automatic_store;
mod profile_session;
mod profile_session_games;
mod profile_session_companions;
mod profile_session_inputs;
mod profile_session_panels_inputs;
mod profile_session_action_lease;
mod profile_session_actions;
mod profile_session_host;
mod profile_desktop_settings;
mod profile_session_desktop;
mod profile_session_virtual;
mod profile_session_visibility;
mod profile_session_overrides;
mod profile_session_scripts;
mod profile_resource_sources;
mod profile_resource_ports;
mod output_leases;
mod panel_events;
pub mod pico;
mod protocol;
mod runtime;
pub mod stm32;
mod transport;
mod xplane;
mod xplane_installation;

// macOS exposes its system temporary parent through /var -> /private/var.
// Positive storage fixtures use a real parent; explicit symlink tests still
// construct their own unsafe targets. Production storage never follows aliases.
#[cfg(test)]
pub(crate) fn test_directory() -> tempfile::TempDir {
    let parent = std::env::temp_dir();
    #[cfg(unix)]
    let parent = parent.canonicalize().expect("canonical native test temporary parent");
    tempfile::tempdir_in(parent).expect("native test temporary directory")
}

/// Explicit lower-level initialization for retired transport/profile test fixtures.
/// Production IPC always uses Hub::command and accepts compiled publications only.
#[cfg(test)]
pub(crate) fn retain_companion_test_fixture(hub:&osg_companion_service::Hub,request:&serde_json::Value)->Result<serde_json::Value,String>{
    let upload=osg_companion_service::PublicationUpload{
        canonical_document:request["canonicalDocument"].as_str().ok_or("Missing fixture document")?.into(),
        publication_hash:request["publicationHash"].as_str().ok_or("Missing fixture digest")?.into(),
        capability_fingerprint:request["capabilityFingerprint"].as_str().map(str::to_owned),
        assets:serde_json::from_value(request["assets"].clone()).map_err(|e|e.to_string())?,
    };
    hub.retain_legacy_test_fixture(upload)
}

use serde_json::Value;
use std::sync::Arc;
use tauri::Manager;

fn local_document(url: &tauri::Url, development_url: Option<&tauri::Url>) -> bool {
    (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
        || (url.scheme() == "http" && url.host_str() == Some("tauri.localhost"))
        || (cfg!(debug_assertions)
            && development_url.is_some_and(|development| {
                development.scheme() == "http"
                    && matches!(development.host_str(), Some("127.0.0.1") | Some("localhost"))
                    && url.origin() == development.origin()
            }))
}

struct NativeGateway(Arc<gateways::GatewayManager>);

#[cfg(test)] mod local_navigation_tests {
    #[test] fn debug_navigation_uses_only_the_configured_loopback_origin() {
        let development=tauri::Url::parse("http://127.0.0.1:1449/").unwrap();
        assert_eq!(super::local_document(&development,Some(&development)),cfg!(debug_assertions));
        assert!(!super::local_document(&tauri::Url::parse("http://127.0.0.1:1420/").unwrap(),Some(&development)));
        assert!(!super::local_document(&tauri::Url::parse("https://example.invalid/").unwrap(),Some(&development)));
        let remote=tauri::Url::parse("http://example.invalid:1449/").unwrap();
        assert!(!super::local_document(&remote,Some(&remote)));
    }
}

#[tauri::command]
async fn pick_game_executable(
    app: tauri::AppHandle,
    game_id: String,
    current_path: Option<String>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (title, executable) = if game_id.is_empty() {
        ("Choose the game's runtime executable".to_owned(), "Game.exe")
    } else {
        game_discovery::executable_picker(&game_id)?
    };
    tauri::async_runtime::spawn_blocking(move || {
        let mut picker = app
            .dialog()
            .file()
            .set_title(title)
            .add_filter("Game executable", &["exe"])
            .set_file_name(executable);
        if let Some(parent) = current_path
            .as_deref()
            .map(std::path::Path::new)
            .and_then(|p| if p.is_dir() { Some(p) } else { p.parent() })
            .filter(|p| p.is_absolute() && p.is_dir())
        {
            picker = picker.set_directory(parent);
        }
        picker
            .blocking_pick_file()
            .map(|file| {
                let path = file.into_path().map_err(|e| e.to_string())?;
                path.to_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "The executable path is not valid text.".to_owned())
            })
            .transpose()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn pick_game_image(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog().file().set_title("Choose game tile background")
            .add_filter("Game background image", &["png", "jpg", "jpeg", "webp"])
            .blocking_pick_file().map(|file| {
                let path = file.into_path().map_err(|error| error.to_string())?;
                path.to_str().map(str::to_owned).ok_or_else(|| "The image path is not valid text.".to_owned())
            }).transpose()
    }).await.map_err(|error| error.to_string())?
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum GameIntegrationFolder {
    Game,
    SavedGames,
    Catalogue,
}

#[tauri::command]
async fn pick_game_integration_folder(
    app: tauri::AppHandle,
    kind: GameIntegrationFolder,
    current_path: Option<String>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    // The Windows shell parser ignores mixed separators even when Path::is_dir
    // accepts them. Preserve UNC roots when rfd strips a Win32 namespace prefix.
    #[cfg(windows)]
    let current_path = current_path.map(|path| {
        let path = path.replace('/', "\\");
        path.strip_prefix(r"\\?\UNC\")
            .map(|rest| format!(r"\\{rest}"))
            .unwrap_or(path)
    });
    let title = match kind {
        GameIntegrationFolder::Game => "Choose game folder",
        GameIntegrationFolder::SavedGames => "DCS Saved Games folder",
        GameIntegrationFolder::Catalogue => "DCS-BIOS doc/json directory",
    };
    tauri::async_runtime::spawn_blocking(move || {
        let mut picker = app.dialog().file().set_title(title);
        if let Some(directory) = current_path
            .as_deref()
            .map(std::path::Path::new)
            .filter(|p| p.is_absolute() && p.is_dir())
        {
            picker = picker.set_directory(directory);
        }
        picker
            .blocking_pick_folder()
            .map(|folder| {
                let path = folder.into_path().map_err(|e| e.to_string())?;
                path.to_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "The folder path is not valid text.".to_owned())
            })
            .transpose()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn optional_dependencies() -> Value {
    optional_dependencies::list()
}

#[tauri::command]
async fn open_hidhide() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(optional_dependencies::open_hidhide).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn open_vjoy() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(optional_dependencies::open_vjoy).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn optional_dependency_install(app: tauri::AppHandle, id: String) -> Result<String, String> {
    let resources = app.path().resource_dir().map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        optional_dependencies::install(&resources, &id).map(str::to_owned)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn open_companion_browser(app: tauri::AppHandle, state: tauri::State<'_, NativeGateway>, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let service=state.0.clone();
    let expected=tauri::async_runtime::spawn_blocking(move || service.request(serde_json::json!({"command":"companion_status","clientId":"browser-launch-native"}))).await.map_err(|e|e.to_string())?;
    let base=expected["state"]["companions"]["browserUrl"].as_str().ok_or("Enable the companion listener first")?;
    if url != base && !url.strip_prefix(&format!("{base}#pair=")).is_some_and(|s|s.len()==43 && s.bytes().all(|b|b.is_ascii_alphanumeric() || b==b'_' || b==b'-')) {
        return Err("Only the current Cockpit browser companion URL can be opened".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(|e|e.to_string())
}
#[tauri::command]
async fn export_companion_certificate(app: tauri::AppHandle, state: tauri::State<'_, NativeGateway>) -> Result<bool,String> {
    use tauri_plugin_dialog::DialogExt;
    let service=state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result=service.request(serde_json::json!({"command":"companion_browser_certificate","clientId":"browser-certificate"}));
        let pem=result["state"]["companions"]["certificatePem"].as_str().ok_or("Cannot read Cockpit's public certificate")?;
        let Some(file)=app.dialog().file().set_title("Export Cockpit public root CA certificate").set_file_name("opensimgear-cockpit-root-ca.crt").add_filter("Certificate",&["crt"]).blocking_save_file() else {return Ok(false)};
        let path=file.into_path().map_err(|e|e.to_string())?;
        std::fs::write(path,pem).map_err(|e|e.to_string())?;
        Ok(true)
    }).await.map_err(|e|e.to_string())?
}

#[tauri::command]
async fn gateway_request(
    request: Value,
    state: tauri::State<'_, NativeGateway>,
) -> Result<Value, String> {
    let service = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || service.request(request))
        .await
        .map_err(|error| error.to_string())
}

pub fn run() {
    // Helper entry points exit in main before the parent collector exists.
    // Initialize before GatewayManager constructs any native source providers.
    osg_desktop_output::audit::initialize(osg_desktop_output::audit::Role::Parent);
    let builder = tauri::Builder::default();
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
        desktop::show_main(app);
    }));
    let app = builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri::plugin::Builder::<tauri::Wry, ()>::new("local-navigation")
                .on_navigation(|window, url| local_document(url, window.app_handle().config().build.dev_url.as_ref()))
                .build(),
        )
        .setup(|app| {
            app.manage(initialize_persistence(&[app.path().app_config_dir()?])?);
            let restart_handle=app.handle().clone();
            persistence::backup::bind_restart(Box::new(move||restart_handle.restart()));
            let gateway = Arc::new(gateways::GatewayManager::new());
            {
                use tauri::Emitter;
                let handle=app.handle().clone();
                gateway.ui_preferences.observe(Arc::new(move|key,revision|{
                    let _=handle.emit("ui-preferences-changed",serde_json::json!({"key":key,"revision":revision}));
                    if key=="progress"{profile_activation_window::preferences_changed(&handle);}
                }));
            }
            gateway.start_automatic();
            app.manage(mcp::Server::native(gateway.clone()));
            app.manage(NativeGateway(gateway));
            app.manage(profile_activation_window::ActivationWindowState::default());
            desktop::setup(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            persistence::persistence_status,
            persistence::persistence_workspace,
            persistence::ui::commands::persistence_ui_read,
            gateway_request,
            mcp::mcp_settings_get,
            mcp::mcp_settings_update,
            mcp::mcp_credential_create,
            mcp::mcp_credential_revoke,
            mcp::mcp_credential_rotate,
            profile_activation_window::profile_activation_window_open,
            profile_activation_window::profile_activation_window_update,
            profile_activation_window::profile_activation_window_state,
            profile_activation_window::profile_activation_window_cancel,
            profile_activation_window::profile_activation_window_close,
            profile_activation_window::profile_progress_settings_get,
            profile_activation_window::profile_progress_settings_save,
            open_companion_browser,
            export_companion_certificate,
            optional_dependencies,
            open_hidhide,
            open_vjoy,
            optional_dependency_install,
            pick_game_executable,
            pick_game_image,
            pick_game_integration_folder,
            desktop::desktop_preferences,
            desktop::set_tray_preferences,
            desktop::set_autostart
        ])
        .on_window_event(|window, event| {
            desktop::window_event(window, event);
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Destroyed) {
                window.state::<Arc<mcp::Server>>().shutdown();
                window.state::<NativeGateway>().0.shutdown();
                window.app_handle().exit(0);
            }
        })
        .build(tauri::generate_context!())
        .expect("could not start OpenSimGear Cockpit");
    app.run(|handle, event| {
        if matches!(event, tauri::RunEvent::MainEventsCleared) {
            desktop::hide_if_minimized(handle);
        }
        if matches!(
            event,
            tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
        ) {
            desktop::shutdown(handle);
            handle.state::<profile_activation_window::ActivationWindowState>().shutdown();
            handle.state::<Arc<mcp::Server>>().shutdown();
            handle.state::<NativeGateway>().0.shutdown();
            if matches!(event,tauri::RunEvent::Exit){persistence::backup::release_namespace();}
        }
    });
}
