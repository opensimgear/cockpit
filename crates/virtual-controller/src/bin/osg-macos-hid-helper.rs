//! Separately packaged helper; the Cockpit application requires no HID entitlement.
fn main() {
    std::process::exit(osg_virtual_controller::apple_helper::run(
        &std::env::args().skip(1).collect::<Vec<_>>(),
    ));
}
