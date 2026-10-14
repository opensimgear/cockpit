// Read-only dependency and capability inspection; never acquire, reset or provision.
fn main() {
    #[cfg(windows)]
    match osg_virtual_controller::windows::Native::open()
        .and_then(|mut driver| osg_virtual_controller::vjoy::inventory(&mut driver))
    {
        Ok(slots) => println!("{}", serde_json::to_string_pretty(&slots).unwrap()),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
    #[cfg(not(windows))]
    println!("vJoy is a Windows backend; recording preview needs no driver.");
}
