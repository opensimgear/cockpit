# Native Companion platform adapter

This Tauri plugin supplies the Android Keystore `NativeSealer` adapter to the standalone Companion. Rust calls it directly through `PluginHandle`; no plugin command is granted to JavaScript. It accepts only bounded plaintext/ciphertext bytes and a fixed key alias, never arbitrary aliases or filesystem paths.

Android's non-exportable AES-256-GCM key seals the existing authenticated envelope. The shared native store writes only ciphertext into the Companion application's SQLite namespace; the plugin has no preferences/file read, write or delete authority. A bounded serial worker performs Keystore operations off the UI thread. The Rust bridge validates response/base64/size budgets and the app initializes it asynchronously to avoid blocking mobile callback dispatch.

Key loss, corruption and provider errors fail closed with explicit recovery. The SQLite owner verifies existing ciphertext before any key-creating probe or seal, and Forget commits a revisioned tombstone. Protection keys remain external. No plaintext fallback or automatic re-pair is attempted. Desktop DPAPI/Secret Service/Keychain and native iOS Keychain adapters live in `companion-service`.

**Mobile authentication remains disabled** pending #55/#56. The portable Rust codec test does not establish Android JNI/Kotlin compilation, hardware Keystore behavior, installation or lifecycle qualification. Those native results remain in #55; use the checked-in mobile build commands on a provisioned SDK/device.

References: [Tauri mobile plugin development](https://v2.tauri.app/develop/plugins/develop-mobile/) and [Android Keystore](https://developer.android.com/privacy-and-security/keystore).
