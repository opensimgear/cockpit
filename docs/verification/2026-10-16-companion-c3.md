# Companion C3 mobile application — 16 October 2026

The retained mobile app has a separate identity, narrow native API, denied remote navigation, verified four-panel/24 MiB public cache, guarded asynchronous sessions and explicit reconnect/Start behavior. Credentials and action queues never enter the public cache. Android uses an app-owned Keystore bridge; iOS uses Keychain. Authentication remains disabled pending #55/#56.

Desktop shells, installers, desktop lifecycle checks and their reproduction commands have been retired. Use the [browser companion](../browser-companion.md) on desktop and the [mobile instructions](../../apps/companion/README.md) for Android/iOS.

Historical shared app helper validation recorded 21 passing tests; the Android portable codec recorded one passing test. These are historical software results, not physical mobile qualification. The Android build job is retained in `companion-mobile.yml`; iOS requires macOS/Xcode and provisioning.

Reproduce shared checks with `pnpm companion:check`, `pnpm companion:test` and `pnpm companion:ui:build`. Native mobile builds require the respective SDKs. Device storage/trust, touch, suspend/resume, LAN permissions and signing remain #55/#56; mixed-peer load and display checks remain #58/#59. No host capability gate changes.

## Discovered short-code pairing endpoint — 9 November 2026

Cockpit advertises the configured pairing hostname or IP in DNS-SD TXT
alongside the listener port. The mobile form uses the exact endpoint
returned by the native client. Discovery entries require the host field
and carry no pairing authority. The short-code exchange still compares
the selected endpoint with the authenticated offer and verifies the
pinned CA and server name before presenting the pairing secret.

The native Windows transport test used multicast discovery to find a
private listener advertised as `LOCALHOST`. It verified the exact endpoint,
completed short-code pairing and tested presence, assignment, credential
reload and revocation. Unit tests exercised hostname and IPv6 formatting,
the 253-byte hostname bound and rejection of malformed hints. Reproduce
the multicast run with `OSG_TEST_PAIRING_DISCOVERY=1` and the
`short_code_pairing_persists_both_ends_tracks_presence_and_revocation` test.

All 243 Windows companion library tests passed. LLVM measured 92.36% line
and 80.00% function coverage for discovery, 87.66% and 73.13% for the
server, and 95.21% and 90.84% for the client. This instrumentation did
not collect branches. The mobile frontend passed 35 tests, its coverage
gate, Svelte diagnostics and the production frontend build. The local
test and coverage output is under `.research/reactive/discovery-pairing-endpoint/`.

The rebuilt native client probe also passed pinned pairing, presence
handoff, stalled TLS cancellation and Forget against the running Windows
dev Cockpit at `f993800a`, using its existing manual pairing path. The
fixture reported no cleanup errors. Its trace is
`.research/reactive/companion-client-io-native/1794214946358/evidence.json`
with SHA-256 `daf2b8da9c11a93da1ea8dfd274b2f161b3d403b6353cf6e69e40f94dd33a340`.

Signed Android and iOS installation and actual device discovery remain
open.

## iOS discovery through Bonjour — 9 November 2026

The iOS client browses the declared `_osg-companion._tcp` service through
Bonjour. One connection waits for readiness and resolves the service on
its reported interface. A bounded set of operations tracks the current
services and drops a result when its service disappears. The TXT host
and SRV port produce the exact untrusted pairing endpoint. IP literals
are included as diagnostic addresses. A Local Network policy denial
reports where to grant permission before trying again.

Apple requires a restricted entitlement for raw multicast on iOS.
Browsing the declared service through Bonjour uses the app's Local
Network permission. The plist describes discovery and connection.
[Apple local network requirements](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy)

The Windows companion suite passed 245 tests, including Bonjour TXT
length, key, endpoint and byte-order tests. The opt-in Windows multicast
short-code pairing test also passed. LLVM measured 93.21% lines and
82.35% functions in shared discovery and 100% of both in the TXT decoder.
This instrumentation did not collect branches. The Bonjour adapter and
shared code passed a type check against the installed Apple Silicon Darwin
Rust target in an isolated harness. Artifacts are under
`.research/reactive/ios-bonjour/`.

The rebuilt Windows client also passed the live dev Cockpit manual
pairing, presence, TLS cancellation and Forget regression on `f993800a`.
The trace is `.research/reactive/companion-client-io-native/1794215466772/evidence.json`,
SHA-256 `882b9e4374e2038e3633b04a374a7c4e170c741bdc4cf5965fab0a729a8db4aa`.

Linking the iOS app and checking Local Network denial and regrant,
discovery, and pairing on a signed device remain in
[#56](https://github.com/opensimgear/cockpit/issues/56) and the operator
follow-up [#199](https://github.com/opensimgear/cockpit/issues/199).

## Android Wi-Fi discovery lease — 9 November 2026

The Android shell acquires a Wi-Fi multicast lock for an explicit
nearby-host request, with the manifest's normal multicast permission.
Rust releases the matching lock on completion. Activity pause or
destruction releases it early, and a four-second one-shot deadline
retires an abandoned bridge call. A second request cannot acquire the
lock while the first holds it. A late finish cannot release another
request's lock.
[Android multicast lock reference](https://developer.android.com/reference/android/net/wifi/WifiManager.MulticastLock)

The Android dependency graph resolved from the locked manifests and the
portable Rust platform codec test passed with full coverage of its
Windows-compiled code. The manifest parsed and contains the multicast
permission. The packaging helpers passed 11 of 12 tests on Windows.
The simulator tarball test needs POSIX executable mode bits. The native
Windows multicast and short-code pairing fixture passed.

Android SDK/JDK compilation, signed-device Wi-Fi discovery and lock
release on pause remain in [#55](https://github.com/opensimgear/cockpit/issues/55).
Builds targeting API 37 still need a runtime Local Network permission
path for discovery and WSS.
[Android Local Network permission](https://developer.android.com/privacy-and-security/local-network-permission)
