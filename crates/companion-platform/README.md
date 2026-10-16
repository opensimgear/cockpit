# Native Companion platform adapter

This Tauri plugin supplies the Android Keystore `StateStorage` adapter to the standalone Companion. Rust calls it directly through `PluginHandle`; no plugin command is granted to JavaScript. It accepts only bounded credential bytes and a fixed application-owned entry, never arbitrary aliases or filesystem paths.

Android's non-exportable AES-256-GCM key seals an authenticated versioned envelope in private application preferences. A bounded serial worker performs Keystore and commit operations off the UI thread. The Rust bridge validates response/base64/size budgets and the app initializes it asynchronously to avoid blocking mobile callback dispatch.

Key loss, corruption and provider errors fail closed with explicit recovery. Forget removes the fixed encrypted entry/key; no plaintext fallback or automatic re-pair is attempted. Desktop DPAPI/Secret Service/Keychain and native iOS Keychain adapters live in `companion-service`.

**Mobile authentication remains disabled** pending #55/#56. The portable Rust codec test does not establish Android JNI/Kotlin compilation, hardware Keystore behavior, installation or lifecycle qualification. Those native results remain in #55; use the checked-in mobile build commands on a provisioned SDK/device.

References: [Tauri mobile plugin development](https://v2.tauri.app/develop/plugins/develop-mobile/) and [Android Keystore](https://developer.android.com/privacy-and-security/keystore).
