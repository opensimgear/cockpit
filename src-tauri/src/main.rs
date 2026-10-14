#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    if let Some(code) = osg_virtual_controller::windows_provision_entry::helper_main() {
        std::process::exit(code);
    }
    if let Some(code) = osg_virtual_controller::entry::helper_main() {
        std::process::exit(code);
    }
    if let Some(code) = osg_virtual_controller::inspection::helper_main() {
        std::process::exit(code);
    }
    osg_cockpit::run();
}
