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
open. The iOS raw multicast client still needs platform work. Apple
requires a restricted entitlement for raw multicast on iOS and supports
declared service browsing through Bonjour.
[Apple local network requirements](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy)
