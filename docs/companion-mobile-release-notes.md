Development preview: mobile authentication remains disabled pending physical-device qualification tracked in opensimgear/cockpit issues #55 and #56.

- Android arm64 debug APK: installable test build using an ephemeral CI debug key. Upgrades across CI runs may require uninstalling the previous test build.
- Android arm64 release APK: included only when persistent signing secrets are configured; use this artifact for consistent signing across upgrades.
- iOS arm64 simulator .app.tar.gz: extract and install with `xcrun simctl install booted <app-path>`. This is a simulator bundle, not an iPhone/iPad installer.
- Device IPA: included only when Apple signing is enabled. App Store Connect export is intended for TestFlight/App Store processing, not direct sideloading.

Builds do not change the existing runtime qualification gates. SHA256SUMS covers the attached artifacts.
