fn main() {
    // No commands/default permissions: only native Rust can invoke this adapter.
    tauri_plugin::Builder::new(&[])
        .android_path("android")
        .build();
}
