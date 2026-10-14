#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(code) = osg_virtual_controller::entry::helper_main() {
        std::process::exit(code);
    }
    if let Some(code) = osg_virtual_controller::inspection::helper_main() {
        std::process::exit(code);
    }
    osg_cockpit::run();
}
