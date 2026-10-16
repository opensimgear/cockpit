# Companion C3 mobile application — 16 October 2026

The retained mobile app has a separate identity, narrow native API, denied remote navigation, verified four-panel/24 MiB public cache, guarded asynchronous sessions and explicit reconnect/Start behavior. Credentials and action queues never enter the public cache. Android uses an app-owned Keystore bridge; iOS uses Keychain. Authentication remains disabled pending #55/#56.

Desktop shells, installers, desktop lifecycle checks and their reproduction commands have been retired. Use the [browser companion](../browser-companion.md) on desktop and the [mobile instructions](../../apps/companion/README.md) for Android/iOS.

Historical shared app helper validation recorded 21 passing tests; the Android portable codec recorded one passing test. These are historical software results, not physical mobile qualification. The Android build job is retained in `companion-mobile.yml`; iOS requires macOS/Xcode and provisioning.

Reproduce shared checks with `pnpm companion:check`, `pnpm companion:test` and `pnpm companion:ui:build`. Native mobile builds require the respective SDKs. Device storage/trust, touch, suspend/resume, LAN permissions and signing remain #55/#56; mixed-peer load and display checks remain #58/#59. No host capability gate changes.
