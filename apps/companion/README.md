# OpenSimGear Companion

A separate **Tauri 2, Svelte 5 and TypeScript** panel app for Android and iOS. Desktop users use the [browser companion](../../docs/browser-companion.md). It uses Cockpit's shared canvas and connects as a narrow authenticated client to the same host endpoint. It does not link Cockpit's gateway, game-launch or controller-driver services, and needs no vJoy/HidHide on the client computer.

## Pair and run a panel

1. Publish a panel in Cockpit, enable its Companion listener on the intended interface/port, and make the immutable revision available.
2. Transfer the host's pairing payload directly to this app. Discovery is an untrusted hint; the native client checks the out-of-band CA fingerprint and ordinary TLS chain/SAN/validity/signatures before sending a one-time secret.
3. Request pairing and approve/assign the installation in Cockpit. Refresh assignments, select a controller instance and Connect. Verified layouts/assets and a fresh acknowledged neutral baseline must arrive before **Start panel**.
4. Start explicitly. Stop, page/orientation/layout change, focus/close/background loss, native invalidation, stalled UI, network failure or overflow disarms this instance. Reconnect and Start explicitly; old actions are never replayed.

Source acknowledgments say `source-accepted` with `submitted: false`. They do not claim successful virtual output or a game action. Existing unqualified host output, provisioning, hiding and script gates are unchanged.

## Offline content and recovery

The app atomically caches up to **four verified public panels, 24 MiB total, at most 13 MiB per entry**, including raster assets and the last selected page. Every disk read revalidates the document, capability/content hashes and assets. Credentials and input/action queues never enter IndexedDB. Quota/corruption errors leave online use available; **Clear saved panels** explicitly resets the public cache.

Use **Open offline** to view a saved panel after disconnection/restart. Controls remain disarmed and feedback is unavailable. A cached panel grants no authority: Connect again for a new generation/baseline and explicitly Start. Feedback is cleared on epoch/selection/lifecycle loss. Late asynchronous responses cannot rearm an obsolete view.

Native credential adapters are distinct from this public cache:

| Platform | Native protection | Qualification boundary |
| --- | --- | --- |
| Android | App-owned Android Keystore AES-GCM adapter | Native bridge wired; authentication remains disabled pending #55. |
| iOS/iPadOS | Native Apple Keychain adapter | Authentication remains disabled pending #56. |

There is no plaintext credential fallback. **Retry protected storage** re-probes a locked store and restores valid saved authority disarmed. Corrupt state remains inspectable; **Forget saved pairing** explicitly deletes only this app's saved credential before allowing a new installation identity. Failed deletion preserves existing authority. Forgetting pairing and clearing public panels are separate actions.

Only the trusted local main window gets two narrow application commands (lifecycle/client request). Remote navigation is denied. The Android native storage plugin grants no JavaScript commands. No JavaScript network/filesystem/shell/controller-driver permissions are added.

## Development and native builds

Run from the repository root with the pinned Rust toolchain, Node and pnpm:

```text
pnpm install --frozen-lockfile
pnpm companion:check
pnpm --filter @opensimgear/companion check
pnpm companion:test
pnpm --filter @opensimgear/companion test
pnpm companion:ui:dev
```

Browser preview uses `http://127.0.0.1:1431` and has no native pairing/remote input. Local preview records shared control transitions only; it is separate from offline cached viewing.

The native shell builds only for Android and iOS. Desktop installer builds and native desktop execution are unavailable.

Mobile SDK entry points:

```text
pnpm --filter @opensimgear/companion android:init
pnpm --filter @opensimgear/companion android:dev
pnpm --filter @opensimgear/companion android:build:dev
pnpm --filter @opensimgear/companion ios:init
pnpm --filter @opensimgear/companion ios:dev
pnpm --filter @opensimgear/companion ios:build:dev
```

Android needs the configured SDK/NDK/JDK; iOS needs macOS/Xcode and signing/provisioning. Generated projects are ignored and reconstructed from the checked-in Tauri configuration/native plugin. Debug APKs use Android development signing; this is not release qualification. Candidate floors are Android API29 and iOS15. These are configuration choices, not a verified support matrix.

Mobile physical install, credential/permission and lifecycle checks remain open in #55/#56. See [C3 verification](../../docs/verification/2026-10-16-companion-c3.md) and [the feature guide](../../docs/companion-uis.md).

## C6 guides and release status

Use the [companion user guide](../../docs/companion-guide.md) for setup, designer,
pairing, feedback and recovery; start from the [portable example layouts](../../examples/companion/README.md).
The [release matrix](../../docs/companion-release-matrix.md) and
`python scripts/companion-release-check.py` record the current blocked release
state and pending native qualification. No browser/Android/iOS client
is release-qualified. Debug APKs and local fixtures do not establish signed/native upgrade, game or latency results.
Signed distribution and cross-version trust/rollback remain #68, reusing #55/#56.
## Mobile CI and download builds

The app stays in Cockpit. [Companion mobile CI](../../.github/workflows/companion-mobile.yml)
runs on pull requests and pushes to `main` only when mobile source, its shared
packages/native crates, dependency configuration, toolchain, or mobile CI files change.
Unrelated desktop, browser-only and ESP32 changes do not trigger mobile builds.
Manual dispatch can explicitly build any branch; its `publish` option works only on `main`.

Checks cover the shared frontend, app lifecycle/cache tests, frontend bundle, artifact
packaging and native companion-service tests. Android and iOS builds run separately
on Ubuntu and macOS after checks pass. General Cockpit CI retains shared/host tests;
mobile app checks and bundling live in this workflow.

Successful affected pushes to `main` publish a development prerelease named
`mobile-build-<run-number>` with SHA256 checksums, an arm64 debug APK and an arm64
iOS simulator `.app.tar.gz`. Reruns reuse the same release. These prereleases do not
replace the repository's latest stable release. No tag-push trigger is used because
GitHub does not apply path filters to tag pushes.

The debug APK is installable for testing. CI generates a new debug signing key per
run, so replacing an older build can require uninstalling it and losing its local
app data. To additionally publish a consistently signed release APK, configure all
four repository secrets: `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`,
`ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`. Keep the keystore backed up securely.
The release APK uses the same app identifier as the debug APK; switching keys requires
uninstalling the other build first. No signing key is generated or committed by this workflow.

The simulator bundle requires an Apple Silicon Mac with Xcode. Extract the archive,
boot an iOS simulator, then run `xcrun simctl install booted "<path-to-app>"`.
It cannot be installed on a physical iPhone or iPad.

Device IPA export is optional: set repository variables `IOS_SIGNING_ENABLED=true`
and `APPLE_DEVELOPMENT_TEAM` to your Apple team ID, and secrets `IOS_CERTIFICATE`
(base64 P12), `IOS_CERTIFICATE_PASSWORD`, and `IOS_MOBILE_PROVISION` (base64 App Store
Connect provisioning profile for `org.opensimgear.companion`). Only trusted publishing
runs use these credentials. The IPA uses the App Store Connect export method for
TestFlight/App Store processing; CI attaches it to the prerelease and does not upload
it to Apple or make it directly sideloadable. See [Tauri's signing guide](https://v2.tauri.app/distribute/sign/ios/).

These are development artifacts. Mobile authentication remains gated pending #55/#56;
CI builds and signing do not establish physical-device qualification.
